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
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub id: String,
    pub title: String,
    pub users: u64,
    pub points: u64,
}

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
        if !(0.0..=100.0).contains(&s.percent) {
            return Err("percent must be between 0 and 100".into());
        }
        Ok(())
    }

    async fn run(&self, ctx: ModuleContext) -> anyhow::Result<()> {
        let uid = ctx.account_id().to_string();
        let mut settings: PredictionSettings = ctx.settings()?;
        let mut topics = ctx.pubsub().topic_set();
        let mut ps = ctx.pubsub().subscribe();
        let mut pending: HashMap<String, Pending> = HashMap::new();
        let mut refresh = tokio::time::interval(Duration::from_secs(120));

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
                    Ok(PubSubEvent::Message(m)) => on_message(&ctx, &settings, &mut pending, &m),
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => return Ok(()),
                },
                call = ctx.next_call() => {
                    let Some(call) = call else { return Ok(()) };
                    let _ = call.reply.send(Ok(json!({ "pending": pending.len() })));
                }
            }
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
            })
        })
        .collect()
}

fn on_message(
    ctx: &ModuleContext,
    s: &PredictionSettings,
    pending: &mut HashMap<String, Pending>,
    m: &PubSubMessage,
) {
    let account = ctx.account_id().to_string();
    match (m.topic_kind.as_str(), m.kind.as_str()) {
        ("predictions-channel-v1", "event-created" | "event-updated") => {
            let Some(ev) = m.data.get("event") else {
                return;
            };
            let Some(id) = ev.get("id").and_then(Value::as_str) else {
                return;
            };
            let status = ev.get("status").and_then(Value::as_str).unwrap_or("");
            let outcomes = parse_outcomes(ev);
            if let Some(p) = pending.get_mut(id) {
                p.outcomes = outcomes;
                p.open = status == "ACTIVE";
                return;
            }
            if status != "ACTIVE" {
                return;
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
        }
        ("predictions-user-v1", "prediction-result") => {
            let Some(p) = m.data.get("prediction") else {
                return;
            };
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
                result: kind,
                points_won: net,
            });
        }
        _ => {}
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
    let amount = ((balance as f64) * s.percent / 100.0).floor() as u64;
    let amount = amount.min(s.max_points).min(250_000);
    if amount < 10 {
        return;
    }
    let Some(outcome) = choose(s.strategy, &p.outcomes) else {
        return;
    };
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

#[cfg(test)]
mod tests {
    use super::*;

    fn o(id: &str, users: u64, points: u64) -> Outcome {
        Outcome {
            id: id.into(),
            title: id.into(),
            users,
            points,
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
