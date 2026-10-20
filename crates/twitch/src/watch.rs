//! Simulated viewing of one live channel.
//!
//! Every ~20 s the watcher fetches the lowest-quality HLS media playlist and sends a
//! `HEAD` for the newest segment (what Twitch uses to credit drops), and once a
//! minute it posts a `minute-watched` event to spade (what credits channel points
//! and watch streaks). No video data is downloaded.

use std::time::Duration;

use base64::Engine;
use serde_json::json;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::error::{Error, Result};
use crate::gql::Gql;
use crate::models::StreamInfo;

const TICK: Duration = Duration::from_secs(20);
const SPADE_EVERY: Duration = Duration::from_secs(60);
const INFO_REFRESH: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone)]
pub struct WatchTarget {
    pub channel_id: String,
    pub login: String,
}

#[derive(Debug, Clone)]
pub enum WatchReport {
    /// One watch cycle completed. `minute` is true when a minute-watched was accepted.
    Tick { hls: bool, minute: bool },
    /// The stream ended; the watcher stops by itself.
    Offline,
    /// A cycle failed. The watcher keeps trying until cancelled.
    Error(String),
}

pub async fn run(
    gql: Gql,
    target: WatchTarget,
    reports: mpsc::Sender<WatchReport>,
    cancel: CancellationToken,
) {
    let mut w = Watcher {
        gql,
        target,
        media_url: None,
        info: None,
        info_at: None,
        last_spade: None,
    };
    let mut next = Instant::now();
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = tokio::time::sleep_until(next) => {}
        }
        let started = Instant::now();
        let report = match w.cycle().await {
            Ok(Some(r)) => r,
            Ok(None) => {
                let _ = reports.send(WatchReport::Offline).await;
                return;
            }
            Err(e) => WatchReport::Error(e.to_string()),
        };
        if reports.send(report).await.is_err() {
            return;
        }
        let jitter = Duration::from_millis(rand::random::<u64>() % 2000);
        next = started + TICK + jitter;
    }
}

struct Watcher {
    gql: Gql,
    target: WatchTarget,
    media_url: Option<String>,
    info: Option<StreamInfo>,
    info_at: Option<Instant>,
    last_spade: Option<Instant>,
}

impl Watcher {
    /// Returns `Ok(None)` when the channel is offline.
    async fn cycle(&mut self) -> Result<Option<WatchReport>> {
        if self.info_at.is_none_or(|t| t.elapsed() >= INFO_REFRESH) {
            let user = self
                .gql
                .users_by_id(std::slice::from_ref(&self.target.channel_id))
                .await?;
            match user.into_iter().next().flatten().and_then(|u| u.stream) {
                Some(stream) => {
                    if self.info.as_ref().is_some_and(|i| i.id != stream.id) {
                        self.media_url = None; // new broadcast
                    }
                    self.info = Some(stream);
                    self.info_at = Some(Instant::now());
                }
                None => return Ok(None),
            }
        }

        let hls = match self.hls_tick().await {
            Ok(ok) => ok,
            Err(Error::Http { status, .. }) if status == 404 || status == 403 => {
                self.media_url = None;
                self.info_at = None; // re-check online status next cycle
                false
            }
            Err(e) => {
                self.media_url = None;
                return Err(e);
            }
        };

        let mut minute = false;
        if self.last_spade.is_none_or(|t| t.elapsed() >= SPADE_EVERY) {
            match self.send_minute().await {
                Ok(ok) => {
                    minute = ok;
                    self.last_spade = Some(Instant::now());
                }
                Err(e) => tracing::debug!(channel = %self.target.login, error = %e, "spade failed"),
            }
        }
        Ok(Some(WatchReport::Tick { hls, minute }))
    }

