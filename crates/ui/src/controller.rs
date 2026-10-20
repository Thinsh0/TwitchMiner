//! Glue between the window and the backend: opens the session, mirrors the state into
//! the views, and turns callbacks into commands (run on the tokio runtime, never on
//! the UI thread).

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use tm_client::Backend;
use tm_core::{ApiResult, Command, Event};
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

use crate::activity;
use crate::forms;
use crate::prefs::{self, Prefs, RemotePrefs};
use crate::present::{self, Input, Last, Today};
use crate::session::{self, LocalInfo, OpenError, Session, Target};
use crate::settings;
use crate::{
    Bridge, Conn, LogItem, MainWindow, SetupReason, ToastItem, ToastKind, Tone, UpdateInfo,
};

thread_local! {
    /// Last state seen, on the UI thread (re-rendered by the timer).
    static LAST: RefCell<Last> = RefCell::new(Last::default());
}

/// Where the local engine serves its API when the option is on (same as the daemon).
const DEFAULT_API_BIND: &str = "127.0.0.1:7878";

/// How long a toast stays on screen.
const TOAST_TTL: Duration = Duration::from_secs(6);

#[derive(Clone)]
pub struct Controller(Arc<Inner>);

struct Inner {
    rt: Handle,
    ui: slint::Weak<MainWindow>,
    data_dir: Option<PathBuf>,
    backend: Mutex<Option<Arc<dyn Backend>>>,
    session: tokio::sync::Mutex<Option<Session>>,
    watcher: Mutex<Option<JoinHandle<()>>>,
    prefs: Mutex<Prefs>,
    /// Logs and tokens of the local engine (none with a remote one).
    local: Mutex<Option<LocalInfo>>,
}

impl Controller {
    pub fn new(rt: Handle, ui: &MainWindow, data_dir: Option<PathBuf>, prefs: Prefs) -> Self {
        Self(Arc::new(Inner {
            rt,
            ui: ui.as_weak(),
            data_dir,
            backend: Mutex::new(None),
            session: tokio::sync::Mutex::new(None),
            watcher: Mutex::new(None),
            prefs: Mutex::new(prefs),
            local: Mutex::new(None),
        }))
    }

