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
    /// Send every message as urgent (ntfy priority `high`: delivered at once and shown
    /// as a pop-up). Otherwise only what needs acting soon is (see [`urgent`]), plus
    /// tests and the confirmation of a new destination.
    pub all_urgent: bool,
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
            all_urgent: false,
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
        // Sending happens apart, in order: a slow or rate-limited service never holds
        // up the events (the queue ends when this module stops).
        let (queue, mut pending) = tokio::sync::mpsc::channel::<Outgoing>(QUEUE);
        let sender_http = http.clone();
        tokio::spawn(async move {
            while let Some(o) = pending.recv().await {
                send_all(&sender_http, &o.targets, &o.text, &o.event, o.urgent).await;
            }
        });
        let enqueue = |targets: &[Target], text: String, event: Value, urgent: bool| {
            let o = Outgoing {
                targets: targets.to_vec(),
                text,
                event,
                urgent,
            };
            if queue.try_send(o).is_err() {
                tracing::warn!("notification dropped: too many waiting");
            }
        };
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
                enqueue(&settings.targets, text, json!(env), true);
            }
        }
        loop {
            let env = tokio::select! {
                _ = ctx.cancelled() => return Ok(()),
                _ = ctx.settings_changed() => {
                    let before = std::mem::take(&mut settings.targets);
                    settings = ctx.settings()?;
                    // A destination just added gets a confirmation: the user knows at once
                    // that it works.
                    let added: Vec<Target> = settings
                        .targets
                        .iter()
                        .filter(|t| !before.contains(t))
                        .cloned()
                        .collect();
                    if !added.is_empty() {
                        let text = match settings.language {
                            Language::Fr => format!("✅ TwitchMiner est bien relié : les notifications de {} arriveront ici.", ctx.login()),
                            Language::En => format!("✅ TwitchMiner is connected: notifications for {} will arrive here.", ctx.login()),
                        };
                        enqueue(&added, text, json!({"type": "connected"}), true);
                    }
                    continue;
                }
                call = ctx.next_call() => {
                    let Some(call) = call else { return Ok(()) };
                    let reply = if call.name == "test" {
                        let text = match settings.language {
                            Language::Fr => format!("Notification de test TwitchMiner pour {}", ctx.login()),
                            Language::En => format!("TwitchMiner test notification for {}", ctx.login()),
                        };
                        let errors = send_all(&http, &settings.targets, &text, &json!({"type": "test"}), true).await;
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
            let urgent = settings.all_urgent || urgent(&env.event);
            enqueue(&settings.targets, text, json!(env.as_ref()), urgent);
        }
    }
}

/// Notifications waiting to be sent, at most.
const QUEUE: usize = 200;
/// Attempts per target and notification.
const ATTEMPTS: u32 = 3;
/// Longest wait a rate limit may impose before a retry.
const MAX_RETRY_WAIT: Duration = Duration::from_secs(60);

struct Outgoing {
    targets: Vec<Target>,
    text: String,
    event: Value,
    urgent: bool,
}

/// Events that need the user soon: sent with a high priority.
fn urgent(event: &Event) -> bool {
    match event {
        Event::AccountStatusChanged { status, .. } => *status == AccountStatus::AuthRequired,
        Event::DropAlert { alert, .. } => matches!(
            alert.kind,
            DropAlertKind::AtRisk | DropAlertKind::WillMiss | DropAlertKind::NotLinked
        ),
        _ => false,
    }
}

/// A failed send, and when it is worth trying again (`None`: it is not).
struct SendError {
    error: anyhow::Error,
    retry_after: Option<Duration>,
}

impl SendError {
    fn fatal(error: impl Into<anyhow::Error>) -> Self {
        Self {
            error: error.into(),
            retry_after: None,
        }
    }
}

/// How long a rate-limited service asks to wait: `Retry-After` (seconds), or the
/// `retry_after` of Discord's and Telegram's answers. Five seconds when unsaid.
fn retry_wait(headers: &reqwest::header::HeaderMap, body: &str) -> Duration {
    let from_header = headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<f64>().ok());
    let from_body = serde_json::from_str::<Value>(body).ok().and_then(|v| {
        v["retry_after"]
            .as_f64()
            .or_else(|| v["parameters"]["retry_after"].as_f64())
    });
    let secs = from_header.or(from_body).unwrap_or(5.0).max(0.0);
    Duration::from_secs_f64(secs).min(MAX_RETRY_WAIT)
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
    urgent: bool,
) -> Vec<String> {
    let mut errors = Vec::new();
    for t in targets {
        if let Err(e) = send_retrying(http, t, text, event, urgent).await {
            let msg = format!("{e:#}");
            tracing::warn!(error = %msg, "notification failed");
            errors.push(msg);
        }
    }
    errors
}

