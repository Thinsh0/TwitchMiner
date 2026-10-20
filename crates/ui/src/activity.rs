//! Activity screen: what the `stats` and `history` queries return, shaped for the
//! views. These are query results (asked again when needed), not engine state.

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde_json::{Value, json};
use tm_core::State;

use crate::present::{today_key, tz_offset_minutes, when_at};
use crate::{ChartBar, HistoryItem};

/// Page size of the history list.
pub const PAGE: usize = 50;

/// What the view asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub account: String,
    /// 0 = points, 1 = minutes, 2 = drops
    pub metric: i32,
    /// 0 = per day, 1 = per channel
    pub group: i32,
    pub days: i32,
    pub filter: i32,
}

pub fn stats_query(c: &Choice, now: DateTime<Utc>) -> Value {
    let to = today_key(now);
    let from = today_key(now - Duration::days(i64::from(c.days.max(1) - 1)));
    json!({
        "cmd": "stats",
        "account": c.account,
        "from": from,
        "to": to,
        "group_by": if c.group == 1 { "channel" } else { "day" },
        "tz_offset_minutes": tz_offset_minutes(),
    })
}

pub fn history_query(c: &Choice, before_id: Option<i64>) -> Value {
    let kinds: &[&str] = match c.filter {
        1 => &["points_earned", "points_spent"],
        2 => &["drop_claimed", "drop_alert"],
        3 => &["watch_started", "watch_stopped"],
        4 => &[
            "prediction_placed",
            "prediction_result",
            "raid_joined",
            "moment_claimed",
        ],
        5 => &["account_status_changed", "notice_raised", "login_finished"],
        _ => &[],
    };
    json!({
        "cmd": "history",
        "account": c.account,
        "kinds": kinds,
        "before_id": before_id,
        "limit": PAGE,
    })
}

fn metric_of(row: &Value, metric: i32) -> i64 {
    let key = match metric {
        1 => "watch_minutes",
        2 => "drops",
        _ => "points",
    };
    row[key].as_i64().unwrap_or_default()
}

/// Bars of the chart, and the total over the period.
pub fn chart(
    rows: &Value,
    c: &Choice,
    state: &State,
    now: DateTime<Utc>,
    lang: i32,
) -> (Vec<ChartBar>, i64) {
    let rows = rows.as_array().cloned().unwrap_or_default();
    let mut bars: Vec<(String, i64)> = if c.group == 1 {
        let channels = state.account(&c.account).map(|a| &a.channels);
        let mut v: Vec<(String, i64)> = rows
            .iter()
            .map(|r| {
                let id = r["key"].as_str().unwrap_or_default();
                let name = channels
                    .and_then(|ch| ch.get(id))
                    .map(|ch| ch.display_name.clone())
                    .unwrap_or_else(|| id.to_string());
                (name, metric_of(r, c.metric))
            })
            .filter(|(_, v)| *v != 0)
            .collect();
        v.sort_by_key(|b| std::cmp::Reverse(b.1));
        v.truncate(12);
        v
    } else {
        // Every day of the period, empty days included (the engine counts days in UTC).
        let today = NaiveDate::parse_from_str(&today_key(now), "%Y-%m-%d")
            .unwrap_or_else(|_| now.date_naive());
        (0..c.days.max(1))
            .rev()
            .map(|back| {
                let day = today - Duration::days(i64::from(back));
                let key = day.format("%Y-%m-%d").to_string();
                let value = rows
                    .iter()
                    .filter(|r| r["key"].as_str() == Some(key.as_str()))
                    .map(|r| metric_of(r, c.metric))
                    .sum();
                let label = day
                    .format(if lang == 1 { "%m/%d" } else { "%d/%m" })
                    .to_string();
                (label, value)
            })
            .collect()
    };
    let total = bars.iter().map(|b| b.1).sum();
    let max = bars.iter().map(|b| b.1).max().unwrap_or_default().max(1);
    let out = bars
        .drain(..)
        .map(|(label, value)| ChartBar {
            label: label.into(),
            value: value.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
            ratio: (value.max(0) as f32 / max as f32).clamp(0.0, 1.0),
        })
        .collect();
    (out, total)
}