    pub fn bind(&self, ui: &MainWindow) {
        let b = ui.global::<Bridge>();
        b.set_toasts(ModelRc::from(Rc::new(VecModel::<ToastItem>::default())));

        let c = self.clone();
        b.on_login_start(move || c.login_start());
        let c = self.clone();
        b.on_login_cancel(move |flow_id| {
            c.command(Command::LoginCancel {
                flow_id: flow_id.into(),
            })
        });
        b.on_open_url(|url| open_url(&url));
        let c = self.clone();
        b.on_connect_remote(move |url, token| {
            c.open(
                Target::Remote {
                    url: url.trim().trim_end_matches('/').to_string(),
                    token: token.trim().to_string(),
                },
                true,
            )
        });
        let c = self.clone();
        b.on_use_local(move || c.open(Target::Local, true));
        let c = self.clone();
        b.on_show_setup(move || c.show_setup());
        let weak = ui.as_weak();
        b.on_cancel_setup(move || {
            if let Some(ui) = weak.upgrade() {
                ui.global::<Bridge>().set_conn(Conn::Ready);
            }
        });
        let c = self.clone();
        b.on_dismiss_notice(move |id| c.command(Command::NoticeDismiss { id: id.into() }));
        let weak = ui.as_weak();
        b.on_dismiss_toast(move |id| {
            if let Some(ui) = weak.upgrade() {
                remove_toast(&ui, id);
            }
        });
        let c = self.clone();
        b.on_set_account_enabled(move |account, enabled| {
            c.command(Command::AccountSetEnabled {
                account: account.into(),
                enabled,
            })
        });
        let c = self.clone();
        b.on_remove_account(move |account| {
            c.command(Command::AccountRemove {
                account: account.into(),
            })
        });
        let weak = ui.as_weak();
        let c = self.clone();
        b.on_select_account(move |id| {
            if let Some(ui) = weak.upgrade() {
                ui.global::<Bridge>().set_account(id);
                render(&ui);
                c.refresh_today();
                c.load_activity(false);
            }
        });
        let c = self.clone();
        b.on_set_paused(move |paused| c.set_paused(paused));
        let weak = ui.as_weak();
        let c = self.clone();
        b.on_set_autostart(move |on| {
            match crate::platform::with(|p| p.set_autostart(on).map(|()| p.autostart())) {
                Some(Ok(Some(now))) => {
                    if let Some(ui) = weak.upgrade() {
                        ui.global::<Bridge>().set_autostart(now);
                    }
                }
                Some(Err(e)) => c.toast(ToastKind::Error, Tone::Danger, e),
                _ => {}
            }
        });
        let c = self.clone();
        let weak = ui.as_weak();
        b.on_focus_campaign(move |campaign_id| {
            let Some(ui) = weak.upgrade() else { return };
            c.command(Command::ModuleCall {
                account: ui.global::<Bridge>().get_account().into(),
                module: "drops".into(),
                name: "focus".into(),
                args: serde_json::json!({ "campaign_id": campaign_id.as_str() }),
            })
        });
        let c = self.clone();
        b.on_unfocus(move || c.module_call("drops", "unfocus", Value::Null, |_| {}));
        let c = self.clone();
        b.on_drops_refresh(move || c.module_call("drops", "refresh", Value::Null, |_| {}));
        let c = self.clone();
        b.on_drops_claim_all(move || {
            let me = c.clone();
            c.module_call("drops", "claim_all", Value::Null, move |r| {
                if let Ok(v) = r {
                    let n = v["claimed"].as_u64().unwrap_or_default();
                    me.toast(ToastKind::Claimed, Tone::Success, n.to_string());
                }
            })
        });
        let c = self.clone();
        b.on_list_add(move |key, value| {
            let value = value.trim().to_string();
            if value.is_empty() {
                return;
            }
            c.edit_list(&key, move |list| {
                if !list.iter().any(|v| v.eq_ignore_ascii_case(&value)) {
                    list.push(value);
                }
            })
        });
        let c = self.clone();
        b.on_list_remove(move |key, value| {
            c.edit_list(&key, move |list| list.retain(|v| v != value.as_str()))
        });
        let c = self.clone();
        b.on_list_move(move |key, from, to| {
            c.edit_list(&key, move |list| {
                let (from, to) = (from as usize, to as usize);
                if from < list.len() && to < list.len() {
                    let item = list.remove(from);
                    list.insert(to, item);
                }
            })
        });
        let c = self.clone();
        b.on_set_module_setting(move |module, key, json| {
            match serde_json::from_str::<Value>(&json) {
                Ok(value) => c.set_module_setting(&module, &key, value),
                Err(e) => tracing::warn!(error = %e, %key, "invalid setting value from the view"),
            }
        });
        let c = self.clone();
        b.on_setting_toggle(move |section, path, v| c.write_setting(&section, &path, json!(v)));
        let c = self.clone();
        b.on_setting_text(move |section, path, text| {
            let Some(field) = settings::field(&section, &path) else {
                return;
            };
            match forms::parse(&field, &text) {
                Ok(v) => c.write_setting(&section, &path, v),
                Err(()) => {
                    c.toast(ToastKind::Invalid, Tone::Warning, String::new());
                    c.on_ui(|ui| {
                        settings::invalidate();
                        render(ui);
                    });
                }
            }
        });
        let c = self.clone();
        b.on_setting_pick(move |section, path, i| {
            let value = settings::field(&section, &path)
                .and_then(|f| f.options.get(i.max(0) as usize).cloned());
            if let Some(v) = value {
                c.write_setting(&section, &path, json!(v));
            }
        });
        let c = self.clone();
        b.on_setting_list_add(move |section, path, value| {
            let value = value.trim().to_string();
            if value.is_empty() {
                return;
            }
            c.edit_setting(&section, &path, move |v| {
                let mut list = present::string_list(Some(v));
                if !list.contains(&value) {
                    list.push(value);
                }
                *v = json!(list);
            })
        });
        let c = self.clone();
        b.on_setting_list_remove(move |section, path, value| {
            c.edit_setting(&section, &path, move |v| {
                let mut list = present::string_list(Some(v));
                list.retain(|x| x != value.as_str());
                *v = json!(list);
            })
        });
        let c = self.clone();
        b.on_setting_reset(move |section, path| {
            let default = forms::get(&settings::defaults(&section), &path).clone();
            c.write_setting(&section, &path, default);
        });
        let c = self.clone();
        b.on_hours_add(move || {
            c.edit_setting("engine", "active_hours", |v| {
                let mut list = v.as_array().cloned().unwrap_or_default();
                list.push(json!({"days": [], "start": "18:00", "end": "02:00"}));
                *v = json!(list);
            })
        });
        let c = self.clone();
        b.on_hours_remove(move |i| {
            c.edit_setting("engine", "active_hours", move |v| {
                if let Some(list) = v.as_array_mut()
                    && (i as usize) < list.len()
                {
                    list.remove(i as usize);
                }
            })
        });
        let c = self.clone();
        b.on_hours_toggle_day(move |i, day| {
            c.edit_setting("engine", "active_hours", move |v| {
                if let Some(list) = v.as_array_mut() {
                    settings::toggle_day(list, i as usize, day as u64 + 1);
                }
            })
        });
        let c = self.clone();
        b.on_hours_set(move |i, start, end| {
            let valid = |t: &str| {
                let Some((h, m)) = t.trim().split_once(':') else {
                    return false;
                };
                matches!((h.parse::<u32>(), m.parse::<u32>()), (Ok(h), Ok(m)) if h < 24 && m < 60)
            };
            if !valid(&start) || !valid(&end) {
                c.toast(ToastKind::Invalid, Tone::Warning, String::new());
                c.on_ui(|ui| {
                    settings::invalidate();
                    render(ui);
                });
                return;
            }
            c.edit_setting("engine", "active_hours", move |v| {
                if let Some(w) = v.as_array_mut().and_then(|l| l.get_mut(i as usize)) {
                    w["start"] = json!(start.trim());
                    w["end"] = json!(end.trim());
                }
            })
        });
        let c = self.clone();
        b.on_logs_refresh(move || c.refresh_logs());
        let c = self.clone();
        b.on_set_api(move |enabled, bind| {
            let bind = bind.trim().to_string();
            let api = if enabled {
                let bind = if bind.is_empty() {
                    DEFAULT_API_BIND.to_string()
                } else {
                    bind
                };
                if bind.parse::<std::net::SocketAddr>().is_err() {
                    c.toast(ToastKind::Invalid, Tone::Warning, String::new());
                    return;
                }
                Some(bind)
            } else {
                None
            };
            c.update_prefs(|p| p.api = api);
            c.restart_local();
        });
        let c = self.clone();
        b.on_update_check(move || {
            let me = c.clone();
            c.run(Command::UpdateCheck, move |r| {
                if let Ok(v) = r {
                    let info = UpdateInfo {
                        checked: true,
                        available: v["update_available"].as_bool().unwrap_or_default(),
                        latest: v["latest"].as_str().unwrap_or_default().into(),
                        url: v["url"].as_str().unwrap_or_default().into(),
                    };
                    me.on_ui(move |ui| ui.global::<Bridge>().set_update_info(info));
                }
            })
        });
        let c = self.clone();
        b.on_manifest_refresh(move || {
            let me = c.clone();
            c.run(Command::ManifestRefresh, move |r| {
                if let Ok(v) = r {
                    let detail = if v["updated"].as_bool() == Some(true) {
                        v["revision"].to_string()
                    } else {
                        String::new()
                    };
                    me.toast(ToastKind::Manifest, Tone::Success, detail);
                }
            })
        });
        let c = self.clone();
        b.on_backup(move || {
            let me = c.clone();
            c.run(Command::Backup { path: None }, move |r| {
                if let Ok(v) = r {
                    let path = v["path"].as_str().unwrap_or_default().to_string();
                    me.toast(ToastKind::Backup, Tone::Success, path);
                }
            })
        });
        let c = self.clone();
        b.on_copy_text(move |text| {
            let copied = arboard::Clipboard::new().and_then(|mut cb| cb.set_text(text.as_str()));
            match copied {
                Ok(()) => c.toast(ToastKind::Copied, Tone::Success, String::new()),
                Err(e) => c.toast(ToastKind::Error, Tone::Danger, e.to_string()),
            }
        });
        let c = self.clone();
        b.on_activity_changed(move || c.load_activity(false));
        let c = self.clone();
        b.on_screen_shown(move || c.load_activity(false));
        let c = self.clone();
        b.on_history_load_more(move || c.load_activity(true));
        let c = self.clone();
        b.on_add_channel(move |login| {
            let login = login.trim().trim_start_matches('@').to_lowercase();
            if !login.is_empty() {
                c.module_call(
                    "points",
                    "add_channel",
                    serde_json::json!({ "login": login }),
                    |_| {},
                );
            }
        });
        let c = self.clone();
        b.on_remove_channel(move |login| {
            c.module_call(
                "points",
                "remove_channel",
                serde_json::json!({ "login": login.as_str() }),
                |_| {},
            )
        });
        let weak = ui.as_weak();
        crate::images::init(self.0.rt.clone(), move || {
            if let Some(ui) = weak.upgrade() {
                render(&ui);
            }
        });
        let c = self.clone();
        b.on_theme_changed(move |i| {
            let name = match i {
                1 => "light",
                2 => "oled",
                _ => "dark",
            };
            c.update_prefs(|p| p.theme = Some(name.into()));
        });
        let c = self.clone();
        let weak = ui.as_weak();
        b.on_language_changed(move |i| {
            let lang = if i == 1 { "en" } else { "fr" };
            if let Some(ui) = weak.upgrade() {
                crate::set_language(&ui, Some(lang));
            }
            c.update_prefs(|p| p.language = Some(lang.into()));
        });
    }

