//! What a guest token sees and may do: only the Twitch accounts its holder added
//! (`tm_core::guests`). Everything else is removed from what is sent (state, events,
//! replies), not only hidden by the interface, and every command is checked.
//!
//! A guest may: read the shared module settings (the defaults its accounts inherit;
//! notification destinations excluded) and the engine settings without secrets; add
//! accounts (they become theirs); change, pause, remove and configure their accounts
//! (`account:<own>/module:*` only); read their history and stats. A guest may not:
//! see or touch other accounts, the shared or engine settings, the guests, the logs,
//! backups, updates or the MCP endpoint.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tm_core::event::{Envelope, Event};
use tm_core::guests;
#[cfg(test)]
use tm_core::guests::GUESTS_SCOPE;
use tm_core::settings::{ENGINE_SCOPE, EngineSettings, Scope, parse_scope};
use tm_core::twitch::manifest::merge_json;
use tm_core::twitch::netguard;
use tm_core::{ApiError, Command, State};

use crate::access;

/// One guest's view, read from the state when needed (it changes as they add accounts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub id: String,
    pub accounts: BTreeSet<String>,
}

fn denied(what: &str) -> ApiError {
    ApiError::new("forbidden", format!("a guest token cannot {what}"))
}

/// Game searches per guest in the current minute.
static SEARCHES: LazyLock<Mutex<HashMap<String, (Instant, u32)>>> = LazyLock::new(Default::default);

/// Counts one game search for `guest`; `false` past `per_minute` in the minute.
fn allow_search(guest: &str, per_minute: u32) -> bool {
    let Ok(mut all) = SEARCHES.lock() else {
        return true;
    };
    let now = Instant::now();
    let minute = Duration::from_secs(60);
    if all.len() > 1000 {
        all.retain(|_, (since, _)| now.duration_since(*since) < minute);
    }
    let entry = all.entry(guest.to_string()).or_insert((now, 0));
    if now.duration_since(entry.0) >= minute {
        *entry = (now, 0);
    }
    entry.1 += 1;
    entry.1 <= per_minute
}

/// Checks one notification destination of a guest: the server will post to it, so
/// only public https addresses, without extra headers (no request forgery towards
/// the server's network). Unknown kinds are refused (safe for kinds added later).
fn check_guest_target(t: &Value) -> Result<(), ApiError> {
    let refused = || ApiError::new("forbidden", netguard::REFUSED);
    let url = |field: &str| {
        t.get(field)
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::invalid(format!("`{field}` missing")))
            .and_then(|u| netguard::check_public_url(u).map_err(|_| refused()))
            .map(drop)
    };
    match t.get("kind").and_then(Value::as_str) {
        Some("discord") => url("webhook_url"),
        Some("ntfy") => url("url"),
        Some("webhook") => {
            let headers = t.get("headers");
            if headers
                .is_some_and(|h| !(h.is_null() || h.as_object().is_some_and(|o| o.is_empty())))
            {
                return Err(denied("send extra headers"));
            }
            url("url")
        }
        // Fixed hosts (api.telegram.org, Google): the module checks the rest.
        Some("telegram") | Some("fcm") => Ok(()),
        _ => Err(denied("use this kind of destination")),
    }
}

impl View {
    /// `None` once the guest was removed (the token no longer works).
    pub fn of(state: &State, id: &str) -> Option<View> {
        let g = guests::all(state).remove(id)?;
        Some(View {
            id: id.to_string(),
            accounts: g.accounts.into_iter().collect(),
        })
    }

    fn owns(&self, account: &str) -> bool {
        self.accounts.contains(account)
    }

    /// Notices are global (`update`) or about one account (`<kind>:<account>`).
    fn notice_is_mine(&self, id: &str) -> bool {
        id.split_once(':').is_some_and(|(_, acc)| self.owns(acc))
    }

    /// Settings scopes the guest may read (`true`) and how their value is cleaned.
    fn readable_scope(&self, scope: &str) -> bool {
        match parse_scope(scope) {
            Some(Scope::Engine) => true,
            Some(Scope::Module(m)) => m != "notify",
            Some(Scope::AccountModule { account, .. }) => self.owns(account),
            Some(Scope::Guests) | None => false,
        }
    }

    /// Only the engine document carries secrets a guest may not see (proxy, peers);
    /// their own accounts' notification destinations are theirs.
    fn clean_settings(scope: &str, value: &mut Value) {
        if scope == ENGINE_SCOPE {
            access::redact_settings(scope, value);
        }
    }

