//! Development only: a `Backend` serving a canned, realistic state (`TM_UI_DEMO=1`,
//! or `TM_UI_DEMO=paused`), to see and screenshot every view without a Twitch
//! account. Commands answer plausible data but change nothing.

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use serde_json::{Value, json};
use tm_client::{Backend, Link};
use tm_core::{ApiResult, Command, Envelope, State};
use tokio::sync::{broadcast, watch};

pub struct Demo {
    state: watch::Receiver<Arc<State>>,
    _state_tx: watch::Sender<Arc<State>>,
    events: broadcast::Sender<Arc<Envelope>>,
    link: watch::Receiver<Link>,
    _link_tx: watch::Sender<Link>,
}

impl Demo {
    pub fn new() -> Self {
        let mut fixture = fixture(Utc::now());
        if std::env::var("TM_UI_DEMO").is_ok_and(|v| v == "paused") {
            fixture["settings"]["engine"] = json!({"paused": true});
            fixture["accounts"]["1001"]["watching"] = json!({});
        }
        let state = serde_json::from_value(fixture).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "demo fixture does not match State");
            State::default()
        });
        let (state_tx, state_rx) = watch::channel(Arc::new(state));
        let (link_tx, link_rx) = watch::channel(Link::Connected);
        Self {
            state: state_rx,
            _state_tx: state_tx,
            events: broadcast::channel(16).0,
            link: link_rx,
            _link_tx: link_tx,
        }
    }
}

#[async_trait::async_trait]
impl Backend for Demo {
    fn state(&self) -> watch::Receiver<Arc<State>> {
        self.state.clone()
    }
    fn events(&self) -> broadcast::Receiver<Arc<Envelope>> {
        self.events.subscribe()
    }
    fn link(&self) -> watch::Receiver<Link> {
        self.link.clone()
    }
    async fn execute(&self, cmd: Command) -> ApiResult {
        let q = serde_json::to_value(&cmd).unwrap_or_default();
        Ok(match q["cmd"].as_str() {
            Some("stats") => demo_stats(&q),
            Some("history") => demo_history(&q),
            // Schemas as published by the engine (snapshot, development only).
            Some("modules_list") => schemas()["modules"].clone(),
            Some("settings_get") if q["scope"] == "engine" => schemas()["engine"].clone(),
            _ => Value::Null,
        })
    }
}

fn schemas() -> Value {
    serde_json::from_str(include_str!("demo_schemas.json")).unwrap_or_default()
}

/// Daily rows over the asked period (a deterministic wave), or totals per channel.
fn demo_stats(q: &Value) -> Value {
    if q["group_by"] == "channel" {
        return json!([
            {"key": "12", "points": 5210, "watch_minutes": 610, "drops": 0},
            {"key": "11", "points": 3870, "watch_minutes": 980, "drops": 5},
            {"key": "14", "points": 1460, "watch_minutes": 220, "drops": 0},
            {"key": "15", "points": 640, "watch_minutes": 95, "drops": 0}
        ]);
    }
    let from = q["from"]
        .as_str()
        .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok());
    let to = q["to"]
        .as_str()
        .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok());
    let (Some(from), Some(to)) = (from, to) else {
        return json!([]);
    };
    let mut rows = Vec::new();
    let mut day = from;
    let mut i = 0i64;
    while day <= to {
        // Some days without activity, others with more.
        let wave = [0, 620, 1340, 980, 0, 1810, 2240, 760, 1450, 1990][(i % 10) as usize];
        if wave > 0 {
            rows.push(json!({
                "key": day.format("%Y-%m-%d").to_string(),
                "points": wave,
                "watch_minutes": wave / 9,
                "drops": (i % 4 == 1) as i64,
            }));
        }
        day += Duration::days(1);
        i += 1;
    }
    Value::Array(rows)
}