    /// Opens a session on `target`. `from_setup`: asked from the "choose an engine"
    /// screen, which stays visible (with the error) if it fails.
    pub fn open(&self, target: Target, from_setup: bool) {
        let me = self.clone();
        me.on_ui(move |ui| {
            let b = ui.global::<Bridge>();
            if from_setup {
                b.set_connecting(true);
            } else {
                b.set_conn(Conn::Starting);
            }
        });
        let inner = self.0.clone();
        self.0.rt.spawn(async move {
            // Already on the requested local engine: nothing to restart.
            let same_local = target == Target::Local
                && inner
                    .session
                    .lock()
                    .await
                    .as_ref()
                    .is_some_and(|s| s.target == Target::Local);
            if same_local {
                me.on_ui(|ui| {
                    let b = ui.global::<Bridge>();
                    b.set_connecting(false);
                    b.set_conn(Conn::Ready);
                });
                return;
            }
            // A local engine must release the data directory before another one opens it.
            if target == Target::Local {
                me.close().await;
            }
            let api = inner
                .prefs
                .lock()
                .expect("prefs lock")
                .api
                .as_deref()
                .and_then(|a| a.parse().ok());
            match Session::open(target.clone(), inner.data_dir.clone(), api).await {
                Ok(new) => {
                    let backend = new.backend.clone();
                    let local = new.local();
                    let (agent, full) = local
                        .as_ref()
                        .map(|l| (l.agent_token.clone(), l.api_token.clone()))
                        .unwrap_or_default();
                    *inner.local.lock().expect("local lock") = local;
                    let is_local = target == Target::Local;
                    let api_bind = api.map(|a| a.to_string()).unwrap_or_default();
                    me.on_ui(move |ui| {
                        let b = ui.global::<Bridge>();
                        b.set_agent_token(agent.into());
                        b.set_api_token(full.into());
                        b.set_api_bind(api_bind.into());
                        b.set_local_engine(is_local);
                    });
                    let remote = match &target {
                        Target::Remote { url, token } => Some(Some(RemotePrefs {
                            url: url.clone(),
                            token: token.clone(),
                        })),
                        Target::Local => Some(None),
                        Target::Demo => None,
                    };
                    if let Some(remote) = remote {
                        me.update_prefs(|p| p.remote = remote);
                    }
                    let old = inner.session.lock().await.replace(new);
                    me.watch(backend, &target);
                    if let Some(old) = old {
                        old.close().await;
                    }
                    me.on_ui(|ui| {
                        let b = ui.global::<Bridge>();
                        b.set_connecting(false);
                        b.set_setup_reason(SetupReason::None);
                        b.set_conn(Conn::Ready);
                    });
                }
                Err(e) => {
                    let has_session = inner.session.lock().await.is_some();
                    let (reason, detail, url, token) = match e {
                        OpenError::Busy { pid } => (
                            SetupReason::Busy,
                            pid,
                            session::LOCAL_DAEMON_URL.to_string(),
                            session::local_daemon_token(inner.data_dir.clone())
                                .await
                                .unwrap_or_default(),
                        ),
                        OpenError::Failed(msg) => {
                            let (url, token) = match &target {
                                Target::Remote { url, token } => (url.clone(), token.clone()),
                                Target::Local | Target::Demo => Default::default(),
                            };
                            (SetupReason::Failed, msg, url, token)
                        }
                    };
                    me.on_ui(move |ui| {
                        let b = ui.global::<Bridge>();
                        b.set_connecting(false);
                        b.set_setup_reason(reason);
                        b.set_setup_detail(detail.into());
                        b.set_setup_url(url.into());
                        b.set_setup_token(token.into());
                        b.set_setup_can_cancel(has_session);
                        b.set_conn(Conn::Setup);
                    });
                }
            }
        });
    }

