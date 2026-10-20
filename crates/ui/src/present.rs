//! State → view: turns the engine state mirror into the `Bridge` properties.
//! Pure projection, recomputed on every state change; nothing here is kept as truth.

use std::rc::Rc;
use std::sync::Arc;

use chrono::{DateTime, Local, Utc};
use serde::Deserialize;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use tm_client::Link;
use tm_core::State;
use tm_core::state::{Account, AccountStatus, Level};

use crate::images;
use crate::{
    AccountItem, Bridge, DropItem, Idle, LinkState, LoginItem, MainWindow, NoticeItem, PlanTodo,
    TodayItem, WatchItem,
};

/// What the projection needs besides the state.
pub struct Input<'a> {
    pub state: &'a State,
    /// `None` for a local engine.
    pub remote: Option<(&'a str, &'a Link)>,
    /// Today's totals of the selected account (`stats` query), when loaded.
    pub today: Option<&'a Today>,
    pub now: DateTime<Utc>,
}

/// Result of the `stats` query for one account and one day.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Today {
    pub account: String,
    pub day: String,
    pub points: i64,
    pub minutes: i64,
    pub drops: i64,
}

pub fn apply(ui: &MainWindow, input: &Input) {
    let bridge = ui.global::<Bridge>();
    let state = input.state;

    let accounts = accounts(state);
    let mut selected = bridge.get_account();
    if !accounts.iter().any(|a| a.id == selected) {
        selected = accounts.first().map(|a| a.id.clone()).unwrap_or_default();
        bridge.set_account(selected.clone());
    }
    let account = state.accounts.get(selected.as_str());
    let logins = logins(state);
    let notices = notices(state);
    let plan_todos = account
        .map(|a| plan_todos(a, input.now))
        .unwrap_or_default();
    let auth = if logins.is_empty() {
        accounts
            .iter()
            .filter(|a| a.status == "auth_required")
            .count()
    } else {
        0
    };
    bridge.set_todo_count((logins.len() + notices.len() + auth + plan_todos.len()) as i32);
    sync(bridge.get_plan_todos(), plan_todos, |m| {
        bridge.set_plan_todos(m)
    });
    home(&bridge, state, selected.as_str(), account, input);
    sync(bridge.get_accounts(), accounts, |m| bridge.set_accounts(m));
    sync(bridge.get_logins(), logins, |m| bridge.set_logins(m));
    sync(bridge.get_notices(), notices, |m| bridge.set_notices(m));

    bridge.set_version(state.version.as_str().into());
    bridge.set_manifest_revision(state.manifest_revision as i32);

    match input.remote {
        None => {
            bridge.set_link(LinkState::Local);
            bridge.set_link_detail(SharedString::new());
        }
        Some((url, link)) => {
            let (l, detail) = match link {
                Link::Connected => (LinkState::Connected, url.to_string()),
                Link::Connecting => (LinkState::Connecting, url.to_string()),
                Link::Disconnected(reason) => (LinkState::Down, format!("{url} · {reason}")),
            };
            bridge.set_link(l);
            bridge.set_link_detail(detail.into());
        }
    }
}

