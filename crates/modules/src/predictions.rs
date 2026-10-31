//! Predictions: optional automatic betting of channel points (disabled by default).

use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tm_core::event::Event;
use tm_core::{Module, ModuleContext, ModuleInfo};
use tm_twitch::pubsub::topics;
use tm_twitch::{PubSubEvent, PubSubMessage};
use tokio::time::Instant;

pub struct Predictions;

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    /// Outcome chosen by most users.
    #[default]
    MostVoted,
    /// Outcome with the most points on it.
    MostPoints,
    /// Outcome with the best payout (fewest points on it).
    HighOdds,
    /// Most voted, unless the vote is close: then the best payout.
    Smart,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
#[serde(default)]
pub struct PredictionSettings {
    pub enabled: bool,
    pub strategy: Strategy,
    /// Percentage of the channel balance to bet.
    pub percent: f64,
    pub max_points: u64,
    /// Never bet when the balance is below this.
    pub min_balance: u64,
    /// Skip predictions with fewer participants than this when betting.
    pub min_voters: u64,
    /// Bet this many seconds before the prediction window closes.
    pub seconds_before_close: u64,
    /// Logins to bet on; empty = every tracked channel.
    pub channels: Vec<String>,
    /// Skip when the chosen outcome pays less than this (×, e.g. 1.5). 0 = no limit.
    pub min_odds: f64,
    /// Skip when the chosen outcome pays more than this (long shots). 0 = no limit.
    pub max_odds: f64,
    /// Never bet more than the biggest bet already on the chosen outcome.
    pub stealth: bool,
}

impl Default for PredictionSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            strategy: Strategy::Smart,
            percent: 5.0,
            max_points: 50_000,
            min_balance: 2_000,
            min_voters: 20,
            seconds_before_close: 6,
            channels: Vec::new(),
            min_odds: 0.0,
            max_odds: 0.0,
            stealth: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub id: String,
    pub title: String,
    pub users: u64,
    pub points: u64,
    /// Biggest single bet on this outcome (0 = unknown).
    pub top_bet: u64,
}

/// What `points / outcome` pays back for a bet on `o` (total pool / outcome pool).
pub fn odds(o: &Outcome, outcomes: &[Outcome]) -> f64 {
    let total: u64 = outcomes.iter().map(|x| x.points).sum();
    if o.points == 0 {
        return 0.0;
    }
    total as f64 / o.points as f64
}

/// Results of this account's bets, kept across restarts (`module_data.predictions.summary`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Summary {
    pub bets: u64,
    pub wins: u64,
    pub losses: u64,
    pub refunds: u64,
    /// Points won minus points bet.
    pub net: i64,
}

impl Summary {
    fn add(&mut self, result: &str, net: i64) {
        self.bets += 1;
        match result {
            "WIN" => self.wins += 1,
            "LOSE" => self.losses += 1,
            _ => self.refunds += 1,
        }
        self.net += net;
    }
}

const SUMMARY_KEY: &str = "summary";

#[derive(Debug, Clone)]
struct Pending {
    channel_id: String,
    title: String,
    outcomes: Vec<Outcome>,
    bet_at: Instant,
    open: bool,
    placed: bool,
}

#[async_trait::async_trait]
impl Module for Predictions {
    fn info(&self) -> ModuleInfo {
        ModuleInfo::new(
            "predictions",
            "Predictions",
            "Bets a share of your channel points on predictions using a configurable strategy.",
            json!(PredictionSettings::default()),
        )
        .with_schema::<PredictionSettings>()
    }

    fn validate_settings(&self, v: &Value) -> Result<(), String> {
        let s: PredictionSettings = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
        check_settings(&s)
    }