    fn show_setup(&self) {
        let remote_url = LAST.with(|l| l.borrow().remote.as_ref().map(|(u, _)| u.clone()));
        self.on_ui(move |ui| {
            let b = ui.global::<Bridge>();
            b.set_setup_reason(SetupReason::None);
            b.set_setup_url(remote_url.unwrap_or_default().into());
            b.set_setup_token(SharedString::new());
            b.set_setup_can_cancel(true);
            b.set_conn(Conn::Setup);
        });
    }

    /// Mirrors the backend's state, link and notable events into the window.
    fn watch(&self, backend: Arc<dyn Backend>, target: &Target) {
        let remote_url = match target {
            Target::Remote { url, .. } => Some(url.clone()),
            Target::Local | Target::Demo => None,
        };
        *self.0.backend.lock().expect("backend lock") = Some(backend.clone());
        let me = self.clone();
        let task = self.0.rt.spawn(async move {
            let mut state = backend.state();
            let mut link = backend.link();
            let mut events = backend.events();
            // A local engine's link never changes (its sender is gone): stop polling it.
            let mut link_open = true;
            let mut first = true;
            loop {
                let s = state.borrow_and_update().clone();
                let l = link.borrow_and_update().clone();
                let remote = remote_url.clone().map(|u| (u, l));
                me.on_ui(move |ui| {
                    LAST.with(|last| {
                        let mut last = last.borrow_mut();
                        last.state = s;
                        last.remote = remote;
                    });
                    render(ui);
                });
                if first {
                    first = false;
                    me.refresh_today();
                    me.load_activity(false);
                    me.load_schemas(backend.clone());
                    me.refresh_logs();
                }
                // Wait for the next change, handling events (toasts) in between.
                loop {
                    tokio::select! {
                        r = state.changed() => { if r.is_err() { return; } break; }
                        r = link.changed(), if link_open => {
                            if r.is_err() { link_open = false; continue; }
                            break;
                        }
                        r = events.recv() => match r {
                            Ok(env) => me.on_event(&env.event),
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                            Err(_) => return,
                        },
                    }
                }
            }
        });
        if let Some(old) = self.0.watcher.lock().expect("watcher lock").replace(task) {
            old.abort();
        }
    }