/// One history row, for the view to phrase. Fields per kind:
/// - points_earned: n = amount, a = reason (`WATCH`, `CLAIM`, `WATCH_STREAK`, `RAID`…)
/// - points_spent: n = amount
/// - raid_joined: a = target login
/// - prediction_placed: n = points, a = outcome, b = title
/// - prediction_result: n = points won (net), a = `WIN` | `LOSE` | `REFUND`
/// - watch_started: a = reason kind (drops, streak, points, other), b = detail
/// - watch_stopped: a = cause
/// - drop_claimed: a = reward, b = game
/// - drop_alert: a = alert kind, b = game, c = campaign
/// - account_status_changed: a = state, b = peer or message
/// - notice_raised: a = message
/// - login_finished: a = error (empty = success)
pub fn history_item(row: &Value, state: &State, now: DateTime<Utc>, lang: i32) -> HistoryItem {
    let ev = &row["event"];
    let kind = row["kind"].as_str().unwrap_or_default().to_string();
    let s = |v: &Value| v.as_str().unwrap_or_default().to_string();
    let channel = row["channel_id"]
        .as_str()
        .map(|id| {
            row["account"]
                .as_str()
                .and_then(|a| state.account(a))
                .and_then(|a| a.channels.get(id))
                .map(|c| c.display_name.clone())
                .unwrap_or_else(|| id.to_string())
        })
        .unwrap_or_default();
    let (n, a, b, c) = match kind.as_str() {
        "points_earned" => (
            ev["amount"].as_i64(),
            s(&ev["reason"]),
            String::new(),
            String::new(),
        ),
        "points_spent" => (
            ev["amount"].as_i64(),
            String::new(),
            String::new(),
            String::new(),
        ),
        "raid_joined" => (None, s(&ev["target_login"]), String::new(), String::new()),
        "prediction_placed" => (
            ev["points"].as_i64(),
            s(&ev["outcome"]),
            s(&ev["title"]),
            String::new(),
        ),
        "prediction_result" => (
            ev["points_won"].as_i64(),
            s(&ev["result"]),
            String::new(),
            String::new(),
        ),
        "watch_started" => {
            let (k, d) = crate::present::reason(ev["reason"].as_str().unwrap_or_default());
            (None, k.to_string(), d, String::new())
        }
        "watch_stopped" => (None, s(&ev["cause"]), String::new(), String::new()),
        "drop_claimed" => (None, s(&ev["drop_name"]), s(&ev["game"]), String::new()),
        "drop_alert" => (
            None,
            s(&ev["alert"]["kind"]),
            s(&ev["alert"]["game"]),
            s(&ev["alert"]["campaign"]),
        ),
        "account_status_changed" => {
            let st = &ev["status"];
            let detail = st["peer"]
                .as_str()
                .or(st["message"].as_str())
                .unwrap_or_default();
            (None, s(&st["state"]), detail.to_string(), String::new())
        }
        "notice_raised" => (
            None,
            s(&ev["notice"]["message"]),
            String::new(),
            String::new(),
        ),
        "login_finished" => (None, s(&ev["error"]), String::new(), String::new()),
        _ => (None, String::new(), String::new(), String::new()),
    };
    let ts = row["ts"]
        .as_str()
        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
        .map(|t| t.with_timezone(&Utc));
    HistoryItem {
        id: row["id"].as_i64().unwrap_or_default() as i32,
        kind: kind.into(),
        when: when_at(ts, now, lang),
        channel: channel.into(),
        n: n.unwrap_or_default()
            .clamp(i32::MIN as i64, i32::MAX as i64) as i32,
        a: a.into(),
        b: b.into(),
        c: c.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(group: i32) -> Choice {
        Choice {
            account: "1".into(),
            metric: 0,
            group,
            days: 3,
            filter: 0,
        }
    }

    #[test]
    fn days_without_activity_are_zero() {
        let now = Utc::now();
        let today = today_key(now);
        let rows = json!([{"key": today, "points": 120, "watch_minutes": 30, "drops": 1}]);
        let (bars, total) = chart(&rows, &choice(0), &State::default(), now, 0);
        assert_eq!(bars.len(), 3);
        assert_eq!(
            bars.iter().map(|b| b.value).collect::<Vec<_>>(),
            vec![0, 0, 120]
        );
        assert_eq!(total, 120);
        assert_eq!(bars[2].ratio, 1.0);
    }

    #[test]
    fn history_rows_are_phrased_by_kind() {
        let row = json!({
            "id": 7, "seq": 1, "ts": "2026-10-20T10:00:00Z", "account": "1",
            "kind": "points_earned", "channel_id": "42",
            "event": {"type": "points_earned", "account": "1", "channel_id": "42",
                      "amount": 50, "reason": "CLAIM", "balance": 1000}
        });
        let item = history_item(&row, &State::default(), Utc::now(), 0);
        assert_eq!(item.kind, "points_earned");
        assert_eq!(item.n, 50);
        assert_eq!(item.a, "CLAIM");
        assert_eq!(item.channel, "42");
    }
}