    /// Removes from `state` everything that is not the guest's.
    pub fn filter_state(&self, state: &mut State) {
        state.accounts.retain(|id, _| self.owns(id));
        state
            .logins
            .retain(|_, f| f.guest.as_deref() == Some(self.id.as_str()));
        state.notices.retain(|id, _| self.notice_is_mine(id));
        state.settings.retain(|scope, _| self.readable_scope(scope));
        for (scope, value) in state.settings.iter_mut() {
            Self::clean_settings(scope, value);
        }
    }

    /// The event as sent to the guest, or `None` when it is not theirs. `flows` keeps
    /// the guest's login flows, to recognise their `login_finished`.
    pub fn filter_event(&self, env: &Envelope, flows: &mut HashSet<String>) -> Option<Value> {
        if let Some(account) = env.event.account() {
            return self.owns(account).then(|| json!(env));
        }
        match &env.event {
            Event::EngineStarted { .. }
            | Event::ManifestUpdated { .. }
            | Event::WatchingAllowed { .. } => Some(json!(env)),
            Event::SettingsChanged { scope, .. } => {
                if !self.readable_scope(scope) {
                    return None;
                }
                let mut v = json!(env);
                Self::clean_settings(scope, &mut v["value"]);
                Some(v)
            }
            Event::NoticeRaised { notice } => self.notice_is_mine(&notice.id).then(|| json!(env)),
            Event::NoticeCleared { id } => self.notice_is_mine(id).then(|| json!(env)),
            Event::LoginStarted { flow } => {
                if flow.guest.as_deref() != Some(self.id.as_str()) {
                    return None;
                }
                flows.insert(flow.flow_id.clone());
                Some(json!(env))
            }
            Event::LoginFinished { flow_id, .. } => flows.remove(flow_id).then(|| json!(env)),
            // Anything else names no account of theirs: never sent (safe default for
            // events added later).
            _ => None,
        }
    }

    /// The command as the engine will run it for the guest, or why it is refused.
    /// `state` is the current one (login flows, accounts).
    pub fn authorize(&self, cmd: Command, state: &State) -> Result<Command, ApiError> {
        let own = |account: &str, what: &str| {
            if self.owns(account) {
                Ok(())
            } else {
                Err(denied(what))
            }
        };
        let limits = || EngineSettings::from_state(state).guest_limits;
        match cmd {
            Command::Snapshot | Command::ModulesList | Command::Summary => Ok(cmd),
            Command::GamesSearch { .. } => {
                if allow_search(&self.id, limits().games_search_per_minute) {
                    Ok(cmd)
                } else {
                    Err(ApiError::new(
                        "rate_limited",
                        "too many game searches: wait a minute",
                    ))
                }
            }
            Command::LoginStart { profile, .. } => {
                let max = limits().max_pending_logins;
                let pending = state
                    .logins
                    .values()
                    .filter(|f| f.guest.as_deref() == Some(self.id.as_str()))
                    .count();
                if pending >= max as usize {
                    return Err(ApiError::new(
                        "forbidden",
                        format!("at most {max} logins at a time: finish or cancel one first"),
                    ));
                }
                Ok(Command::LoginStart {
                    profile,
                    guest: Some(self.id.clone()),
                })
            }
            Command::LoginCancel { ref flow_id } => {
                let mine = state
                    .logins
                    .get(flow_id)
                    .is_some_and(|f| f.guest.as_deref() == Some(self.id.as_str()));
                if mine {
                    Ok(cmd)
                } else {
                    Err(denied("cancel another login"))
                }
            }
            Command::AccountRemove { ref account } => {
                own(account, "remove another account")?;
                Ok(cmd)
            }
            Command::AccountSetEnabled { ref account, .. } => {
                own(account, "change another account")?;
                Ok(cmd)
            }
            Command::SettingsGet {
                ref scope,
                ref account,
            } => {
                if !self.readable_scope(scope) {
                    return Err(denied("read these settings"));
                }
                if let Some(a) = account {
                    own(a, "read another account's settings")?;
                }
                Ok(cmd)
            }
            Command::SettingsSet {
                scope,
                value,
                merge,
            } => {
                let notify = match parse_scope(&scope) {
                    Some(Scope::AccountModule { account, module }) if self.owns(account) => {
                        module == "notify"
                    }
                    _ => return Err(denied("change settings other than its own accounts'")),
                };
                if !notify {
                    return Ok(Command::SettingsSet {
                        scope,
                        value,
                        merge,
                    });
                }
                let value = self.checked_notify(&scope, value, merge, state)?;
                Ok(Command::SettingsSet {
                    scope,
                    value,
                    merge: false,
                })
            }
            Command::History { mut query } => {
                query.account = Some(self.only_account(query.account)?);
                Ok(Command::History { query })
            }
            Command::Stats { mut query } => {
                query.account = Some(self.only_account(query.account)?);
                Ok(Command::Stats { query })
            }
            Command::ModuleCall { ref account, .. } => {
                own(account, "act for another account")?;
                Ok(cmd)
            }
            Command::NoticeDismiss { ref id } => {
                if self.notice_is_mine(id) {
                    Ok(cmd)
                } else {
                    Err(denied("dismiss this notice"))
                }
            }
            Command::LoginWithToken { .. } => Err(denied("add accounts from raw tokens")),
            Command::Backup { .. } => Err(denied("write backups")),
            Command::ManifestRefresh | Command::UpdateCheck => {
                Err(denied("run engine maintenance"))
            }
            Command::GuestInvite { .. } | Command::GuestRemove { .. } => {
                Err(denied("manage guests"))
            }
        }
    }