    fn on_event(&self, event: &Event) {
        if let Event::DropClaimed { drop_name, .. } = event {
            self.toast(ToastKind::DropClaimed, Tone::Success, drop_name.clone());
        }
        if matches!(
            event,
            Event::PointsEarned { .. } | Event::DropClaimed { .. } | Event::WatchMinute { .. }
        ) {
            self.refresh_today();
        }
        if let Event::LoginFinished { error, account, .. } = event {
            match error {
                Some(e) => self.toast(ToastKind::Error, Tone::Danger, e.clone()),
                None if account.is_some() => {
                    let id = account.clone().unwrap_or_default();
                    self.on_ui(move |ui| {
                        let name = LAST.with(|l| {
                            l.borrow()
                                .state
                                .accounts
                                .get(&id)
                                .and_then(|a| a.info.as_ref())
                                .map(|i| i.display_name.clone())
                        });
                        let b = ui.global::<Bridge>();
                        b.set_account(id.into());
                        b.set_screen(crate::Screen::Home);
                        if let Some(name) = name {
                            push_toast(ui, ToastKind::LoginOk, Tone::Success, name);
                        }
                    });
                }
                None => {}
            }
        }
    }

    fn login_start(&self) {
        self.on_ui(|ui| ui.global::<Bridge>().set_login_busy(true));
        let me = self.clone();
        self.run(Command::LoginStart { profile: None }, move |r| {
            me.on_ui(|ui| ui.global::<Bridge>().set_login_busy(false));
            if let Ok(v) = r
                && let Some(url) = v["verification_uri_complete"]
                    .as_str()
                    .or_else(|| v["verification_uri"].as_str())
            {
                open_url(url);
            }
        });
    }

    /// Global pause of the engine.
    pub fn set_paused(&self, paused: bool) {
        self.command(Command::SettingsSet {
            scope: "engine".into(),
            value: json!({ "paused": paused }),
            merge: true,
        });
    }

    /// Restarts the local engine (to apply the API option); nothing for a remote one.
    fn restart_local(&self) {
        let me = self.clone();
        let inner = self.0.clone();
        self.0.rt.spawn(async move {
            let local = inner
                .session
                .lock()
                .await
                .as_ref()
                .is_some_and(|s| s.target == Target::Local);
            if local {
                me.close().await;
                me.open(Target::Local, false);
            }
        });
    }

    /// Writes one setting (`section` = module id or `engine`, dotted `path`).
    fn write_setting(&self, section: &str, path: &str, value: Value) {
        let me = self.clone();
        let (section, path) = (section.to_string(), path.to_string());
        self.on_ui(move |ui| {
            let account = ui.global::<Bridge>().get_account().to_string();
            let scope =
                LAST.with(|l| settings::scope_for(&l.borrow().state, &account, &section, &path));
            me.command(Command::SettingsSet {
                scope,
                value: forms::patch(&path, value),
                merge: true,
            });
        });
    }

