//! The Settings screen: module forms and engine sections generated from the schemas,
//! module health per account, and how edits are written back.

use std::cell::RefCell;

use serde_json::Value;
use slint::{ComponentHandle, ModelRc, VecModel};
use tm_core::State;
use tm_core::state::ModuleHealth;

use crate::forms::{self, Field};
use crate::present;
use crate::{Bridge, FieldItem, HealthItem, HourWindow, MainWindow, ModuleForm};

/// Settings shown on another screen (one main screen per piece of data), or with a
/// dedicated control, per section.
fn hidden(section: &str) -> &'static [&'static str] {
    match section {
        "drops" => &["enabled", "games", "exclude_games", "wanted_rewards"],
        "points" => &["enabled", "channels", "exclude", "follows", "order_by"],
        "engine" => &["paused", "active_hours", "agent_enabled"],
        _ => &["enabled"],
    }
}

/// Engine settings per section of the screen; anything else goes to maintenance.
const ENGINE_MAIN: &[&str] = &["max_watch_slots"];
const ENGINE_NETWORK: &[&str] = &["proxy"];
const ENGINE_PEERS: &[&str] = &["role", "peers"];

struct ModuleSchema {
    id: String,
    name: String,
    description: String,
    defaults: Value,
    fields: Vec<Field>,
}

#[derive(Default)]
struct Schemas {
    modules: Vec<ModuleSchema>,
    engine_defaults: Value,
    engine_fields: Vec<Field>,
    loaded: bool,
}

/// What the forms were last built from (they are rebuilt only when it changes, so a
/// field being edited is not reset by unrelated state updates).
#[derive(PartialEq)]
struct Built {
    settings: std::collections::BTreeMap<String, Value>,
    account: String,
    modules: Vec<(String, String)>,
}

thread_local! {
    static SCHEMAS: RefCell<Schemas> = RefCell::new(Schemas::default());
    static BUILT: RefCell<Option<Built>> = const { RefCell::new(None) };
}

