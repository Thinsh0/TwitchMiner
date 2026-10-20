//! State → view: turns the engine state mirror into the `Bridge` properties.
//! Pure projection, recomputed on every state change; nothing here is kept as truth.

use std::rc::Rc;
use std::sync::Arc;

use chrono::{DateTime, Local, Utc};
use serde::Deserialize;
use serde_json::Value;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use tm_client::Link;
use tm_core::State;
use tm_core::state::{Account, AccountStatus, Level};

use crate::images;
use crate::{
    AccountItem, Bridge, ChannelItem, DropItem, DropLine, GameItem, Idle, LinkState, LoginItem,
    MainWindow, NoticeItem, PlanRow, PlanTodo, TodayItem, WatchItem, When,
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
    bridge.set_account_name(
        account
            .and_then(|a| a.info.as_ref())
            .map(|i| i.display_name.as_str())
            .unwrap_or_default()
            .into(),
    );
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
    drops(&bridge, state, selected.as_str(), account, input);
    channels(&bridge, state, selected.as_str(), account);
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
    pub start_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub eta: Option<DateTime<Utc>>,
    #[serde(default)]
    pub missing_minutes: Option<u32>,
    #[serde(default)]
    pub starts_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub skipped_until: Option<DateTime<Utc>>,
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
pub fn reason(raw: &str) -> (&'static str, String) {
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

/// A module setting as the module sees it (top-level key): the account's override,
/// else the shared scope. `None` = module default.
pub fn module_setting<'a>(
    state: &'a State,
    account: &str,
    module: &str,
    key: &str,
) -> Option<&'a Value> {
    use tm_core::settings::{account_module_scope, module_scope};
    state
        .settings
        .get(&account_module_scope(account, module))
        .and_then(|v| v.get(key))
        .or_else(|| {
            state
                .settings
                .get(&module_scope(module))
                .and_then(|v| v.get(key))
        })
}

/// Scope to write `key` of `module` to: the account's override if it already holds
/// that key (it would hide the shared value), else the shared scope.
pub fn module_setting_scope(state: &State, account: &str, module: &str, key: &str) -> String {
    use tm_core::settings::{account_module_scope, module_scope};
    let account_scope = account_module_scope(account, module);
    if state
        .settings
        .get(&account_scope)
        .is_some_and(|v| v.get(key).is_some())
    {
        account_scope
    } else {
        module_scope(module)
    }
}