    /// Edits one setting from its effective value.
    fn edit_setting(&self, section: &str, path: &str, f: impl FnOnce(&mut Value) + Send + 'static) {
        let me = self.clone();
        let (section, path) = (section.to_string(), path.to_string());
        self.on_ui(move |ui| {
            let account = ui.global::<Bridge>().get_account().to_string();
            let mut value = LAST.with(|l| {
                let eff = settings::effective(&l.borrow().state, &account, &section);
                forms::get(&eff, &path).clone()
            });
            f(&mut value);
            me.write_setting(&section, &path, value);
        });
    }

    /// Loads the settings schemas (once per session).
    fn load_schemas(&self, backend: Arc<dyn Backend>) {
        let ui = self.0.ui.clone();
        self.0.rt.spawn(async move {
            let modules = backend.execute(Command::ModulesList).await;
            let engine = backend
                .execute(Command::SettingsGet {
                    scope: "engine".into(),
                    account: None,
                })
                .await;
            if let (Ok(modules), Ok(engine)) = (modules, engine) {
                let _ = ui.upgrade_in_event_loop(move |ui| {
                    settings::set_schemas(&modules, &engine);
                    render(&ui);
                });
            }
        });
    }

    /// Recent lines of the local engine's log (not available for a remote engine).
    fn refresh_logs(&self) {
        let logs = self
            .0
            .local
            .lock()
            .expect("local lock")
            .as_ref()
            .map(|l| l.logs.clone());
        self.on_ui(move |ui| {
            let b = ui.global::<Bridge>();
            b.set_logs_available(logs.is_some());
            let lines: Vec<LogItem> = logs
                .map(|l| l.recent(300))
                .unwrap_or_default()
                .into_iter()
                .rev()
                .map(|l| LogItem {
                    time: l
                        .ts
                        .with_timezone(&chrono::Local)
                        .format("%d/%m %H:%M:%S")
                        .to_string()
                        .into(),
                    level: l.level.to_lowercase().into(),
                    target: l.target.into(),
                    message: l.message.into(),
                })
                .collect();
            b.set_logs(ModelRc::new(VecModel::from(lines)));
        });
    }

    /// Queries the Activity screen's chart and history (only while it is shown).
    /// `more`: next page of the history, appended.
    pub fn load_activity(&self, more: bool) {
        let me = self.clone();
        self.on_ui(move |ui| {
            let b = ui.global::<Bridge>();
            if b.get_screen() != crate::Screen::Activity {
                return;
            }
            let choice = activity::Choice {
                account: b.get_account().to_string(),
                metric: b.get_chart_metric(),
                group: b.get_chart_group(),
                days: b.get_chart_days(),
                filter: b.get_history_filter(),
            };
            if choice.account.is_empty() {
                return;
            }
            let Some(backend) = me.0.backend.lock().expect("backend lock").clone() else {
                return;
            };
            let before = more
                .then(|| ACTIVITY.with(|a| a.borrow().history.last().map(|r| r["id"].as_i64())))
                .flatten()
                .flatten();
            if more && before.is_none() {
                return;
            }
            b.set_history_loading(true);
            let now = chrono::Utc::now();
            let stats_cmd = serde_json::from_value::<Command>(activity::stats_query(&choice, now));
            let history_cmd =
                serde_json::from_value::<Command>(activity::history_query(&choice, before));
            let ui = me.0.ui.clone();
            me.0.rt.spawn(async move {
                let stats = match (more, stats_cmd) {
                    (false, Ok(cmd)) => backend.execute(cmd).await.ok(),
                    _ => None,
                };
                let history = match history_cmd {
                    Ok(cmd) => backend.execute(cmd).await.ok(),
                    Err(_) => None,
                };
                let _ = ui.upgrade_in_event_loop(move |ui| {
                    ACTIVITY.with(|a| {
                        let mut a = a.borrow_mut();
                        if !more {
                            a.history.clear();
                        }
                        if let Some(s) = stats {
                            a.stats = s;
                        }
                        let rows = history
                            .and_then(|h| h.as_array().cloned())
                            .unwrap_or_default();
                        a.more = rows.len() >= activity::PAGE;
                        a.history.extend(rows);
                        a.choice = Some(choice);
                    });
                    render_activity(&ui);
                    ui.global::<Bridge>().set_history_loading(false);
                });
            });
        });
    }