/// Sends to one target: a rate limit (429) waits what the service asks, a network or
/// server error waits a little; `ATTEMPTS` tries in all.
async fn send_retrying(
    http: &reqwest::Client,
    t: &Target,
    text: &str,
    event: &Value,
    urgent: bool,
) -> anyhow::Result<()> {
    let mut attempt = 1;
    loop {
        match send(http, t, text, event, urgent).await {
            Ok(()) => return Ok(()),
            Err(SendError {
                retry_after: Some(wait),
                ..
            }) if attempt < ATTEMPTS => {
                tokio::time::sleep(wait).await;
                attempt += 1;
            }
            Err(e) => return Err(e.error),
        }
    }
}

async fn send(
    http: &reqwest::Client,
    t: &Target,
    text: &str,
    event: &Value,
    urgent: bool,
) -> Result<(), SendError> {
    let req = match t {
        Target::Discord { webhook_url } => http.post(webhook_url).json(&json!({ "content": text })),
        Target::Ntfy { url, token } => {
            let mut r = http
                .post(url)
                .header("Title", "TwitchMiner")
                .body(text.to_string());
            // ntfy.sh asks Firebase for an immediate delivery from priority 4 (`high`) on;
            // the app also shows it as a pop-up.
            if urgent {
                r = r.header("Priority", "high");
            }
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
    let resp = req
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| SendError {
            error: e.without_url().into(),
            retry_after: Some(Duration::from_secs(3)),
        })?;
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    let retry_after = if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let headers = resp.headers().clone();
        let body = resp.text().await.unwrap_or_default();
        Some(retry_wait(&headers, &body))
    } else if status.is_server_error() {
        Some(Duration::from_secs(3))
    } else {
        return Err(SendError::fatal(anyhow::anyhow!("HTTP {status}")));
    };
    Err(SendError {
        error: anyhow::anyhow!("HTTP {status}"),
        retry_after,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn only_what_needs_acting_is_urgent() {
        let status = |status| Event::AccountStatusChanged {
            account: "1".into(),
            status,
        };
        assert!(urgent(&status(AccountStatus::AuthRequired)));
        assert!(!urgent(&status(AccountStatus::Running)));
        assert!(!urgent(&Event::DropClaimed {
            account: "1".into(),
            drop_id: "d".into(),
            campaign_id: "c".into(),
            drop_name: "Skin".into(),
            game: "Rust".into(),
        }));
    }

    #[test]
    fn rate_limit_waits_follow_the_service() {
        let mut h = reqwest::header::HeaderMap::new();
        assert_eq!(retry_wait(&h, ""), Duration::from_secs(5));
        assert_eq!(
            retry_wait(&h, r#"{"retry_after": 1.5}"#),
            Duration::from_millis(1500)
        );
        assert_eq!(
            retry_wait(&h, r#"{"parameters": {"retry_after": 2}}"#),
            Duration::from_secs(2)
        );
        h.insert(reqwest::header::RETRY_AFTER, "7".parse().unwrap());
        assert_eq!(retry_wait(&h, ""), Duration::from_secs(7));
        h.insert(reqwest::header::RETRY_AFTER, "3600".parse().unwrap());
        assert_eq!(retry_wait(&h, ""), MAX_RETRY_WAIT);
    }

    #[tokio::test]
    async fn a_rate_limited_send_is_retried() {
        tm_twitch::tls::install_crypto_provider();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/hook", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let answers = [
                "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                "HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            ];
            for a in answers {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 4096];
                let _ = sock.read(&mut buf).await;
                sock.write_all(a.as_bytes()).await.unwrap();
            }
        });
        let targets = [Target::Webhook {
            url,
            token: None,
            headers: BTreeMap::new(),
        }];
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let errors = send_all(&http, &targets, "hi", &json!({}), false).await;
        assert!(errors.is_empty(), "{errors:?}");
        server.await.unwrap();
    }

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
        let errors = send_all(&http, &targets, "hi", &json!({}), false).await;
        assert_eq!(errors.len(), 1);
        assert!(!errors[0].contains("SECRET"), "{}", errors[0]);
    }
}