/// A page of plausible history rows, newest first.
fn demo_history(q: &Value) -> Value {
    let now = Utc::now();
    let before = q["before_id"].as_i64().unwrap_or(1000);
    let kinds: Vec<&str> = q["kinds"]
        .as_array()
        .map(|k| k.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    let samples = [
        (
            "points_earned",
            Some("11"),
            json!({"amount": 50, "reason": "CLAIM", "balance": 48210}),
        ),
        (
            "drop_claimed",
            None,
            json!({"campaign_id": "c-rust", "drop_id": "d1", "drop_name": "Hazmat Hoodie", "game": "Rust"}),
        ),
        (
            "points_earned",
            Some("12"),
            json!({"amount": 450, "reason": "WATCH_STREAK", "balance": 152340}),
        ),
        (
            "watch_started",
            Some("12"),
            json!({"reason": "points: watch streak"}),
        ),
        (
            "prediction_result",
            Some("12"),
            json!({"event_id": "e1", "result": "WIN", "points_won": 1200}),
        ),
        (
            "raid_joined",
            Some("14"),
            json!({"target_login": "otplol_"}),
        ),
        ("watch_stopped", Some("14"), json!({"cause": "offline"})),
        (
            "drop_alert",
            None,
            json!({"alert": {"kind": "at_risk", "campaign_id": "c-mr", "campaign": "Season 4 Drops", "game": "Marvel Rivals", "ends_at": ts(now + Duration::hours(9)), "rewards": ["Jeff Spray"]}}),
        ),
        (
            "points_earned",
            Some("11"),
            json!({"amount": 10, "reason": "WATCH", "balance": 48160}),
        ),
        (
            "account_status_changed",
            None,
            json!({"status": {"state": "running"}}),
        ),
    ];
    let rows: Vec<Value> = samples
        .iter()
        .enumerate()
        .filter(|(_, (kind, _, _))| kinds.is_empty() || kinds.contains(kind))
        .map(|(i, (kind, channel, fields))| {
            let id = before - 1 - i as i64;
            let mut event = fields.clone();
            event["type"] = json!(kind);
            event["account"] = json!("1001");
            if let Some(c) = channel {
                event["channel_id"] = json!(c);
            }
            json!({
                "id": id, "seq": id, "account": "1001", "kind": kind, "channel_id": channel,
                "ts": ts(now - Duration::minutes(7 * i as i64 + (1000 - before) * 3)),
                "event": event,
            })
        })
        .collect();
    Value::Array(rows)
}

fn ts(t: DateTime<Utc>) -> String {
    t.to_rfc3339()
}

fn game(id: &str, name: &str) -> Value {
    json!({"id": id, "name": name})
}

fn fixture(now: DateTime<Utc>) -> Value {
    let h = |n: i64| ts(now + Duration::hours(n));
    let m = |n: i64| ts(now + Duration::minutes(n));
    json!({
        "seq": 1,
        "version": "0.1.0-demo",
        "instance_id": "demo",
        "started_at": m(-95),
        "manifest_revision": 2,
        "settings": {"engine": {
            "active_hours": [{"days": [1, 2, 3, 4, 5], "start": "18:00", "end": "02:00"},
                             {"days": [6, 7], "start": "10:00", "end": "23:30"}]
        }, "module:predictions": {"enabled": true, "percent": 2.5},
        "account:1001/module:points": {
            "channels": ["rustgod", "zerator", "deadlockdev", "newchannel"],
            "exclude": ["somebot"]
        }, "module:drops": {
            "games": ["Rust", "Marvel Rivals", "Valorant", "Deadlock", "Apex Legends"],
            "exclude_games": ["Fortnite"],
            "wanted_rewards": ["Garage Door"]
        }},
        "notices": {
            "update": {"id": "update", "level": "info",
                "message": "TwitchMiner 0.2.0 is available.", "at": m(-30)}
        },
        "logins": {},
        "accounts": {
            "1001": {
                "info": {"id": "1001", "login": "thinsho", "display_name": "Thinsho",
                    "avatar_url": null, "enabled": true},
                "status": {"state": "running"},
                "modules": {"drops": {"state": "running"}, "points": {"state": "running"}},
                "channels": {
                    "11": {"id": "11", "login": "rustgod", "display_name": "RustGod",
                        "avatar_url": null,
                        "stream": {"id": "s11", "title": "Wipe day! Drops on", "viewers": 18342,
                            "started_at": m(-140), "game": game("g1", "Rust")},
                        "points": 48210, "multiplier": 1.0, "sources": ["drops", "points"],
                        "session_points": 620, "session_minutes": 74},
                    "12": {"id": "12", "login": "zerator", "display_name": "ZeratoR",
                        "avatar_url": null,
                        "stream": {"id": "s12", "title": "Soirée découverte", "viewers": 25780,
                            "started_at": m(-35), "game": game("g9", "Just Chatting")},
                        "points": 152340, "multiplier": 1.2, "sources": ["points"],
                        "session_points": 410, "session_minutes": 21},
                    "14": {"id": "14", "login": "kamet0", "display_name": "Kamet0",
                        "avatar_url": null,
                        "stream": {"id": "s14", "title": "Ranked jusqu'au Master", "viewers": 9120,
                            "started_at": m(-90), "game": game("g5", "League of Legends")},
                        "points": 3150, "multiplier": 1.0, "sources": ["points"],
                        "session_points": 0, "session_minutes": 0},
                    "15": {"id": "15", "login": "gotaga", "display_name": "Gotaga",
                        "avatar_url": null, "stream": null,
                        "points": 870, "multiplier": 2.0, "sources": ["points"],
                        "session_points": 0, "session_minutes": 0},
                    "13": {"id": "13", "login": "deadlockdev", "display_name": "DeadlockDev",
                        "avatar_url": null, "stream": null,
                        "points": null, "multiplier": 1.0, "sources": ["points"],
                        "session_points": 0, "session_minutes": 0}
                },
                "watching": {
                    "11": {"channel_id": "11", "reason": "drops: Rust", "since": m(-74), "minutes": 74},
                    "12": {"channel_id": "12", "reason": "points: watch streak", "since": m(-21), "minutes": 21}
                },
                "campaigns": {
                    "c-rust": {
                        "id": "c-rust", "name": "Rust Twitch Drops #38", "game": game("g1", "Rust"),
                        "status": "ACTIVE", "starts_at": h(-48), "ends_at": h(70),
                        "image_url": null, "details_url": null, "link_url": null, "linked": true,
                        "allowed_channels": null,
                        "drops": [
                            {"id": "d1", "name": "Hazmat Hoodie", "starts_at": h(-48), "ends_at": h(70),
                             "required_minutes": 120, "current_minutes": 120, "claimed": true,
                             "claim_id": null, "preconditions": [], "benefits": []},
                            {"id": "d2", "name": "Tempered AK-47", "starts_at": h(-48), "ends_at": h(70),
                             "required_minutes": 240, "current_minutes": 158, "claimed": false,
                             "claim_id": null, "preconditions": [], "benefits": []},
                            {"id": "d3", "name": "Garage Door Skin", "starts_at": h(-48), "ends_at": h(70),
                             "required_minutes": 360, "current_minutes": 158, "claimed": false,
                             "claim_id": null, "preconditions": [], "benefits": []}
                        ]
                    },
                    "c-ow": {
                        "id": "c-ow", "name": "Overwatch League Viewer Rewards", "game": game("g7", "Overwatch 2"),
                        "status": "ACTIVE", "starts_at": h(-10), "ends_at": h(300),
                        "image_url": null, "details_url": null, "link_url": null, "linked": true,
                        "allowed_channels": null, "drops": []
                    },
                    "c-mr": {
                        "id": "c-mr", "name": "Season 4 Drops", "game": game("g2", "Marvel Rivals"),
                        "status": "ACTIVE", "starts_at": h(-20), "ends_at": h(9),
                        "image_url": null, "details_url": "https://example.com/campaign", "link_url": null,
                        "linked": true, "allowed_channels": null,
                        "drops": [
                            {"id": "m1", "name": "Jeff Spray", "starts_at": h(-20), "ends_at": h(9),
                             "required_minutes": 300, "current_minutes": 0, "claimed": false,
                             "claim_id": null, "preconditions": [], "benefits": []}
                        ]
                    }
                },
                "drop_target": {"campaign_id": "c-rust", "drop_id": "d2", "channel_id": "11", "game": "Rust"},
                "session": {"points_earned": 1030, "watch_minutes": 95, "drops_claimed": 1,
                    "bonuses_claimed": 6, "raids_joined": 0, "moments_claimed": 0},
                "module_data": {
                    "drops": {
                        "focus": null,
                        "plan": [
                            {"campaign_id": "c-rust", "campaign": "Rust Twitch Drops #38", "game": "Rust",
                             "status": "active", "ends_at": h(70), "objective": ["Garage Door Skin"],
                             "remaining_minutes": 202, "game_rank": 0},
                            {"campaign_id": "c-mr", "campaign": "Season 4 Drops", "game": "Marvel Rivals",
                             "status": "at_risk", "ends_at": h(9), "objective": ["Jeff Spray"],
                             "remaining_minutes": 300, "start_at": h(4), "eta": h(9), "game_rank": 1},
                            {"campaign_id": "c-dl", "campaign": "Deadlock Launch", "game": "Deadlock",
                             "status": "not_linked", "ends_at": h(200), "objective": ["Founder Badge"],
                             "remaining_minutes": 180,
                             "link_url": "https://example.com/link", "game_rank": 3},
                            {"campaign_id": "c-vct", "campaign": "VCT Champions", "game": "Valorant",
                             "status": "restricted", "ends_at": h(120), "objective": ["Champions Spray"],
                             "remaining_minutes": 120, "game_rank": 2},
                            {"campaign_id": "c-apex", "campaign": "Apex Legends Global Series", "game": "Apex Legends",
                             "status": "planned", "ends_at": h(160), "objective": ["Pathfinder Charm"],
                             "remaining_minutes": 240, "start_at": h(14), "eta": h(19), "game_rank": 4},
                            {"campaign_id": "c-rust2", "campaign": "Rust Streamer Drops", "game": "Rust",
                             "status": "will_miss", "ends_at": h(30), "objective": ["Neon Door"],
                             "remaining_minutes": 600, "missing_minutes": 170, "game_rank": 0},
                            {"campaign_id": "c-val2", "campaign": "Episode 10 Drops", "game": "Valorant",
                             "status": "upcoming", "ends_at": h(400), "objective": ["Gun Buddy"],
                             "remaining_minutes": 180, "starts_at": h(40), "game_rank": 2},
                            {"campaign_id": "c-apex2", "campaign": "Holo-Day Bash", "game": "Apex Legends",
                             "status": "skipped", "ends_at": h(90), "objective": ["Holo Banner"],
                             "remaining_minutes": 60, "skipped_until": h(1), "game_rank": 4},
                            {"campaign_id": "c-mr0", "campaign": "Launch Drops", "game": "Marvel Rivals",
                             "status": "done", "ends_at": h(50), "objective": ["Launch Nameplate"],
                             "remaining_minutes": 0, "game_rank": 1}
                        ]
                    }
                }
            }
        }
    })
}