    /// `module_call` for the selected account.
    fn module_call(
        &self,
        module: &str,
        name: &str,
        args: Value,
        done: impl FnOnce(&ApiResult) + Send + 'static,
    ) {
        let me = self.clone();
        let (module, name) = (module.to_string(), name.to_string());
        self.on_ui(move |ui| {
            let account = ui.global::<Bridge>().get_account().to_string();
            if account.is_empty() {
                return;
            }
            me.run(
                Command::ModuleCall {
                    account,
                    module,
                    name,
                    args,
                },
                done,
            );
        });
    }

    /// Edits a list setting (`<module>.<key>`) from its current value in the state.
    fn edit_list(&self, path: &str, f: impl FnOnce(&mut Vec<String>) + Send + 'static) {
        let Some((module, key)) = path.split_once('.') else {
            return;
        };
        if !matches!(
            (module, key),
            ("drops", "games" | "exclude_games" | "wanted_rewards")
                | ("points", "channels" | "exclude")
        ) {
            return;
        }
        let me = self.clone();
        let (module, key) = (module.to_string(), key.to_string());
        self.on_ui(move |ui| {
            let account = ui.global::<Bridge>().get_account().to_string();
            let mut list = LAST.with(|l| {
                present::string_list(present::module_setting(
                    &l.borrow().state,
                    &account,
                    &module,
                    &key,
                ))
            });
            f(&mut list);
            me.set_module_setting(&module, &key, serde_json::json!(list));
        });
    }

    /// Saves one top-level module setting where the module reads it for the selected
    /// account (its override if it has one for this key, else the shared scope).
    fn set_module_setting(&self, module: &str, key: &str, value: Value) {
        let me = self.clone();
        let (module, key) = (module.to_string(), key.to_string());
        self.on_ui(move |ui| {
            let account = ui.global::<Bridge>().get_account().to_string();
            let scope = LAST.with(|l| {
                present::module_setting_scope(&l.borrow().state, &account, &module, &key)
            });
            me.command(Command::SettingsSet {
                scope,
                value: serde_json::json!({ key: value }),
                merge: true,
            });
        });
    }

    /// Periodic refresh of query-based views; keeps extra history pages.
    pub fn tick(&self) {
        self.refresh_today();
        let paged = ACTIVITY.with(|a| a.borrow().history.len() > activity::PAGE);
        if !paged {
            self.load_activity(false);
        }
    }

    /// Reloads today's totals of the selected account (`stats` query, not engine state).
    pub fn refresh_today(&self) {
        let me = self.clone();
        self.on_ui(move |ui| {
            let account = ui.global::<Bridge>().get_account().to_string();
            if account.is_empty() {
                return;
            }
            let Some(backend) = me.0.backend.lock().expect("backend lock").clone() else {
                return;
            };
            let day = present::today_key(chrono::Utc::now());
            let Ok(cmd) = serde_json::from_value::<Command>(serde_json::json!({
                "cmd": "stats", "account": account, "from": day, "to": day,
            })) else {
                return;
            };
            let ui = me.0.ui.clone();
            me.0.rt.spawn(async move {
                let Ok(rows) = backend.execute(cmd).await else {
                    return;
                };
                let sum = |k: &str| {
                    rows.as_array()
                        .map(|r| r.iter().filter_map(|x| x[k].as_i64()).sum())
                        .unwrap_or_default()
                };
                let today = Today {
                    account,
                    day,
                    points: sum("points"),
                    minutes: sum("watch_minutes"),
                    drops: sum("drops"),
                };
                let _ = ui.upgrade_in_event_loop(move |ui| {
                    LAST.with(|l| l.borrow_mut().today = Some(today));
                    render(&ui);
                });
            });
        });
    }

    /// Changes the interface preferences and saves them (in the background) if they
    /// changed.
    fn update_prefs(&self, f: impl FnOnce(&mut Prefs)) {
        let saved = {
            let mut p = self.0.prefs.lock().expect("prefs lock");
            let before = p.clone();
            f(&mut p);
            (*p != before).then(|| p.clone())
        };
        if let Some(p) = saved {
            self.0.rt.spawn(prefs::save(self.0.data_dir.clone(), p));
        }
    }

    /// Fire-and-forget command; failures become a toast.
    fn command(&self, cmd: Command) {
        self.run(cmd, |_| {});
    }

    fn run(&self, cmd: Command, done: impl FnOnce(&ApiResult) + Send + 'static) {
        let Some(backend) = self.0.backend.lock().expect("backend lock").clone() else {
            return;
        };
        let me = self.clone();
        self.0.rt.spawn(async move {
            let r = backend.execute(cmd).await;
            if let Err(e) = &r {
                me.toast(ToastKind::Error, Tone::Danger, e.message.clone());
            }
            done(&r);
        });
    }

    fn toast(&self, kind: ToastKind, tone: Tone, detail: String) {
        self.on_ui(move |ui| push_toast(ui, kind, tone, detail));
    }