    async fn run(&self, ctx: ModuleContext) -> anyhow::Result<()> {
        let uid = ctx.account_id().to_string();
        let mut settings: PredictionSettings = ctx.settings()?;
        let mut topics = ctx.pubsub().topic_set();
        let mut ps = ctx.pubsub().subscribe();
        let mut pending: HashMap<String, Pending> = HashMap::new();
        let mut refresh = tokio::time::interval(Duration::from_secs(120));
        let mut summary = match ctx.kv_get::<Summary>(SUMMARY_KEY).await.ok().flatten() {
            Some(s) => s,
            None => {
                // First run with a summary: start from the results already in the history.
                let s = summary_from_history(&ctx).await;
                let _ = ctx.kv_put(SUMMARY_KEY, &s).await;
                s
            }
        };
        ctx.set_data("summary", json!(summary));

        loop {
            let next = pending
                .values()
                .filter(|p| p.open && !p.placed)
                .map(|p| p.bet_at)
                .min();
            tokio::select! {
                _ = ctx.cancelled() => return Ok(()),
                _ = ctx.settings_changed() => settings = ctx.settings()?,
                _ = refresh.tick() => {
                    let wanted = wanted_channels(&ctx, &settings);
                    let mut t: Vec<String> = wanted.iter().map(|id| topics::predictions_channel(id)).collect();
                    t.push(topics::predictions_user(&uid));
                    topics.set(t);
                    pending.retain(|_, p| p.open || p.bet_at.elapsed() < Duration::from_secs(3600));
                }
                _ = async { match next { Some(t) => tokio::time::sleep_until(t).await, None => std::future::pending().await } } => {
                    let due: Vec<String> = pending.iter().filter(|(_, p)| p.open && !p.placed && p.bet_at <= Instant::now()).map(|(k, _)| k.clone()).collect();
                    for id in due {
                        if let Some(p) = pending.get_mut(&id) {
                            p.placed = true;
                            let p = p.clone();
                            place(&ctx, &settings, &id, &p).await;
                        }
                    }
                }
                ev = ps.recv() => match ev {
                    Ok(PubSubEvent::Message(m)) => {
                        if let Some((result, net)) = on_message(&ctx, &settings, &mut pending, &m) {
                            summary.add(&result, net);
                            if let Err(e) = ctx.kv_put(SUMMARY_KEY, &summary).await {
                                tracing::warn!(error = %e, "could not save predictions summary");
                            }
                            ctx.set_data("summary", json!(summary));
                        }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => return Ok(()),
                },
                call = ctx.next_call() => {
                    let Some(call) = call else { return Ok(()) };
                    let _ = call.reply.send(Ok(json!({ "pending": pending.len(), "summary": summary })));
                }
            }
        }
    }
}

/// The summary of the bet results kept in the history (`prediction_result`).
async fn summary_from_history(ctx: &ModuleContext) -> Summary {
    let mut s = Summary::default();
    let mut before_id = None;
    loop {
        let q = tm_core::db::HistoryQuery {
            account: Some(ctx.account_id().to_string()),
            kinds: vec!["prediction_result".into()],
            before_id,
            limit: Some(1000),
            ..Default::default()
        };
        let rows = ctx.db().history(q).await.unwrap_or_default();
        for row in &rows {
            let result = row.event["result"].as_str().unwrap_or_default();
            s.add(result, row.event["points_won"].as_i64().unwrap_or(0));
        }
        match rows.last() {
            Some(last) if rows.len() == 1000 => before_id = Some(last.id),
            _ => return s,
        }
    }
}

fn wanted_channels(ctx: &ModuleContext, s: &PredictionSettings) -> Vec<String> {
    let only: Vec<String> = s.channels.iter().map(|c| c.to_lowercase()).collect();
    ctx.store().read(|st| {
        st.accounts
            .get(ctx.account_id())
            .map(|a| {
                a.channels
                    .values()
                    .filter(|c| only.is_empty() || only.contains(&c.login))
                    .map(|c| c.id.clone())
                    .collect()
            })
            .unwrap_or_default()
    })
}

fn parse_outcomes(event: &Value) -> Vec<Outcome> {
    event
        .get("outcomes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|o| {
            Some(Outcome {
                id: o.get("id")?.as_str()?.to_string(),
                title: o
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                users: o.get("total_users").and_then(Value::as_u64).unwrap_or(0),
                points: o.get("total_points").and_then(Value::as_u64).unwrap_or(0),
                top_bet: o
                    .get("top_predictors")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|p| p.get("points").and_then(Value::as_u64))
                    .max()
                    .unwrap_or(0),
            })
        })
        .collect()
}