    /// The complete notification settings a guest's change leads to (sent whole, so
    /// what is checked is what is stored): destinations checked and counted, never a
    /// Firebase key of their own (the server's is used for their phones), and always a
    /// `targets` list (without one, the owner's destinations would be inherited).
    fn checked_notify(
        &self,
        scope: &str,
        value: Value,
        merge: bool,
        state: &State,
    ) -> Result<Value, ApiError> {
        if value
            .get("firebase_key")
            .and_then(Value::as_str)
            .is_some_and(|k| !k.is_empty() && k != "***")
        {
            return Err(denied("set a Firebase key (the server's is used)"));
        }
        let mut next = if value.is_null() {
            json!({ "targets": [] })
        } else if merge {
            let mut base = state
                .settings
                .get(scope)
                .cloned()
                .unwrap_or_else(|| json!({}));
            merge_json(&mut base, &value);
            base
        } else {
            value
        };
        let obj = next
            .as_object_mut()
            .ok_or_else(|| ApiError::invalid("settings must be a JSON object"))?;
        obj.remove("firebase_key");
        let targets = obj
            .entry("targets")
            .or_insert_with(|| json!([]))
            .as_array()
            .ok_or_else(|| ApiError::invalid("`targets` must be a list"))?;
        let max = EngineSettings::from_state(state)
            .guest_limits
            .max_notify_targets;
        if targets.len() > max as usize {
            return Err(ApiError::new(
                "forbidden",
                format!("at most {max} notification destinations per account"),
            ));
        }
        targets.iter().try_for_each(check_guest_target)?;
        Ok(next)
    }

    /// History and stats always name one of the guest's accounts.
    fn only_account(&self, asked: Option<String>) -> Result<String, ApiError> {
        match asked {
            Some(a) if self.owns(&a) => Ok(a),
            Some(_) => Err(denied("read another account's history")),
            None if self.accounts.len() == 1 => {
                Ok(self.accounts.iter().next().cloned().unwrap_or_default())
            }
            None => Err(ApiError::invalid("name the account")),
        }
    }

    /// Cleans a reply: the state, the summary and settings are recomputed or masked.
    pub fn filter_reply(&self, kind: &ReplyKind, out: &mut Value, now_state: &State) {
        match kind {
            ReplyKind::Snapshot => {
                let mut s = now_state.clone();
                self.filter_state(&mut s);
                *out = json!(s);
            }
            ReplyKind::Summary => {
                let mut s = now_state.clone();
                self.filter_state(&mut s);
                *out = json!(tm_core::view::summary(&s, chrono::Utc::now()));
            }
            ReplyKind::Settings(scope) => {
                for k in ["raw", "effective"] {
                    Self::clean_settings(scope, &mut out[k]);
                }
                // The effective destinations of an account without its own list are
                // the owner's: never shown.
                if scope.ends_with("/module:notify") && out["raw"].get("targets").is_none() {
                    access::redact_settings("module:notify", &mut out["effective"]);
                }
                // Nor the owner's Firebase key, used for the guest's phones too.
                if scope.ends_with("/module:notify")
                    && out["raw"].get("firebase_key").is_none()
                    && let Some(key) = out["effective"].get_mut("firebase_key")
                    && key.as_str().is_some_and(|k| !k.is_empty())
                {
                    *key = json!("***");
                }
            }
            ReplyKind::Other => {}
        }
    }
}