    fn on_ui(&self, f: impl FnOnce(&MainWindow) + Send + 'static) {
        let _ = self.0.ui.upgrade_in_event_loop(move |ui| f(&ui));
    }

    /// Stops watching and closes the session (local engine shut down cleanly).
    pub async fn close(&self) {
        if let Some(w) = self.0.watcher.lock().expect("watcher lock").take() {
            w.abort();
        }
        *self.0.backend.lock().expect("backend lock") = None;
        if let Some(s) = self.0.session.lock().await.take() {
            s.close().await;
        }
    }
}

/// The summary shown outside the window (tray), all accounts together.
fn status(
    ui: &MainWindow,
    state: &tm_core::State,
    now: chrono::DateTime<chrono::Utc>,
) -> crate::Status {
    let s = tm_core::view::summary(state, now);
    let drop = s.accounts.iter().find_map(|a| a.drop.as_ref()).map(|d| {
        (
            d.drop.clone(),
            d.game.clone(),
            (d.progress * 100.0).round() as u8,
        )
    });
    crate::Status {
        language: if ui.global::<Bridge>().get_language() == 1 {
            "en"
        } else {
            "fr"
        },
        paused: s.paused,
        watching_allowed: s.watching_allowed,
        attention: s.attention,
        watching: s
            .accounts
            .iter()
            .flat_map(|a| a.watching.iter().map(|w| w.channel.clone()))
            .collect(),
        drop,
    }
}

/// Last Activity query results, on the UI thread.
#[derive(Default)]
struct ActivityCache {
    choice: Option<activity::Choice>,
    stats: Value,
    history: Vec<Value>,
    more: bool,
}

thread_local! {
    static ACTIVITY: RefCell<ActivityCache> = RefCell::new(ActivityCache::default());
}

fn render_activity(ui: &MainWindow) {
    let b = ui.global::<Bridge>();
    let lang = b.get_language();
    let now = chrono::Utc::now();
    ACTIVITY.with(|a| {
        let a = a.borrow();
        let Some(choice) = &a.choice else { return };
        LAST.with(|l| {
            let state = &l.borrow().state;
            let (bars, total) = activity::chart(&a.stats, choice, state, now, lang);
            b.set_chart(ModelRc::new(VecModel::from(bars)));
            b.set_chart_total(total.clamp(i32::MIN as i64, i32::MAX as i64) as i32);
            let items: Vec<_> = a
                .history
                .iter()
                .map(|r| activity::history_item(r, state, now, lang))
                .collect();
            present::sync(b.get_history(), items, |m| b.set_history(m));
        });
        b.set_history_more(a.more);
    });
}

/// Re-renders every view from the last state (UI thread).
pub fn render(ui: &MainWindow) {
    LAST.with(|last| {
        let last = last.borrow();
        let input = Input {
            state: &last.state,
            remote: last.remote.as_ref().map(|(u, l)| (u.as_str(), l)),
            today: last.today.as_ref(),
            now: chrono::Utc::now(),
        };
        present::apply(ui, &input);
        settings::render(ui, &last.state);
        crate::platform::publish(status(ui, &last.state, input.now));
    });
}

fn push_toast(ui: &MainWindow, kind: ToastKind, tone: Tone, detail: String) {
    thread_local!(static NEXT: std::cell::Cell<i32> = const { std::cell::Cell::new(1) });
    let id = NEXT.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    let toasts = ui.global::<Bridge>().get_toasts();
    let Some(model) = toasts.as_any().downcast_ref::<VecModel<ToastItem>>() else {
        return;
    };
    // Same message already shown: do not stack it.
    if model.iter().any(|t| t.kind == kind && t.detail == detail) {
        return;
    }
    model.push(ToastItem {
        id,
        kind,
        tone,
        detail: detail.into(),
    });
    let weak = ui.as_weak();
    slint::Timer::single_shot(TOAST_TTL, move || {
        if let Some(ui) = weak.upgrade() {
            remove_toast(&ui, id);
        }
    });
}

fn remove_toast(ui: &MainWindow, id: i32) {
    let toasts = ui.global::<Bridge>().get_toasts();
    if let Some(model) = toasts.as_any().downcast_ref::<VecModel<ToastItem>>()
        && let Some(i) = model.iter().position(|t| t.id == id)
    {
        model.remove(i);
    }
}

fn open_url(url: &str) {
    // Only web links: never let a value from the state launch a local program.
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return;
    }
    if let Err(e) = open::that_detached(url) {
        tracing::warn!(error = %e, url, "cannot open the browser");
    }
}