fn accounts(state: &State) -> Vec<AccountItem> {
    let mut out: Vec<AccountItem> = state
        .accounts
        .iter()
        .map(|(id, acc)| {
            let (name, login, enabled) = match &acc.info {
                Some(i) => (i.display_name.clone(), i.login.clone(), i.enabled),
                None => (id.clone(), id.clone(), true),
            };
            let (status, detail) = match &acc.status {
                Some(AccountStatus::Running) => ("running", String::new()),
                Some(AccountStatus::Starting) => ("starting", String::new()),
                Some(AccountStatus::Stopped) | None => ("stopped", String::new()),
                Some(AccountStatus::AuthRequired) => ("auth_required", String::new()),
                Some(AccountStatus::Error { message }) => ("error", message.clone()),
                Some(AccountStatus::Standby { peer }) => ("standby", peer.clone()),
            };
            let avatar = images::get(acc.info.as_ref().and_then(|i| i.avatar_url.as_deref()));
            AccountItem {
                id: id.as_str().into(),
                initial: initial(&name).into(),
                name: name.into(),
                login: login.into(),
                avatar,
                status: status.into(),
                detail: detail.into(),
                enabled,
            }
        })
        .collect();
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// Plan entries of `module_data.drops.plan`, as published by the drops module. Only
/// the fields the views use; unknown fields are ignored.
#[derive(Debug, Clone, Deserialize)]
pub struct PlanEntry {
    pub campaign_id: String,
    pub campaign: String,
    pub game: String,
    pub status: String,
    pub ends_at: DateTime<Utc>,
    #[serde(default)]
    pub objective: Vec<String>,
    #[serde(default)]
    pub remaining_minutes: u32,
    #[serde(default)]
    pub missing_minutes: Option<u32>,
    #[serde(default)]
    pub link_url: Option<String>,
    #[serde(default)]
    pub game_rank: Option<usize>,
}

pub fn plan(account: &Account) -> Vec<PlanEntry> {
    account
        .module_data
        .get("drops")
        .and_then(|m| m.get("plan"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Plan entries that need the user, for games of the user's list only (like the alerts).
fn plan_todos(account: &Account, now: DateTime<Utc>) -> Vec<PlanTodo> {
    plan(account)
        .into_iter()
        .filter(|e| e.game_rank.is_some())
        .filter(|e| matches!(e.status.as_str(), "not_linked" | "at_risk" | "will_miss"))
        .map(|e| PlanTodo {
            kind: e.status.as_str().into(),
            campaign_id: e.campaign_id.as_str().into(),
            campaign: e.campaign.as_str().into(),
            game: e.game.as_str().into(),
            rewards: e.objective.join(", ").into(),
            link_url: e.link_url.unwrap_or_default().into(),
            ends_in: minutes_until(now, e.ends_at),
            remaining_minutes: e.remaining_minutes as i32,
            missing_minutes: e.missing_minutes.unwrap_or_default() as i32,
        })
        .collect()
}

/// "En ce moment", "Drop en cours", "Aujourd'hui" and the pause state.
fn home(bridge: &Bridge, state: &State, id: &str, account: Option<&Account>, input: &Input) {
    let summary = tm_core::view::summary(state, input.now);
    bridge.set_paused(summary.paused);
    let standby = match account.and_then(|a| a.status.as_ref()) {
        Some(AccountStatus::Standby { peer }) => Some(peer.clone()),
        _ => None,
    };
    let (idle, detail) = if summary.paused {
        (Idle::Paused, String::new())
    } else if let Some(peer) = standby {
        (Idle::Standby, peer)
    } else if !summary.watching_allowed {
        (Idle::OutsideHours, String::new())
    } else {
        (Idle::None, String::new())
    };
    bridge.set_idle(idle);
    bridge.set_idle_detail(detail.into());

    let Some(account) = account else {
        sync(bridge.get_watching(), Vec::new(), |m| {
            bridge.set_watching(m)
        });
        bridge.set_has_drop(false);
        bridge.set_today(TodayItem::default());
        return;
    };
    bridge.set_any_live(account.channels.values().any(|c| c.stream.is_some()));

    let mut slots: Vec<_> = account.watching.values().collect();
    slots.sort_by_key(|w| w.since);
    let watching = slots
        .into_iter()
        .map(|w| {
            let ch = account.channels.get(&w.channel_id);
            let name = ch
                .map(|c| c.display_name.clone())
                .unwrap_or_else(|| w.channel_id.clone());
            let stream = ch.and_then(|c| c.stream.as_ref());
            let (reason, detail) = reason(&w.reason);
            WatchItem {
                channel_id: w.channel_id.as_str().into(),
                initial: initial(&name).into(),
                avatar: images::get(ch.and_then(|c| c.avatar_url.as_deref())),
                name: name.into(),
                game: stream
                    .and_then(|s| s.game.as_ref())
                    .map(|g| g.name.as_str())
                    .unwrap_or_default()
                    .into(),
                title: stream.map(|s| s.title.as_str()).unwrap_or_default().into(),
                viewers: stream.map(|s| s.viewers as i32).unwrap_or_default(),
                reason: reason.into(),
                reason_detail: detail.into(),
                minutes: w.minutes as i32,
            }
        })
        .collect();
    sync(bridge.get_watching(), watching, |m| bridge.set_watching(m));

    let drop = summary
        .accounts
        .iter()
        .find(|a| a.id == id)
        .and_then(|a| a.drop.as_ref());
    bridge.set_has_drop(drop.is_some());
    if let Some(d) = drop {
        bridge.set_drop(DropItem {
            game: d.game.as_str().into(),
            campaign: d.campaign.as_str().into(),
            reward: d.drop.as_str().into(),
            image: images::get(d.image_url.as_deref()),
            current: d.current_minutes as i32,
            required: d.required_minutes as i32,
            progress: d.progress,
            eta_in: d.eta.map(|t| minutes_until(input.now, t)).unwrap_or(-1),
            remaining_drops: d.remaining_drops as i32,
        });
    }

    let today = input
        .today
        .filter(|t| t.account == id && t.day == today_key(input.now));
    bridge.set_today(match today {
        Some(t) => TodayItem {
            points: t.points as i32,
            minutes: t.minutes as i32,
            drops: t.drops as i32,
        },
        None => TodayItem::default(),
    });
}

/// Splits a watch reason (`drops: <game>`, `points: watch streak`, `points`…) into a
/// kind the views know and a detail.
fn reason(raw: &str) -> (&'static str, String) {
    let (kind, detail) = raw
        .split_once(':')
        .map(|(k, d)| (k.trim(), d.trim()))
        .unwrap_or((raw.trim(), ""));
    match kind {
        "drops" => ("drops", detail.to_string()),
        "points" if detail.contains("streak") => ("streak", String::new()),
        "points" => ("points", String::new()),
        _ => ("other", raw.to_string()),
    }
}

/// Key of today's row in `daily_stats` (the engine counts days in UTC).
pub fn today_key(now: DateTime<Utc>) -> String {
    now.format("%Y-%m-%d").to_string()
}

fn minutes_until(now: DateTime<Utc>, t: DateTime<Utc>) -> i32 {
    (t - now).num_minutes().clamp(0, i32::MAX as i64) as i32
}

fn logins(state: &State) -> Vec<LoginItem> {
    let mut flows: Vec<_> = state.logins.values().collect();
    flows.sort_by_key(|f| f.expires_at);
    flows
        .into_iter()
        .map(|f| LoginItem {
            flow_id: f.flow_id.as_str().into(),
            code: f.user_code.as_str().into(),
            url: f.verification_uri.as_str().into(),
            link: f
                .verification_uri_complete
                .as_deref()
                .unwrap_or(&f.verification_uri)
                .into(),
            expires_at: local_hm(f.expires_at).into(),
        })
        .collect()
}

fn notices(state: &State) -> Vec<NoticeItem> {
    let mut list: Vec<_> = state.notices.values().collect();
    list.sort_by_key(|n| std::cmp::Reverse(n.at));
    list.into_iter()
        .map(|n| NoticeItem {
            id: n.id.as_str().into(),
            level: match n.level {
                Level::Info => "info",
                Level::Warning => "warning",
                Level::Error => "error",
            }
            .into(),
            message: n.message.as_str().into(),
        })
        .collect()
}

pub fn initial(name: &str) -> String {
    name.chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "?".into())
}

pub fn local_hm(t: DateTime<Utc>) -> String {
    t.with_timezone(&Local).format("%H:%M").to_string()
}

/// Updates a list model in place (keeps delegates, scroll positions and hover states),
/// or installs a new `VecModel` the first time.
pub fn sync<T: Clone + PartialEq + 'static>(
    current: ModelRc<T>,
    rows: Vec<T>,
    install: impl FnOnce(ModelRc<T>),
) {
    let Some(model) = current.as_any().downcast_ref::<VecModel<T>>() else {
        install(ModelRc::from(Rc::new(VecModel::from(rows))));
        return;
    };
    for (i, row) in rows.iter().enumerate() {
        if i < model.row_count() {
            if model.row_data(i).as_ref() != Some(row) {
                model.set_row_data(i, row.clone());
            }
        } else {
            model.push(row.clone());
        }
    }
    while model.row_count() > rows.len() {
        model.remove(model.row_count() - 1);
    }
}

/// Last state and link, kept on the UI thread to re-render on a timer (relative times).
#[derive(Default)]
pub struct Last {
    pub state: Arc<State>,
    pub remote: Option<(String, Link)>,
    pub today: Option<Today>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_reasons() {
        assert_eq!(reason("drops: Rust"), ("drops", "Rust".to_string()));
        assert_eq!(reason("points: watch streak"), ("streak", String::new()));
        assert_eq!(reason("points"), ("points", String::new()));
        assert_eq!(reason("raid"), ("other", "raid".to_string()));
    }

    #[test]
    fn initials() {
        assert_eq!(initial("thinsho"), "T");
        assert_eq!(initial("_élodie"), "É");
        assert_eq!(initial(""), "?");
    }
}