/// Tracks predictions from PubSub. Returns the result and net points of one of this
/// account's bets when it is settled.
fn on_message(
    ctx: &ModuleContext,
    s: &PredictionSettings,
    pending: &mut HashMap<String, Pending>,
    m: &PubSubMessage,
) -> Option<(String, i64)> {
    let account = ctx.account_id().to_string();
    match (m.topic_kind.as_str(), m.kind.as_str()) {
        ("predictions-channel-v1", "event-created" | "event-updated") => {
            let ev = m.data.get("event")?;
            let id = ev.get("id").and_then(Value::as_str)?;
            let status = ev.get("status").and_then(Value::as_str).unwrap_or("");
            let outcomes = parse_outcomes(ev);
            if let Some(p) = pending.get_mut(id) {
                p.outcomes = outcomes;
                p.open = status == "ACTIVE";
                return None;
            }
            if status != "ACTIVE" {
                return None;
            }
            let created = ev
                .get("created_at")
                .and_then(Value::as_str)
                .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or_else(Utc::now);
            let window = ev
                .get("prediction_window_seconds")
                .and_then(Value::as_u64)
                .unwrap_or(60);
            let closes = created + chrono::Duration::seconds(window as i64);
            let wait = (closes - Utc::now())
                .to_std()
                .unwrap_or_default()
                .saturating_sub(Duration::from_secs(s.seconds_before_close));
            pending.insert(
                id.to_string(),
                Pending {
                    channel_id: m.target_id.clone(),
                    title: ev
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    outcomes,
                    bet_at: Instant::now() + wait,
                    open: true,
                    placed: false,
                },
            );
            None
        }
        ("predictions-user-v1", "prediction-result") => {
            let p = m.data.get("prediction")?;
            let event_id = p
                .get("event_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let placed = p.get("points").and_then(Value::as_i64).unwrap_or(0);
            let r = p.get("result").cloned().unwrap_or(Value::Null);
            let kind = r
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("UNKNOWN")
                .to_string();
            let won = r.get("points_won").and_then(Value::as_i64).unwrap_or(0);
            let net = match kind.as_str() {
                "WIN" => won - placed,
                "LOSE" => -placed,
                _ => 0,
            };
            let channel_id = p
                .get("channel_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            ctx.emit(Event::PredictionResult {
                account,
                channel_id,
                event_id,
                result: kind.clone(),
                points_won: net,
            });
            Some((kind, net))
        }
        _ => None,
    }
}

/// Chooses an outcome according to the strategy.
pub fn choose(strategy: Strategy, outcomes: &[Outcome]) -> Option<&Outcome> {
    let most_voted = outcomes.iter().max_by_key(|o| o.users);
    let most_points = outcomes.iter().max_by_key(|o| o.points);
    let high_odds = outcomes
        .iter()
        .filter(|o| o.points > 0)
        .min_by_key(|o| o.points)
        .or(outcomes.first());
    match strategy {
        Strategy::MostVoted => most_voted,
        Strategy::MostPoints => most_points,
        Strategy::HighOdds => high_odds,
        Strategy::Smart => {
            let total: u64 = outcomes.iter().map(|o| o.users).sum();
            let top = most_voted?;
            let share = if total == 0 {
                0.0
            } else {
                top.users as f64 / total as f64
            };
            if share < 0.55 { high_odds } else { most_voted }
        }
    }
}