    async fn hls_tick(&mut self) -> Result<bool> {
        let http = self.gql.net().http.clone();
        let ua = self.gql.user_agent();
        if self.media_url.is_none() {
            self.media_url = Some(self.resolve_media_url().await?);
        }
        let url = self.media_url.clone().unwrap_or_default();
        let resp = http.get(&url).header("User-Agent", &ua).send().await?;
        let status = resp.status().as_u16();
        if status >= 400 {
            return Err(Error::Http {
                status,
                url: "media playlist".into(),
            });
        }
        let body = resp.text().await?;
        let Some(segment) = body
            .lines()
            .rev()
            .map(str::trim)
            .find(|l| l.starts_with("http"))
        else {
            return Ok(false);
        };
        let head = http.head(segment).header("User-Agent", &ua).send().await?;
        Ok(head.status().is_success())
    }

    async fn resolve_media_url(&self) -> Result<String> {
        let (token, sig) = self.gql.playback_token(&self.target.login).await?;
        let mut url = url::Url::parse(&format!(
            "https://usher.ttvnw.net/api/channel/hls/{}.m3u8",
            self.target.login
        ))
        .map_err(|e| Error::decode(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("sig", &sig)
            .append_pair("token", &token)
            .append_pair("allow_source", "true")
            .append_pair("fast_bread", "true")
            .append_pair(
                "p",
                &(rand::random::<u32>() % 9_000_000 + 1_000_000).to_string(),
            );
        let resp = self
            .gql
            .net()
            .http
            .get(url)
            .header("User-Agent", self.gql.user_agent())
            .send()
            .await?;
        let status = resp.status().as_u16();
        if status >= 400 {
            return Err(Error::Http {
                status,
                url: "usher".into(),
            });
        }
        let master = resp.text().await?;
        lowest_variant(&master).ok_or_else(|| Error::decode("no variant in master playlist"))
    }

    async fn send_minute(&self) -> Result<bool> {
        let Some(info) = &self.info else {
            return Ok(false);
        };
        let creds = self.gql.credentials();
        let game = info.game.as_ref();
        let payload = json!([{
            "event": "minute-watched",
            "properties": {
                "broadcast_id": info.id,
                "channel_id": self.target.channel_id,
                "channel": self.target.login,
                "client_time": chrono::Utc::now().timestamp_millis() as f64 / 1000.0,
                "game": game.map(|g| g.name.as_str()).unwrap_or(""),
                "game_id": game.map(|g| g.id.as_str()).unwrap_or(""),
                "hidden": false,
                "is_live": true,
                "live": true,
                "logged_in": true,
                "minutes_logged": 1,
                "muted": false,
                "player": "site",
                "user_id": creds.user_id,
            }
        }]);
        let data = base64::engine::general_purpose::STANDARD.encode(payload.to_string());
        let spade = self.gql.manifest().load().spade_url.clone();
        let resp = self
            .gql
            .net()
            .http
            .post(spade)
            .header("User-Agent", self.gql.user_agent())
            .form(&[("data", data)])
            .send()
            .await?;
        Ok(resp.status().as_u16() == 204 || resp.status().is_success())
    }
}

/// Picks the variant with the lowest `BANDWIDTH` from an HLS master playlist.
pub fn lowest_variant(master: &str) -> Option<String> {
    let mut best: Option<(u64, String)> = None;
    let mut pending_bw: Option<u64> = None;
    for line in master.lines().map(str::trim) {
        if let Some(attrs) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            pending_bw = attrs
                .split(',')
                .find_map(|kv| kv.strip_prefix("BANDWIDTH="))
                .and_then(|v| v.parse().ok())
                .or(Some(u64::MAX - 1));
        } else if line.starts_with("http")
            && let Some(bw) = pending_bw.take()
            && best.as_ref().is_none_or(|(b, _)| bw < *b)
        {
            best = Some((bw, line.to_string()));
        }
    }
    best.map(|(_, u)| u)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_lowest_bandwidth() {
        let m = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=6000000,RESOLUTION=1920x1080\nhttps://a/1080\n\
                 #EXT-X-STREAM-INF:BANDWIDTH=160000,RESOLUTION=284x160\nhttps://a/160\n\
                 #EXT-X-STREAM-INF:BANDWIDTH=3400000,RESOLUTION=1280x720\nhttps://a/720\n";
        assert_eq!(lowest_variant(m).as_deref(), Some("https://a/160"));
    }
}
