//! Notifications: forwards selected events to Discord, ntfy, Telegram or any webhook.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tm_core::event::{Envelope, Event};
use tm_core::state::AccountStatus;
use tm_core::{Module, ModuleContext, ModuleInfo};

pub struct Notify;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    Discord {
        webhook_url: String,
    },
    Ntfy {
        url: String,
        #[serde(default)]
        token: Option<String>,
    },
    Telegram {
        bot_token: String,
        chat_id: String,
    },
    /// POSTs `{ "text": …, "event": <envelope> }`.
    Webhook {
        url: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
#[serde(default)]
pub struct NotifySettings {
    pub enabled: bool,
    pub targets: Vec<Target>,
    /// Event kinds to send. See `Event::kind()`; `auth_required` is a shortcut for
    /// account status changes that need a new login.
    pub events: Vec<String>,
    /// Only notify points gains of at least this amount (for `points_earned`).
    pub min_points: i64,
}

impl Default for NotifySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            targets: Vec::new(),
            events: vec![
                "drop_claimed".into(),
                "auth_required".into(),
                "prediction_result".into(),
                "notice_raised".into(),
            ],
            min_points: 0,
        }
    }
}

#[async_trait::async_trait]
impl Module for Notify {
    fn info(&self) -> ModuleInfo {
        ModuleInfo::new(
            "notify",
            "Notifications",
            "Sends drops, login problems and other events to Discord, ntfy, Telegram or a webhook.",
            json!(NotifySettings::default()),
        )
        .with_schema::<NotifySettings>()
    }

    fn validate_settings(&self, v: &Value) -> Result<(), String> {
        tm_core::module::validate_as::<NotifySettings>(v)
    }

    async fn run(&self, ctx: ModuleContext) -> anyhow::Result<()> {
        let mut rx = ctx.events();
        let mut settings: NotifySettings = ctx.settings()?;
        let http = ctx.gql().net().http.clone();
        loop {
            let env = tokio::select! {
                _ = ctx.cancelled() => return Ok(()),
                _ = ctx.settings_changed() => { settings = ctx.settings()?; continue; }
                call = ctx.next_call() => {
                    let Some(call) = call else { return Ok(()) };
                    let reply = if call.name == "test" {
                        let text = format!("TwitchMiner test notification for {}", ctx.login());
                        let errors = send_all(&http, &settings.targets, &text, &json!({"type": "test"})).await;
                        Ok(json!({"sent": settings.targets.len() - errors.len(), "errors": errors}))
                    } else {
                        Err(format!("unknown call `{}`", call.name))
                    };
                    let _ = call.reply.send(reply);
                    continue;
                }
                r = rx.recv() => match r {
                    Ok(e) => e,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => return Ok(()),
                },
            };
            if settings.targets.is_empty() || !concerns_me(&ctx, &env) {
                continue;
            }
            let Some(text) = format_event(&ctx, &settings, &env) else {
                continue;
            };
            send_all(&http, &settings.targets, &text, &json!(env.as_ref())).await;
        }
    }
}

/// Engine-wide events are sent by the first account only, to avoid duplicates.
fn concerns_me(ctx: &ModuleContext, env: &Envelope) -> bool {
    match env.event.account() {
        Some(a) => a == ctx.account_id(),
        None => ctx.store().read(|s| {
            s.accounts
                .keys()
                .next()
                .is_some_and(|k| k == ctx.account_id())
        }),
    }
}

fn channel_name(ctx: &ModuleContext, id: &str) -> String {
    ctx.store()
        .read(|s| {
            s.accounts
                .get(ctx.account_id())
                .and_then(|a| a.channels.get(id))
                .map(|c| c.display_name.clone())
        })
        .unwrap_or_else(|| id.to_string())
}

fn format_event(ctx: &ModuleContext, s: &NotifySettings, env: &Envelope) -> Option<String> {
    let wants = |k: &str| s.events.iter().any(|e| e == k);
    let who = ctx.login();
    let text = match &env.event {
        Event::DropClaimed {
            drop_name, game, ..
        } if wants("drop_claimed") => {
            format!("🎁 [{who}] Drop claimed: {drop_name} ({game})")
        }
        Event::AccountStatusChanged {
            status: AccountStatus::AuthRequired,
            ..
        } if wants("auth_required") || wants("account_status_changed") => {
            format!("🔑 [{who}] Login expired, please sign in again")
        }
        Event::AccountStatusChanged { status, .. } if wants("account_status_changed") => {
            format!("ℹ️ [{who}] Account status: {status:?}")
        }
        Event::PointsEarned {
            channel_id,
            amount,
            reason,
            ..
        } if wants("points_earned") && *amount >= s.min_points => {
            format!(
                "💰 [{who}] +{amount} on {} ({reason})",
                channel_name(ctx, channel_id)
            )
        }
        Event::RaidJoined {
            channel_id,
            target_login,
            ..
        } if wants("raid_joined") => {
            format!(
                "⚔️ [{who}] Raid from {} to {target_login}",
                channel_name(ctx, channel_id)
            )
        }
        Event::PredictionPlaced {
            title,
            outcome,
            points,
            ..
        } if wants("prediction_placed") => {
            format!("🔮 [{who}] Bet {points} on \"{outcome}\" — {title}")
        }
        Event::PredictionResult {
            result,
            points_won,
            channel_id,
            ..
        } if wants("prediction_result") => {
            format!(
                "🔮 [{who}] Prediction {result} on {}: {points_won:+}",
                channel_name(ctx, channel_id)
            )
        }
        Event::NoticeRaised { notice } if wants("notice_raised") => {
            format!("⚠️ {}", notice.message)
        }
        Event::WatchStarted {
            channel_id, reason, ..
        } if wants("watch_started") => {
            format!(
                "▶️ [{who}] Watching {} ({reason})",
                channel_name(ctx, channel_id)
            )
        }
        _ => return None,
    };
    Some(text)
}

/// Sends to every target; returns one error message per failed target.
async fn send_all(
    http: &reqwest::Client,
    targets: &[Target],
    text: &str,
    event: &Value,
) -> Vec<String> {
    let mut errors = Vec::new();
    for t in targets {
        if let Err(e) = send(http, t, text, event).await {
            let msg = format!("{e:#}");
            tracing::warn!(error = %msg, "notification failed");
            errors.push(msg);
        }
    }
    errors
}

async fn send(http: &reqwest::Client, t: &Target, text: &str, event: &Value) -> anyhow::Result<()> {
    let req = match t {
        Target::Discord { webhook_url } => http.post(webhook_url).json(&json!({ "content": text })),
        Target::Ntfy { url, token } => {
            let mut r = http
                .post(url)
                .header("Title", "TwitchMiner")
                .body(text.to_string());
            if let Some(tok) = token {
                r = r.bearer_auth(tok);
            }
            r
        }
        Target::Telegram { bot_token, chat_id } => http
            .post(format!(
                "https://api.telegram.org/bot{bot_token}/sendMessage"
            ))
            .json(&json!({ "chat_id": chat_id, "text": text })),
        Target::Webhook { url } => http
            .post(url)
            .json(&json!({ "text": text, "event": event })),
    };
    req.timeout(Duration::from_secs(15))
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}