async fn place(ctx: &ModuleContext, s: &PredictionSettings, event_id: &str, p: &Pending) {
    if !wanted_channels(ctx, s).contains(&p.channel_id) {
        return;
    }
    let voters: u64 = p.outcomes.iter().map(|o| o.users).sum();
    if voters < s.min_voters {
        tracing::info!(title = %p.title, voters, "prediction skipped (too few voters)");
        return;
    }
    let balance = ctx
        .store()
        .read(|st| {
            st.accounts
                .get(ctx.account_id())
                .and_then(|a| a.channels.get(&p.channel_id))
                .and_then(|c| c.points)
        })
        .unwrap_or(0);
    if balance < s.min_balance {
        return;
    }
    let Some(outcome) = choose(s.strategy, &p.outcomes) else {
        return;
    };
    let pays = odds(outcome, &p.outcomes);
    if (s.min_odds > 0.0 && pays < s.min_odds) || (s.max_odds > 0.0 && pays > s.max_odds) {
        tracing::info!(title = %p.title, outcome = %outcome.title, odds = pays, "prediction skipped (odds out of range)");
        return;
    }
    let amount = ((balance as f64) * s.percent / 100.0).floor() as u64;
    let mut amount = amount.min(s.max_points).min(250_000);
    if s.stealth && outcome.top_bet > 0 {
        amount = amount.min(outcome.top_bet.saturating_sub(1));
    }
    if amount < 10 {
        return;
    }
    match ctx
        .gql()
        .make_prediction(event_id, &outcome.id, amount)
        .await
    {
        Ok(None) => {
            tracing::info!(title = %p.title, outcome = %outcome.title, amount, "prediction placed");
            ctx.emit(Event::PredictionPlaced {
                account: ctx.account_id().to_string(),
                channel_id: p.channel_id.clone(),
                event_id: event_id.to_string(),
                title: p.title.clone(),
                outcome: outcome.title.clone(),
                points: amount,
            });
        }
        Ok(Some(code)) => tracing::warn!(%code, "prediction rejected"),
        Err(e) => tracing::warn!(error = %e, "prediction failed"),
    }
}

/// Checks values serde accepts but the strategy cannot use (NaN and infinities can
/// reach here from a Rust caller even though JSON has no such numbers).
fn check_settings(s: &PredictionSettings) -> Result<(), String> {
    if ![s.percent, s.min_odds, s.max_odds]
        .iter()
        .all(|x| x.is_finite())
    {
        return Err("percent, min_odds and max_odds must be finite numbers".into());
    }
    if !(s.percent > 0.0 && s.percent <= 100.0) {
        return Err("percent must be above 0 and at most 100".into());
    }
    if !(1..=120).contains(&s.seconds_before_close) {
        return Err("seconds_before_close must be between 1 and 120".into());
    }
    if s.min_odds < 0.0 || s.max_odds < 0.0 {
        return Err("odds limits cannot be negative".into());
    }
    if s.max_odds > 0.0 && s.min_odds > s.max_odds {
        return Err("min_odds must not be above max_odds".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn o(id: &str, users: u64, points: u64) -> Outcome {
        Outcome {
            id: id.into(),
            title: id.into(),
            users,
            points,
            top_bet: 0,
        }
    }

    #[test]
    fn odds_and_summary() {
        let v = [o("a", 80, 1000), o("b", 20, 3000)];
        assert_eq!(odds(&v[0], &v), 4.0);
        assert!((odds(&v[1], &v) - 4.0 / 3.0).abs() < 1e-9);
        let mut s = Summary::default();
        s.add("WIN", 300);
        s.add("LOSE", -100);
        s.add("REFUND", 0);
        assert_eq!(
            (s.bets, s.wins, s.losses, s.refunds, s.net),
            (3, 1, 1, 1, 200)
        );
    }

    #[test]
    fn settings_must_be_finite() {
        let ok = PredictionSettings::default();
        assert!(check_settings(&ok).is_ok());
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut s = ok.clone();
            s.min_odds = bad;
            assert!(check_settings(&s).is_err());
            let mut s = ok.clone();
            s.max_odds = bad;
            assert!(check_settings(&s).is_err());
            let mut s = ok.clone();
            s.percent = bad;
            assert!(check_settings(&s).is_err());
        }
    }

    #[test]
    fn strategies() {
        let v = [o("a", 80, 1000), o("b", 20, 5000)];
        assert_eq!(choose(Strategy::MostVoted, &v).unwrap().id, "a");
        assert_eq!(choose(Strategy::MostPoints, &v).unwrap().id, "b");
        assert_eq!(choose(Strategy::HighOdds, &v).unwrap().id, "a");
        assert_eq!(choose(Strategy::Smart, &v).unwrap().id, "a");
        let close = [o("a", 52, 1000), o("b", 48, 9000)];
        assert_eq!(
            choose(Strategy::Smart, &close).unwrap().id,
            "a",
            "high odds = fewest points"
        );
    }
}
