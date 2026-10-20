//! State → view: turns the engine state mirror into the `Bridge` properties.
//! Pure projection, recomputed on every state change; nothing here is kept as truth.

use std::rc::Rc;
use std::sync::Arc;

use chrono::{DateTime, Local, Utc};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use tm_client::Link;
use tm_core::State;
use tm_core::state::{AccountStatus, Level};

use crate::{AccountItem, Bridge, LinkState, LoginItem, MainWindow, NoticeItem};

/// What the projection needs besides the state.
pub struct Input<'a> {
    pub state: &'a State,
    /// `None` for a local engine.
    pub remote: Option<(&'a str, &'a Link)>,
}

pub fn apply(ui: &MainWindow, input: &Input) {
    let bridge = ui.global::<Bridge>();
    let state = input.state;

    let accounts = accounts(state);
    let selected = bridge.get_account();
    if !accounts.iter().any(|a| a.id == selected) {
        bridge.set_account(accounts.first().map(|a| a.id.clone()).unwrap_or_default());
    }
    let logins = logins(state);
    let notices = notices(state);
    let auth = if logins.is_empty() {
        accounts
            .iter()
            .filter(|a| a.status == "auth_required")
            .count()
    } else {
        0
    };
    bridge.set_todo_count((logins.len() + notices.len() + auth) as i32);
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
            AccountItem {
                id: id.as_str().into(),
                initial: initial(&name).into(),
                name: name.into(),
                login: login.into(),
                avatar: Default::default(),
                status: status.into(),
                detail: detail.into(),
                enabled,
            }
        })
        .collect();
    out.sort_by_key(|a| a.name.to_lowercase());
    out
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials() {
        assert_eq!(initial("thinsho"), "T");
        assert_eq!(initial("_élodie"), "É");
        assert_eq!(initial(""), "?");
    }
}
