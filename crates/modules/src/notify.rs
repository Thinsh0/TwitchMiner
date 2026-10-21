//! Notifications: forwards selected events to Discord, ntfy, Telegram or any webhook.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tm_core::event::{Envelope, Event};
use tm_core::state::{AccountStatus, DropAlertKind};
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
    /// POSTs `{ "text": …, "event": <envelope> }`. Also the way to wake an AI agent.
    Webhook {
        url: String,
        /// Sent as `Authorization: Bearer <token>`.
        #[serde(default)]
        token: Option<String>,
        #[serde(default)]
        headers: BTreeMap<String, String>,
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
    pub language: Language,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    Fr,
    En,
}

impl Default for NotifySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            targets: Vec::new(),
            events: vec![
                "drop_claimed".into(),
                "drop_alert".into(),
                "auth_required".into(),
                "prediction_result".into(),
                "notice_raised".into(),
            ],
            min_points: 0,
            language: Language::Fr,
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

    /// Keeps running while the account waits for a new login, to report it.
    fn needs_login(&self) -> bool {
        false
    }

    async fn run(&self, ctx: ModuleContext) -> anyhow::Result<()> {
        let mut rx = ctx.events();
        let mut settings: NotifySettings = ctx.settings()?;
        let http = ctx.gql().net().http.clone();
        // An account whose token was rejected at start-up is already marked before
        // this module subscribes: report it once now.
        let (seq, auth_required) = ctx.store().read(|s| {
            let status = s
                .accounts
                .get(ctx.account_id())
                .and_then(|a| a.status.as_ref());
            (s.seq, status == Some(&AccountStatus::AuthRequired))
        });
        if auth_required && !settings.targets.is_empty() {
            let env = Envelope {
                seq,
                ts: chrono::Utc::now(),
                event: Event::AccountStatusChanged {
                    account: ctx.account_id().to_string(),
                    status: AccountStatus::AuthRequired,
                },
            };
            if let Some(text) = format_event(&ctx, &settings, &env) {
                send_all(&http, &settings.targets, &text, &json!(env)).await;
            }
        }
        loop {
            let env = tokio::select! {
                _ = ctx.cancelled() => return Ok(()),
                _ = ctx.settings_changed() => { settings = ctx.settings()?; continue; }
                call = ctx.next_call() => {
                    let Some(call) = call else { return Ok(()) };
                    let reply = if call.name == "test" {
                        let text = match settings.language {
                            Language::Fr => format!("Notification de test TwitchMiner pour {}", ctx.login()),
                            Language::En => format!("TwitchMiner test notification for {}", ctx.login()),
                        };
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

/// Engine-wide events are sent by one account only, to avoid duplicates: the first
/// one whose notifications run (running, or waiting for a new login).
fn concerns_me(ctx: &ModuleContext, env: &Envelope) -> bool {
    match env.event.account() {
        Some(a) => a == ctx.account_id(),
        None => ctx.store().read(|s| {
            s.accounts
                .iter()
                .find(|(_, a)| {
                    matches!(
                        a.status,
                        Some(AccountStatus::Running | AccountStatus::AuthRequired)
                    )
                })
                .is_some_and(|(k, _)| k == ctx.account_id())
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
    let fr = s.language == Language::Fr;
    let who = ctx.login();
    let text = match &env.event {
        Event::DropClaimed {
            drop_name, game, ..
        } if wants("drop_claimed") => {
            if fr {
                format!("🎁 [{who}] Drop récupéré : {drop_name} ({game})")
            } else {
                format!("🎁 [{who}] Drop claimed: {drop_name} ({game})")
            }
        }
        Event::DropAlert { alert: a, .. } if wants("drop_alert") => {
            let game = &a.game;
            let ends = a
                .ends_at
                .with_timezone(&chrono::Local)
                .format("%d/%m %H:%M");
            let hours = |m: u32| format!("{:.1}", f64::from(m) / 60.0);
            let rewards = if a.rewards.is_empty() {
                String::new()
            } else {
                format!(" ({})", a.rewards.join(", "))
            };
            match (a.kind, fr) {
                (DropAlertKind::AtRisk, true) => format!(
                    "⏳ [{who}] Drops {game} à risque : « {} » finit le {ends}, peu de marge{rewards}",
                    a.campaign
                ),
                (DropAlertKind::AtRisk, false) => format!(
                    "⏳ [{who}] {game} drops at risk: \"{}\" ends {ends}, little margin{rewards}",
                    a.campaign
                ),
                (DropAlertKind::WillMiss, true) => format!(
                    "❌ [{who}] Drops {game} : « {} » ne pourra pas être fini avant le {ends} (il manque ~{} h){rewards}",
                    a.campaign,
                    hours(a.missing_minutes.unwrap_or(0))
                ),
                (DropAlertKind::WillMiss, false) => format!(
                    "❌ [{who}] {game} drops: \"{}\" cannot be finished before {ends} (~{} h short){rewards}",
                    a.campaign,
                    hours(a.missing_minutes.unwrap_or(0))
                ),
                (DropAlertKind::NotLinked, true) => format!(
                    "🔗 [{who}] Lie ton compte {game} pour « {} » (fin le {ends}) : {}",
                    a.campaign,
                    a.link_url
                        .as_deref()
                        .unwrap_or("https://www.twitch.tv/drops/campaigns")
                ),
                (DropAlertKind::NotLinked, false) => format!(
                    "🔗 [{who}] Link your {game} account for \"{}\" (ends {ends}): {}",
                    a.campaign,
                    a.link_url
                        .as_deref()
                        .unwrap_or("https://www.twitch.tv/drops/campaigns")
                ),
                (DropAlertKind::NewCampaign, true) => format!(
                    "🆕 [{who}] Nouvelle campagne {game} : « {} », jusqu'au {ends}{rewards}",
                    a.campaign
                ),
                (DropAlertKind::NewCampaign, false) => format!(
                    "🆕 [{who}] New {game} campaign: \"{}\", until {ends}{rewards}",
                    a.campaign
                ),
            }
        }
        Event::AccountStatusChanged {
            status: AccountStatus::AuthRequired,
            ..
        } if wants("auth_required") || wants("account_status_changed") => {
            if fr {
                format!("🔑 [{who}] Connexion expirée, reconnectez-vous")
            } else {
                format!("🔑 [{who}] Login expired, please sign in again")
            }
        }
        Event::AccountStatusChanged { status, .. } if wants("account_status_changed") => {
            if fr {
                format!("ℹ️ [{who}] Statut du compte : {status:?}")
            } else {
                format!("ℹ️ [{who}] Account status: {status:?}")
            }
        }
        Event::PointsEarned {
            channel_id,
            amount,
            reason,
            ..
        } if wants("points_earned") && *amount >= s.min_points => {
            let ch = channel_name(ctx, channel_id);
            if fr {
                format!("💰 [{who}] +{amount} sur {ch} ({reason})")
            } else {
                format!("💰 [{who}] +{amount} on {ch} ({reason})")
            }
        }
        Event::RaidJoined {
            channel_id,
            target_login,
            ..
        } if wants("raid_joined") => {
            let ch = channel_name(ctx, channel_id);
            if fr {
                format!("⚔️ [{who}] Raid de {ch} vers {target_login}")
            } else {
                format!("⚔️ [{who}] Raid from {ch} to {target_login}")
            }
        }
        Event::PredictionPlaced {
            title,
            outcome,
            points,
            ..
        } if wants("prediction_placed") => {
            if fr {
                format!("🔮 [{who}] Pari de {points} sur « {outcome} » — {title}")
            } else {
                format!("🔮 [{who}] Bet {points} on \"{outcome}\" — {title}")
            }
        }
        Event::PredictionResult {
            result,
            points_won,
            channel_id,
            ..
        } if wants("prediction_result") => {
            let ch = channel_name(ctx, channel_id);
            if fr {
                let r = match result.as_str() {
                    "WIN" => "gagnée",
                    "LOSE" => "perdue",
                    "REFUND" => "remboursée",
                    other => other,
                };
                format!("🔮 [{who}] Prédiction {r} sur {ch} : {points_won:+}")
            } else {
                format!("🔮 [{who}] Prediction {result} on {ch}: {points_won:+}")
            }
        }
        Event::NoticeRaised { notice } if wants("notice_raised") => {
            format!("⚠️ {}", notice.message)
        }
        Event::WatchStarted {
            channel_id, reason, ..
        } if wants("watch_started") => {
            let ch = channel_name(ctx, channel_id);
            if fr {
                format!("▶️ [{who}] Visionnage de {ch} ({reason})")
            } else {
                format!("▶️ [{who}] Watching {ch} ({reason})")
            }
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
        Target::Webhook {
            url,
            token,
            headers,
        } => {
            let mut r = http
                .post(url)
                .json(&json!({ "text": text, "event": event }));
            if let Some(tok) = token {
                r = r.bearer_auth(tok);
            }
            for (k, v) in headers {
                r = r.header(k, v);
            }
            r
        }
    };
    // Errors without their URL: webhook and bot URLs carry the target's secret.
    req.timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(reqwest::Error::without_url)?
        .error_for_status()
        .map_err(reqwest::Error::without_url)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn errors_never_show_the_target_url() {
        tm_twitch::tls::install_crypto_provider();
        // Nothing listens on port 9: the request fails at once.
        let targets = [Target::Telegram {
            bot_token: "123:SECRET".into(),
            chat_id: "1".into(),
        }];
        let http = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all("http://127.0.0.1:9").unwrap())
            .build()
            .unwrap();
        let errors = send_all(&http, &targets, "hi", &json!({})).await;
        assert_eq!(errors.len(), 1);
        assert!(!errors[0].contains("SECRET"), "{}", errors[0]);
    }
}