/// `modules_list` and `settings_get("engine")` replies.
pub fn set_schemas(modules: &Value, engine: &Value) {
    let modules = modules
        .as_array()
        .map(|list| {
            list.iter()
                .map(|m| {
                    let id = m["id"].as_str().unwrap_or_default().to_string();
                    ModuleSchema {
                        fields: forms::fields(&m["settings_schema"], hidden(&id)),
                        name: m["name"].as_str().unwrap_or_default().to_string(),
                        description: m["description"].as_str().unwrap_or_default().to_string(),
                        defaults: m["default_settings"].clone(),
                        id,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    SCHEMAS.with(|s| {
        *s.borrow_mut() = Schemas {
            modules,
            engine_defaults: engine["defaults"].clone(),
            engine_fields: forms::fields(&engine["schema"], hidden("engine")),
            loaded: true,
        }
    });
    BUILT.with(|b| *b.borrow_mut() = None);
}

pub fn field(section: &str, path: &str) -> Option<Field> {
    SCHEMAS.with(|s| {
        let s = s.borrow();
        let fields = if section == "engine" {
            &s.engine_fields
        } else {
            &s.modules.iter().find(|m| m.id == section)?.fields
        };
        fields.iter().find(|f| f.path == path).cloned()
    })
}

pub fn defaults(section: &str) -> Value {
    SCHEMAS.with(|s| {
        let s = s.borrow();
        if section == "engine" {
            s.engine_defaults.clone()
        } else {
            s.modules
                .iter()
                .find(|m| m.id == section)
                .map(|m| m.defaults.clone())
                .unwrap_or(Value::Null)
        }
    })
}

/// The value the engine uses: defaults ⊕ shared scope ⊕ account override.
pub fn effective(state: &State, account: &str, section: &str) -> Value {
    if section == "engine" {
        return serde_json::to_value(tm_core::settings::EngineSettings::from_state(state))
            .unwrap_or_default();
    }
    tm_core::settings::module_effective(state, account, section, &defaults(section))
}

/// Scope to write a setting of `section` to (by its top-level key).
pub fn scope_for(state: &State, account: &str, section: &str, path: &str) -> String {
    if section == "engine" {
        return "engine".into();
    }
    let top = path.split('.').next().unwrap_or(path);
    present::module_setting_scope(state, account, section, top)
}

pub fn render(ui: &MainWindow, state: &State) {
    let b = ui.global::<Bridge>();
    let account = b.get_account().to_string();

    // Module health of every account (filtered by the view).
    let health: Vec<HealthItem> = state
        .accounts
        .iter()
        .flat_map(|(id, acc)| {
            acc.modules.iter().map(move |(m, h)| {
                let (st, err) = match h {
                    ModuleHealth::Disabled => ("disabled", String::new()),
                    ModuleHealth::Running => ("running", String::new()),
                    ModuleHealth::Stopped => ("stopped", String::new()),
                    ModuleHealth::Restarting { error, .. } => ("restarting", error.clone()),
                };
                HealthItem {
                    account: id.as_str().into(),
                    module: m.as_str().into(),
                    state: st.into(),
                    error: err.into(),
                }
            })
        })
        .collect();
    present::sync(b.get_module_health(), health, |m| b.set_module_health(m));

    let engine = effective(state, &account, "engine");
    b.set_agent_enabled(engine["agent_enabled"].as_bool().unwrap_or_default());

    let built = Built {
        settings: state.settings.clone(),
        account: account.clone(),
        modules: SCHEMAS.with(|s| {
            s.borrow()
                .modules
                .iter()
                .map(|m| (m.id.clone(), m.name.clone()))
                .collect()
        }),
    };
    let unchanged = BUILT.with(|b| b.borrow().as_ref() == Some(&built));
    if unchanged || !SCHEMAS.with(|s| s.borrow().loaded) {
        return;
    }

    SCHEMAS.with(|s| {
        let s = s.borrow();
        let forms: Vec<ModuleForm> = s
            .modules
            .iter()
            .map(|m| {
                let eff = effective(state, &account, &m.id);
                ModuleForm {
                    id: m.id.as_str().into(),
                    name: m.name.as_str().into(),
                    description: m.description.as_str().into(),
                    enabled: eff["enabled"].as_bool().unwrap_or(true),
                    fields: ModelRc::new(VecModel::from(forms::items(
                        &m.id,
                        &m.fields,
                        &eff,
                        &m.defaults,
                    ))),
                }
            })
            .collect();
        b.set_module_forms(ModelRc::new(VecModel::from(forms)));

        let section = |keys: Option<&[&str]>| -> ModelRc<FieldItem> {
            let all: &[&str] = &[ENGINE_MAIN, ENGINE_NETWORK, ENGINE_PEERS].concat();
            let fields: Vec<Field> = s
                .engine_fields
                .iter()
                .filter(|f| {
                    let top = f.path.split('.').next().unwrap_or(&f.path);
                    match keys {
                        Some(k) => k.contains(&top),
                        None => !all.contains(&top),
                    }
                })
                .cloned()
                .collect();
            ModelRc::new(VecModel::from(forms::items(
                "engine",
                &fields,
                &engine,
                &s.engine_defaults,
            )))
        };
        b.set_engine_main(section(Some(ENGINE_MAIN)));
        b.set_engine_network(section(Some(ENGINE_NETWORK)));
        b.set_engine_peers(section(Some(ENGINE_PEERS)));
        b.set_engine_maintenance(section(None));
    });

    let hours: Vec<HourWindow> = engine["active_hours"]
        .as_array()
        .map(|list| list.iter().map(hour_window).collect())
        .unwrap_or_default();
    b.set_active_hours(ModelRc::new(VecModel::from(hours)));
    BUILT.with(|b| *b.borrow_mut() = Some(built));
}

fn hour_window(w: &Value) -> HourWindow {
    let days: Vec<u64> = w["days"]
        .as_array()
        .map(|d| d.iter().filter_map(|v| v.as_u64()).collect())
        .unwrap_or_default();
    let on: Vec<bool> = (1..=7)
        .map(|d| days.is_empty() || days.contains(&d))
        .collect();
    HourWindow {
        days: ModelRc::new(VecModel::from(on)),
        start: w["start"].as_str().unwrap_or_default().into(),
        end: w["end"].as_str().unwrap_or_default().into(),
    }
}

/// `active_hours` after toggling ISO day `day` (1-7) of window `i`. A window with
/// every day selected is written with `days: []` (= every day).
pub fn toggle_day(hours: &mut [Value], i: usize, day: u64) {
    let Some(w) = hours.get_mut(i) else { return };
    let mut days: Vec<u64> = w["days"]
        .as_array()
        .map(|d| d.iter().filter_map(|v| v.as_u64()).collect())
        .unwrap_or_default();
    if days.is_empty() {
        days = (1..=7).collect();
    }
    if let Some(pos) = days.iter().position(|d| *d == day) {
        days.remove(pos);
    } else {
        days.push(day);
        days.sort_unstable();
    }
    if days.len() == 7 {
        days.clear();
    }
    w["days"] = serde_json::json!(days);
}

/// Forces the next render to rebuild the forms (after a rejected edit, to show the
/// stored value again).
pub fn invalidate() {
    BUILT.with(|b| *b.borrow_mut() = None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn day_toggles_keep_every_day_as_empty() {
        let mut hours = vec![json!({"days": [], "start": "18:00", "end": "02:00"})];
        toggle_day(&mut hours, 0, 7);
        assert_eq!(hours[0]["days"], json!([1, 2, 3, 4, 5, 6]));
        toggle_day(&mut hours, 0, 7);
        assert_eq!(hours[0]["days"], json!([]));
    }
}
