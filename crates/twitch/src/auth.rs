//! OAuth device-code login, token validation and refresh.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::manifest::ClientProfile;
use crate::net::Net;

const DEVICE_URL: &str = "https://id.twitch.tv/oauth2/device";
const TOKEN_URL: &str = "https://id.twitch.tv/oauth2/token";
const VALIDATE_URL: &str = "https://id.twitch.tv/oauth2/validate";

/// Scopes requested at login. Most features work with none; these mirror the
/// official TV app and keep follow management possible.
pub const SCOPES: &str =
    "channel_read chat:read user_blocks_edit user_blocks_read user_follows_edit user_read";

/// Everything needed to act as a user. Persisted by the core.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Credentials {
    pub user_id: String,
    pub login: String,
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// Manifest profile the token was issued for (tokens are bound to a client id).
    pub profile: String,
    /// Stable per-account device id, sent as `X-Device-Id`.
    pub device_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    /// The activation page with the code already filled in.
    #[serde(default)]
    pub verification_uri_complete: String,
    pub interval: u64,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// Seconds until expiry (`None`/0 = does not expire).
    #[serde(default)]
    pub expires_in: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Validation {
    pub client_id: String,
    pub login: String,
    pub user_id: String,
    /// Seconds; `0` means the token does not expire.
    #[serde(default)]
    pub expires_in: u64,
    #[serde(default)]
    pub scopes: Vec<String>,
}

pub enum PollOutcome {
    Pending,
    SlowDown,
    Done(TokenResponse),
}

pub async fn start_device_flow(net: &Net, profile: &ClientProfile) -> Result<DeviceCode> {
    #[derive(Deserialize)]
    struct Raw {
        device_code: String,
        user_code: String,
        verification_uri: String,
        interval: u64,
        expires_in: i64,
    }
    let raw: Raw = net
        .http
        .post(DEVICE_URL)
        .header("Client-Id", &profile.client_id)
        .header("User-Agent", &profile.user_agents[0])
        .form(&[
            ("client_id", profile.client_id.as_str()),
            ("scopes", SCOPES),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(DeviceCode {
        verification_uri_complete: complete_uri(&raw.verification_uri, &raw.user_code),
        device_code: raw.device_code,
        user_code: raw.user_code,
        verification_uri: raw.verification_uri,
        interval: raw.interval.max(1),
        expires_at: Utc::now() + chrono::Duration::seconds(raw.expires_in),
    })
}

/// Adds the `device-code` parameter, which makes Twitch's activation page pre-fill the
/// code (Twitch sometimes returns the page without it).
pub fn complete_uri(uri: &str, user_code: &str) -> String {
    if uri.contains("device-code=") || !user_code.chars().all(|c| c.is_ascii_alphanumeric()) {
        return uri.to_string();
    }
    let sep = if uri.contains('?') { '&' } else { '?' };
    format!("{uri}{sep}device-code={user_code}")
}

pub async fn poll_device_flow(
    net: &Net,
    profile: &ClientProfile,
    code: &DeviceCode,
) -> Result<PollOutcome> {
    let resp = net
        .http
        .post(TOKEN_URL)
        .header("User-Agent", &profile.user_agents[0])
        .form(&[
            ("client_id", profile.client_id.as_str()),
            ("device_code", code.device_code.as_str()),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ])
        .send()
        .await?;
    if resp.status().is_success() {
        return Ok(PollOutcome::Done(resp.json().await?));
    }
    let body = resp.text().await.unwrap_or_default();
    if body.contains("authorization_pending") {
        Ok(PollOutcome::Pending)
    } else if body.contains("slow_down") {
        Ok(PollOutcome::SlowDown)
    } else {
        Err(Error::Gql {
            op: "device_flow".into(),
            message: body,
        })
    }
}

/// Polls until the user enters the code, the code expires or `cancel` fires.
pub async fn wait_device_flow(
    net: &Net,
    profile: &ClientProfile,
    code: &DeviceCode,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<TokenResponse> {
    let mut interval = Duration::from_secs(code.interval);
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Err(Error::Cancelled),
            _ = tokio::time::sleep(interval) => {}
        }
        if Utc::now() >= code.expires_at {
            return Err(Error::Timeout(Duration::from_secs(0)));
        }
        match poll_device_flow(net, profile, code).await {
            Ok(PollOutcome::Done(t)) => return Ok(t),
            Ok(PollOutcome::Pending) => {}
            Ok(PollOutcome::SlowDown) => interval += Duration::from_secs(2),
            Err(e) if e.is_transient() => {}
            Err(e) => return Err(e),
        }
    }
}

pub async fn validate(net: &Net, access_token: &str) -> Result<Validation> {
    let resp = net
        .http
        .get(VALIDATE_URL)
        .header("Authorization", format!("OAuth {access_token}"))
        .send()
        .await?;
    match resp.status().as_u16() {
        200 => Ok(resp.json().await?),
        401 => Err(Error::Unauthorized),
        s => Err(Error::Http {
            status: s,
            url: VALIDATE_URL.into(),
        }),
    }
}

pub async fn refresh(
    net: &Net,
    profile: &ClientProfile,
    refresh_token: &str,
) -> Result<TokenResponse> {
    let resp = net
        .http
        .post(TOKEN_URL)
        .form(&[
            ("client_id", profile.client_id.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ])
        .send()
        .await?;
    match resp.status().as_u16() {
        200 => Ok(resp.json().await?),
        400 | 401 => Err(Error::Unauthorized),
        s => Err(Error::Http {
            status: s,
            url: TOKEN_URL.into(),
        }),
    }
}

/// Random 32-char alphanumeric device id, like the one the web client stores in `unique_id`.
pub fn new_device_id() -> String {
    use rand::Rng;
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::rng();
    (0..32)
        .map(|_| CHARS[rng.random_range(0..CHARS.len())] as char)
        .collect()
}

/// Builds credentials from a fresh token: validates it to learn who it belongs to.
pub async fn credentials_from_token(
    net: &Net,
    profile_name: &str,
    token: TokenResponse,
    device_id: Option<String>,
) -> Result<(Credentials, Validation)> {
    let v = validate(net, &token.access_token).await?;
    Ok((
        Credentials {
            user_id: v.user_id.clone(),
            login: v.login.clone(),
            access_token: token.access_token,
            refresh_token: token.refresh_token,
            profile: profile_name.to_string(),
            device_id: device_id.unwrap_or_else(new_device_id),
        },
        v,
    ))
}

#[cfg(test)]
mod tests {
    use super::complete_uri;

    #[test]
    fn activation_link_carries_the_code() {
        assert_eq!(
            complete_uri("https://www.twitch.tv/activate", "ABCD1234"),
            "https://www.twitch.tv/activate?device-code=ABCD1234"
        );
        assert_eq!(
            complete_uri("https://www.twitch.tv/activate?public=true", "ABCD1234"),
            "https://www.twitch.tv/activate?public=true&device-code=ABCD1234"
        );
        let already = "https://www.twitch.tv/activate?public=true&device-code=ABCD1234";
        assert_eq!(complete_uri(already, "ABCD1234"), already);
        assert_eq!(
            complete_uri("https://www.twitch.tv/activate", "a&b"),
            "https://www.twitch.tv/activate"
        );
    }
}