pub fn string_list(v: Option<&Value>) -> Vec<String> {
    v.and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// The Drops screen: plan, game list, exclusions and wanted rewards.
fn drops(bridge: &Bridge, state: &State, id: &str, account: Option<&Account>, input: &Input) {
    let lang = bridge.get_language();
    let Some(account) = account else {
        sync(bridge.get_plan(), Vec::new(), |m| bridge.set_plan(m));
        return;
    };
    let entries = plan(account);
    let rows: Vec<PlanRow> = entries
        .iter()
        .map(|e| plan_row(account, e, input.now, lang))
        .collect();
    sync(bridge.get_plan(), rows, |m| bridge.set_plan(m));
    let focus = account
        .module_data
        .get("drops")
        .and_then(|m| m.get("focus"))
        .and_then(|f| f.get("campaign_id"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    bridge.set_focus_id(focus.into());
    bridge.set_drops_enabled(
        module_setting(state, id, "drops", "enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
    );

    let games = string_list(module_setting(state, id, "drops", "games"));
    let excluded = string_list(module_setting(state, id, "drops", "exclude_games"));
    let rewards = string_list(module_setting(state, id, "drops", "wanted_rewards"));
    let same = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
    let items: Vec<GameItem> = games
        .iter()
        .map(|g| GameItem {
            name: g.as_str().into(),
            campaigns: account
                .campaigns
                .values()
                .filter(|c| same(&c.game.name, g) && c.is_active(input.now))
                .count() as i32,
        })
        .collect();
    let mut suggestions: Vec<String> = account
        .campaigns
        .values()
        .filter(|c| c.is_active(input.now))
        .map(|c| c.game.name.clone())
        .filter(|n| !games.iter().chain(&excluded).any(|g| same(g, n)))
        .collect();
    suggestions.sort_by_key(|s| s.to_lowercase());
    suggestions.dedup();
    sync(bridge.get_games(), items, |m| bridge.set_games(m));
    sync(
        bridge.get_exclude_games(),
        excluded.into_iter().map(Into::into).collect(),
        |m| bridge.set_exclude_games(m),
    );
    sync(
        bridge.get_wanted_rewards(),
        rewards.into_iter().map(Into::into).collect(),
        |m| bridge.set_wanted_rewards(m),
    );
    sync(
        bridge.get_game_suggestions(),
        suggestions.into_iter().map(Into::into).collect(),
        |m| bridge.set_game_suggestions(m),
    );
}

/// The Channels screen: configured channels in priority order, then followed ones.
fn channels(bridge: &Bridge, state: &State, id: &str, account: Option<&Account>) {
    let Some(account) = account else {
        sync(bridge.get_my_channels(), Vec::new(), |m| {
            bridge.set_my_channels(m)
        });
        sync(bridge.get_all_channels(), Vec::new(), |m| {
            bridge.set_all_channels(m)
        });
        return;
    };
    let setting = |key| module_setting(state, id, "points", key);
    let configured = string_list(setting("channels"));
    let excluded = string_list(setting("exclude"));
    bridge.set_follows(setting("follows").and_then(|v| v.as_bool()).unwrap_or(true));
    bridge.set_points_enabled(setting("enabled").and_then(|v| v.as_bool()).unwrap_or(true));
    bridge.set_points_order(match setting("order_by").and_then(|v| v.as_str()) {
        Some("multiplier") => 1,
        Some("fewest_points") => 2,
        Some("most_points") => 3,
        _ => 0,
    });
    let item = |c: &tm_core::state::Channel| {
        let stream = c.stream.as_ref();
        ChannelItem {
            id: c.id.as_str().into(),
            login: c.login.as_str().into(),
            initial: initial(&c.display_name).into(),
            name: c.display_name.as_str().into(),
            avatar: images::get(c.avatar_url.as_deref()),
            live: stream.is_some(),
            game: stream
                .and_then(|s| s.game.as_ref())
                .map(|g| g.name.as_str())
                .unwrap_or_default()
                .into(),
            title: stream.map(|s| s.title.as_str()).unwrap_or_default().into(),
            viewers: stream.map(|s| s.viewers as i32).unwrap_or_default(),
            points: c.points.unwrap_or_default().min(i32::MAX as u64) as i32,
            points_known: c.points.is_some(),
            multiplier: if (c.multiplier - 1.0).abs() < f64::EPSILON {
                SharedString::new()
            } else {
                format!("×{}", (c.multiplier * 100.0).round() / 100.0).into()
            },
            session_points: c.session_points as i32,
            session_minutes: c.session_minutes as i32,
            watching: account.watching.contains_key(&c.id),
            pending: false,
            priority: false,
            excluded: false,
        }
    };
    let by_login = |login: &str| {
        account
            .channels
            .values()
            .find(|c| c.login.eq_ignore_ascii_case(login))
    };
    // Priority channels (the `channels` setting), in their order.
    let mine: Vec<ChannelItem> = configured
        .iter()
        .map(|login| match by_login(login) {
            Some(c) => ChannelItem {
                priority: true,
                ..item(c)
            },
            None => ChannelItem {
                login: login.as_str().into(),
                name: login.as_str().into(),
                initial: initial(login).into(),
                pending: true,
                priority: true,
                excluded: excluded.iter().any(|e| e.eq_ignore_ascii_case(login)),
                ..Default::default()
            },
        })
        .collect();
    let followed: Vec<ChannelItem> = account
        .channels
        .values()
        .filter(|c| c.sources.contains("points"))
        .filter(|c| !configured.iter().any(|l| l.eq_ignore_ascii_case(&c.login)))
        .map(item)
        .collect();
    // Every channel mined for points, in one list: watched, then live, then the
    // rest; priority channels first within each group.
    let mut all: Vec<ChannelItem> = mine.iter().cloned().chain(followed).collect();
    all.sort_by_key(|c| (!c.watching, !c.live, !c.priority, c.name.to_lowercase()));
    bridge.set_channels_live(all.iter().filter(|c| c.live).count() as i32);
    bridge.set_channels_count(all.len() as i32);
    bridge.set_channels_session_points(account.session.points_earned as i32);
    bridge.set_watch_slots(
        tm_core::settings::EngineSettings::from_state(state).max_watch_slots as i32,
    );
    sync(bridge.get_my_channels(), mine, |m| {
        bridge.set_my_channels(m)
    });
    sync(bridge.get_all_channels(), all, |m| {
        bridge.set_all_channels(m)
    });
    sync(
        bridge.get_excluded_channels(),
        excluded.into_iter().map(Into::into).collect(),
        |m| bridge.set_excluded_channels(m),
    );
}

fn plan_row(account: &Account, e: &PlanEntry, now: DateTime<Utc>, lang: i32) -> PlanRow {
    let campaign = account.campaigns.get(&e.campaign_id);
    let image = campaign.and_then(|c| {
        c.image_url.clone().or_else(|| {
            c.game
                .box_art_url
                .as_ref()
                .map(|u| u.replace("{width}", "144").replace("{height}", "192"))
        })
    });
    // Objective progress: its minutes, prerequisites included (chained drops).
    let total = campaign
        .map(|c| {
            c.drops
                .iter()
                .filter(|d| {
                    e.objective
                        .iter()
                        .any(|o| d.name == *o || d.benefits.iter().any(|b| b.name == *o))
                })
                .map(|d| chain_minutes(c, &d.id, 0))
                .max()
                .unwrap_or_default()
        })
        .unwrap_or_default();
    let total = total.max(e.remaining_minutes);
    let drops = campaign
        .map(|c| {
            c.drops
                .iter()
                .map(|d| DropLine {
                    name: d
                        .benefits
                        .first()
                        .map(|b| b.name.as_str())
                        .unwrap_or(&d.name)
                        .into(),
                    current: d.current_minutes.min(d.required_minutes) as i32,
                    required: d.required_minutes as i32,
                    progress: if d.claimed {
                        1.0
                    } else if d.required_minutes == 0 {
                        0.0
                    } else {
                        (d.current_minutes as f32 / d.required_minutes as f32).clamp(0.0, 1.0)
                    },
                    claimed: d.claimed,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let channels = campaign
        .and_then(|c| c.allowed_channels.as_ref())
        .map(|list| {
            list.iter()
                .map(|ch| {
                    ch.display_name
                        .clone()
                        .or_else(|| ch.login.clone())
                        .unwrap_or_else(|| ch.id.clone())
                })
                .take(6)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let when = match e.status.as_str() {
        "planned" | "at_risk" => e.start_at,
        "upcoming" => e.starts_at,
        "skipped" => e.skipped_until,
        _ => None,
    };
    PlanRow {
        campaign_id: e.campaign_id.as_str().into(),
        campaign: e.campaign.as_str().into(),
        game: e.game.as_str().into(),
        image: images::get(image.as_deref()),
        status: e.status.as_str().into(),
        rewards: e.objective.join(", ").into(),
        has_progress: total > 0,
        progress: if total == 0 {
            0.0
        } else {
            1.0 - e.remaining_minutes as f32 / total as f32
        },
        remaining_minutes: e.remaining_minutes as i32,
        missing_minutes: e.missing_minutes.unwrap_or_default() as i32,
        when: when_at(when, now, lang),
        eta: when_at(e.eta, now, lang),
        ends: when_at(Some(e.ends_at), now, lang),
        link_url: e.link_url.clone().unwrap_or_default().into(),
        details_url: campaign
            .and_then(|c| c.details_url.clone())
            .unwrap_or_default()
            .into(),
        channels: channels.into(),
        listed: e.game_rank.is_some(),
        drops: ModelRc::from(Rc::new(VecModel::from(drops))),
    }
}

/// Minutes to reach a drop, its prerequisites included.
fn chain_minutes(c: &tm_core::twitch::models::Campaign, drop_id: &str, depth: u8) -> u32 {
    let Some(d) = c.drops.iter().find(|d| d.id == drop_id) else {
        return 0;
    };
    if depth > 8 {
        return d.required_minutes;
    }
    let before = d
        .preconditions
        .iter()
        .map(|p| chain_minutes(c, p, depth + 1))
        .max()
        .unwrap_or_default();
    before + d.required_minutes
}

/// Game names the engine knows (campaigns, live channels), matching `text`: names
/// starting with it first, then names containing it. `taken` names are left out.
pub fn game_matches(account: &Account, text: &str, taken: &[String], max: usize) -> Vec<String> {
    let q = text.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut names: Vec<String> = account
        .campaigns
        .values()
        .map(|c| c.game.name.clone())
        .chain(
            account
                .channels
                .values()
                .filter_map(|c| c.stream.as_ref()?.game.as_ref().map(|g| g.name.clone())),
        )
        .filter(|n| !n.is_empty() && !taken.iter().any(|t| t.eq_ignore_ascii_case(n)))
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    let mut out: Vec<String> = names
        .iter()
        .filter(|n| n.to_lowercase().starts_with(&q))
        .cloned()
        .collect();
    out.extend(
        names
            .iter()
            .filter(|n| {
                let l = n.to_lowercase();
                !l.starts_with(&q) && l.contains(&q)
            })
            .cloned(),
    );
    out.truncate(max);
    out
}

/// A point in time for the views (local time, day relative to today).
pub fn when_at(t: Option<DateTime<Utc>>, now: DateTime<Utc>, lang: i32) -> When {
    let Some(t) = t else {
        return When::default();
    };
    let local = t.with_timezone(&Local);
    let today = now.with_timezone(&Local).date_naive();
    let days = (local.date_naive() - today).num_days();
    When {
        day_offset: if (-1..=1).contains(&days) {
            days as i32
        } else {
            99
        },
        date: local
            .format(if lang == 1 { "%m/%d" } else { "%d/%m" })
            .to_string()
            .into(),
        time: local.format("%H:%M").to_string().into(),
        known: true,
    }
}

/// Today's key in `stats` rows (local days: queries send [`tz_offset_minutes`]).
pub fn today_key(now: DateTime<Utc>) -> String {
    now.with_timezone(&Local).format("%Y-%m-%d").to_string()
}

/// This machine's offset from UTC, sent with `stats` queries so a remote engine cuts
/// days in the viewer's time zone.
pub fn tz_offset_minutes() -> i32 {
    Local::now().offset().local_minus_utc() / 60
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
    fn game_names_match_by_prefix_then_substring() {
        let mut acc = Account::default();
        for (id, name) in [
            ("1", "Rust"),
            ("2", "Trust Me"),
            ("3", "Rocket League"),
            ("4", "rust"),
        ] {
            let c: tm_core::twitch::models::Campaign = serde_json::from_value(serde_json::json!({
                "id": id, "name": "c", "game": {"id": id, "name": name}, "status": "ACTIVE",
                "starts_at": null, "ends_at": "2030-01-01T00:00:00Z", "image_url": null,
                "details_url": null, "link_url": null, "linked": null,
                "allowed_channels": null, "drops": []
            }))
            .unwrap();
            acc.campaigns.insert(id.into(), c);
        }
        assert_eq!(game_matches(&acc, "ru", &[], 6), vec!["Rust", "Trust Me"]);
        assert_eq!(
            game_matches(&acc, "r", &["Rust".into()], 6),
            vec!["Rocket League", "Trust Me"]
        );
        assert!(game_matches(&acc, "  ", &[], 6).is_empty());
    }

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