/// What a reply contains, for [`View::filter_reply`].
pub enum ReplyKind {
    Snapshot,
    Summary,
    Settings(String),
    Other,
}

impl ReplyKind {
    pub fn of(cmd: &Command) -> ReplyKind {
        match cmd {
            Command::Snapshot => ReplyKind::Snapshot,
            Command::Summary => ReplyKind::Summary,
            Command::SettingsGet { scope, .. } | Command::SettingsSet { scope, .. } => {
                ReplyKind::Settings(scope.clone())
            }
            _ => ReplyKind::Other,
        }
    }
}

#[cfg(test)]
const ENGINE: &str = ENGINE_SCOPE;

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tm_core::state::{Account, LoginFlow, Notice};

    fn state() -> State {
        let mut s = State::default();
        for id in ["mine", "other"] {
            s.accounts.insert(id.into(), Account::default());
        }
        s.settings.insert(
            GUESTS_SCOPE.into(),
            json!({"g": {"name": "Sam", "token_sha256": "x", "accounts": ["mine"],
                         "created_at": "2026-10-26T00:00:00Z"}}),
        );
        s.settings.insert(
            ENGINE.into(),
            json!({"proxy": "http://u:SECRET@h:1", "paused": false}),
        );
        s.settings.insert(
            "module:notify".into(),
            json!({"targets": [{"kind": "ntfy", "url": "https://ntfy.sh/OWNER-TOPIC"}]}),
        );
        s.settings
            .insert("module:drops".into(), json!({"games": ["Rust"]}));
        s.settings.insert(
            "account:other/module:drops".into(),
            json!({"games": ["OTHER-GAME"]}),
        );
        s.settings.insert(
            "account:mine/module:drops".into(),
            json!({"games": ["MY-GAME"]}),
        );
        s.settings.insert(
            "account:mine/module:notify".into(),
            json!({"targets": [{"kind": "ntfy", "url": "https://ntfy.sh/MY-TOPIC"}]}),
        );
        let notice = |id: &str| Notice {
            id: id.into(),
            level: tm_core::state::Level::Warning,
            message: String::new(),
            at: Utc::now(),
            kind: None,
            params: Value::Null,
        };
        for id in ["update", "duplicate:other", "duplicate:mine"] {
            s.notices.insert(id.into(), notice(id));
        }
        let flow = |id: &str, guest: Option<&str>| LoginFlow {
            flow_id: id.into(),
            user_code: "CODE".into(),
            verification_uri: String::new(),
            verification_uri_complete: None,
            expires_at: Utc::now(),
            guest: guest.map(str::to_string),
        };
        s.logins.insert("f-mine".into(), flow("f-mine", Some("g")));
        s.logins.insert("f-owner".into(), flow("f-owner", None));
        s
    }

    #[test]
    fn state_keeps_only_what_is_theirs() {
        let s = state();
        let view = View::of(&s, "g").unwrap();
        let mut f = s.clone();
        view.filter_state(&mut f);
        let text = json!(f).to_string();
        assert!(f.accounts.contains_key("mine") && !f.accounts.contains_key("other"));
        assert!(f.logins.contains_key("f-mine") && !f.logins.contains_key("f-owner"));
        assert_eq!(f.notices.keys().collect::<Vec<_>>(), ["duplicate:mine"]);
        for leak in ["SECRET", "OWNER-TOPIC", "OTHER-GAME", "token_sha256"] {
            assert!(!text.contains(leak), "{leak} leaked");
        }
        assert!(text.contains("MY-GAME") && text.contains("Rust") && text.contains("MY-TOPIC"));
        assert!(View::of(&s, "removed").is_none());
    }

    #[test]
    fn events_of_others_are_not_sent() {
        let s = state();
        let view = View::of(&s, "g").unwrap();
        let mut flows = HashSet::new();
        let env = |event| Envelope {
            seq: 1,
            ts: Utc::now(),
            event,
        };
        let status = |a: &str| Event::AccountStatusChanged {
            account: a.into(),
            status: tm_core::state::AccountStatus::Running,
        };
        assert!(
            view.filter_event(&env(status("mine")), &mut flows)
                .is_some()
        );
        assert!(
            view.filter_event(&env(status("other")), &mut flows)
                .is_none()
        );
        let set = |scope: &str, value: Value| Event::SettingsChanged {
            scope: scope.into(),
            value,
        };
        assert!(
            view.filter_event(&env(set(GUESTS_SCOPE, json!({}))), &mut flows)
                .is_none()
        );
        assert!(
            view.filter_event(&env(set("module:notify", json!({}))), &mut flows)
                .is_none()
        );
        let engine = view
            .filter_event(
                &env(set(ENGINE, json!({"proxy": "http://u:SECRET@h:1"}))),
                &mut flows,
            )
            .unwrap();
        assert!(!engine.to_string().contains("SECRET"));
        let finished = |id: &str| Event::LoginFinished {
            flow_id: id.into(),
            account: Some("x".into()),
            error: None,
        };
        assert!(
            view.filter_event(&env(finished("f-owner")), &mut flows)
                .is_none()
        );
        let started = Event::LoginStarted {
            flow: s.logins["f-mine"].clone(),
        };
        assert!(view.filter_event(&env(started), &mut flows).is_some());
        assert!(
            view.filter_event(&env(finished("f-mine")), &mut flows)
                .is_some()
        );
    }

    #[test]
    fn commands_are_limited_to_their_accounts() {
        let s = state();
        let view = View::of(&s, "g").unwrap();
        let ok = |c: Command| view.authorize(c, &s).is_ok();
        let set = |scope: &str| Command::SettingsSet {
            scope: scope.into(),
            value: json!({}),
            merge: true,
        };
        assert!(ok(set("account:mine/module:drops")));
        assert!(!ok(set("account:other/module:drops")));
        assert!(!ok(set("module:drops")));
        assert!(!ok(set(ENGINE)));
        assert!(!ok(set(GUESTS_SCOPE)));
        assert!(ok(Command::AccountRemove {
            account: "mine".into()
        }));
        assert!(!ok(Command::AccountRemove {
            account: "other".into()
        }));
        assert!(!ok(Command::GuestInvite { name: "x".into() }));
        assert!(!ok(Command::Backup { path: None }));
        assert!(!ok(Command::LoginCancel {
            flow_id: "f-owner".into()
        }));
        assert!(ok(Command::LoginCancel {
            flow_id: "f-mine".into()
        }));
        match view
            .authorize(
                Command::LoginStart {
                    profile: None,
                    guest: Some("someone-else".into()),
                },
                &s,
            )
            .unwrap()
        {
            Command::LoginStart { guest, .. } => assert_eq!(guest.as_deref(), Some("g")),
            _ => unreachable!(),
        }
        match view
            .authorize(
                Command::History {
                    query: Default::default(),
                },
                &s,
            )
            .unwrap()
        {
            Command::History { query } => assert_eq!(query.account.as_deref(), Some("mine")),
            _ => unreachable!(),
        }
        let hq = tm_core::db::HistoryQuery {
            account: Some("other".into()),
            ..Default::default()
        };
        assert!(!ok(Command::History { query: hq }));
        assert!(!ok(Command::SettingsGet {
            scope: "account:other/module:drops".into(),
            account: None
        }));
        assert!(!ok(Command::SettingsGet {
            scope: "module:notify".into(),
            account: None
        }));
        assert!(!ok(Command::NoticeDismiss {
            id: "duplicate:other".into()
        }));
        assert!(!ok(Command::ModuleCall {
            account: "other".into(),
            module: "drops".into(),
            name: "refresh".into(),
            args: Value::Null
        }));
    }

    fn notify_set(value: Value) -> Command {
        Command::SettingsSet {
            scope: "account:mine/module:notify".into(),
            value,
            merge: true,
        }
    }

    #[test]
    fn guest_destinations_are_checked() {
        let s = state();
        let view = View::of(&s, "g").unwrap();
        let run = |c: Command| view.authorize(c, &s);
        let targets = |t: Value| notify_set(json!({ "targets": [t] }));
        for ok in [
            json!({"kind": "ntfy", "url": "https://ntfy.sh/topic"}),
            json!({"kind": "discord", "webhook_url": "https://discord.com/api/webhooks/1/x"}),
            json!({"kind": "webhook", "url": "https://hooks.example.com/x", "token": "t"}),
            json!({"kind": "telegram", "bot_token": "1:abc", "chat_id": "2"}),
            json!({"kind": "fcm", "token": "t", "device": "d", "name": "Pixel"}),
        ] {
            assert!(run(targets(ok.clone())).is_ok(), "{ok}");
        }
        for bad in [
            json!({"kind": "ntfy", "url": "http://ntfy.sh/topic"}),
            json!({"kind": "ntfy", "url": "https://192.168.1.10/topic"}),
            json!({"kind": "ntfy", "url": "https://100.101.102.103:8080/topic"}),
            json!({"kind": "webhook", "url": "https://localhost:7878/api/v1/command"}),
            json!({"kind": "webhook", "url": "https://169.254.169.254/latest"}),
            json!({"kind": "webhook", "url": "https://hooks.example.com/x", "headers": {"X": "1"}}),
            json!({"kind": "discord", "webhook_url": "https://mini-pc/x"}),
            json!({"kind": "smtp", "url": "https://mail.example.com"}),
        ] {
            let e = run(targets(bad.clone())).unwrap_err();
            assert_eq!(e.code, "forbidden", "{bad}");
        }
        // No Firebase key of their own.
        assert!(
            run(notify_set(
                json!({"firebase_key": "{\"type\": \"service_account\"}"})
            ))
            .is_err()
        );
        // The stored document always has its own list and never a key.
        match run(notify_set(
            json!({"firebase_key": "***", "events": ["drop_claimed"]}),
        ))
        .unwrap()
        {
            Command::SettingsSet { value, merge, .. } => {
                assert!(!merge);
                assert!(value.get("firebase_key").is_none());
                assert_eq!(value["targets"][0]["url"], "https://ntfy.sh/MY-TOPIC");
                assert_eq!(value["events"][0], "drop_claimed");
            }
            _ => unreachable!(),
        }
        match run(Command::SettingsSet {
            scope: "account:mine/module:notify".into(),
            value: Value::Null,
            merge: false,
        })
        .unwrap()
        {
            Command::SettingsSet { value, .. } => assert_eq!(value, json!({"targets": []})),
            _ => unreachable!(),
        }
        // At most `max_notify_targets` destinations.
        let many: Vec<Value> = (0..11)
            .map(|i| json!({"kind": "ntfy", "url": format!("https://ntfy.sh/t{i}")}))
            .collect();
        assert!(run(notify_set(json!({ "targets": many[..10] }))).is_ok());
        assert!(run(notify_set(json!({ "targets": many }))).is_err());
    }

    #[test]
    fn guest_limits() {
        let mut s = state();
        let view = View::of(&s, "g").unwrap();
        let login = || Command::LoginStart {
            profile: None,
            guest: None,
        };
        // One flow is already waiting (`f-mine`): one more is allowed by default.
        assert!(view.authorize(login(), &s).is_ok());
        let mut second = s.logins["f-mine"].clone();
        second.flow_id = "f-mine-2".into();
        s.logins.insert(second.flow_id.clone(), second);
        assert!(view.authorize(login(), &s).is_err());
        // Game searches per minute.
        s.settings.insert(
            ENGINE.into(),
            json!({"guest_limits": {"games_search_per_minute": 3}}),
        );
        let view = View {
            id: "limits-test".into(),
            ..view
        };
        let search = || Command::GamesSearch {
            query: "rust".into(),
            limit: None,
        };
        for _ in 0..3 {
            assert!(view.authorize(search(), &s).is_ok());
        }
        assert_eq!(
            view.authorize(search(), &s).unwrap_err().code,
            "rate_limited"
        );
    }

    #[test]
    fn summary_counts_only_their_accounts() {
        let s = state();
        let view = View::of(&s, "g").unwrap();
        let mut out = Value::Null;
        view.filter_reply(&ReplyKind::Summary, &mut out, &s);
        let ids: Vec<&str> = out["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|a| a["id"].as_str())
            .collect();
        assert!(!ids.contains(&"other"));
    }

    #[test]
    fn the_owner_firebase_key_stays_hidden() {
        let s = state();
        let view = View::of(&s, "g").unwrap();
        let kind = ReplyKind::Settings("account:mine/module:notify".into());
        let mut out = json!({
            "raw": {"targets": [{"kind": "fcm", "token": "t"}]},
            "effective": {"targets": [{"kind": "fcm", "token": "t"}], "firebase_key": "{\"private_key\": \"PEM\"}"},
        });
        view.filter_reply(&kind, &mut out, &s);
        assert_eq!(out["effective"]["firebase_key"], "***");
        assert_eq!(out["effective"]["targets"][0]["token"], "t");
    }
}
