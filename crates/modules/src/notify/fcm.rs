//! Firebase Cloud Messaging (HTTP v1 API): pushes to the TwitchMiner Android app.
//!
//! The engine signs in to Google with the Firebase project's service account key (a
//! JSON file from the Firebase console): a JWT signed with its RSA key is traded for
//! an access token valid one hour, kept here until it nearly expires.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use ring::signature::{RSA_PKCS1_SHA256, RsaKeyPair};
use serde::Deserialize;
use serde_json::{Value, json};

const SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";
const TOKEN_URI: &str = "https://oauth2.googleapis.com/token";

/// A service account key, checked and ready to sign.
pub struct Account {
    pub project_id: String,
    pub client_email: String,
    token_uri: String,
    key: RsaKeyPair,
}

impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account")
            .field("project_id", &self.project_id)
            .field("client_email", &self.client_email)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
struct KeyFile {
    #[serde(rename = "type")]
    kind: String,
    project_id: String,
    client_email: String,
    private_key: String,
    #[serde(default)]
    token_uri: Option<String>,
}

impl Account {
    /// Reads the key file's content.
    pub fn parse(json: &str) -> anyhow::Result<Self> {
        let file: KeyFile = serde_json::from_str(json.trim())
            .context("not a Firebase service account key (JSON file expected)")?;
        if file.kind != "service_account" {
            return Err(anyhow!(
                "not a service account key (Firebase › Project settings › Service accounts › Generate new private key)"
            ));
        }
        let body: String = file
            .private_key
            .lines()
            .filter(|l| !l.starts_with("-----"))
            .flat_map(|l| l.chars().filter(|c| !c.is_whitespace()))
            .collect();
        let der = STANDARD
            .decode(body)
            .context("unreadable private key in the service account key")?;
        let key = RsaKeyPair::from_pkcs8(&der)
            .map_err(|e| anyhow!("unusable private key in the service account key: {e}"))?;
        Ok(Self {
            project_id: file.project_id,
            client_email: file.client_email,
            token_uri: file.token_uri.unwrap_or_else(|| TOKEN_URI.into()),
            key,
        })
    }

    /// The signed request for an access token (RFC 7523).
    fn assertion(&self, now: i64) -> anyhow::Result<String> {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
        let claims = json!({
            "iss": self.client_email,
            "scope": SCOPE,
            "aud": self.token_uri,
            "iat": now,
            "exp": now + 3600,
        });
        let claims = URL_SAFE_NO_PAD.encode(claims.to_string());
        let signed = format!("{header}.{claims}");
        let mut sig = vec![0; self.key.public().modulus_len()];
        self.key
            .sign(
                &RSA_PKCS1_SHA256,
                &ring::rand::SystemRandom::new(),
                signed.as_bytes(),
                &mut sig,
            )
            .map_err(|_| anyhow!("cannot sign with the service account key"))?;
        Ok(format!("{signed}.{}", URL_SAFE_NO_PAD.encode(sig)))
    }
}

/// Access tokens by service account, with when to stop using them.
static TOKENS: LazyLock<Mutex<HashMap<String, (String, Instant)>>> =
    LazyLock::new(Default::default);

/// A valid access token for `account`, from Google when none is kept.
pub async fn access_token(http: &reqwest::Client, account: &Account) -> anyhow::Result<String> {
    let cached = TOKENS
        .lock()
        .ok()
        .and_then(|t| t.get(&account.client_email).cloned());
    if let Some((token, until)) = cached
        && Instant::now() < until
    {
        return Ok(token);
    }
    let assertion = account.assertion(chrono::Utc::now().timestamp())?;
    let resp = http
        .post(&account.token_uri)
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
            ("assertion", assertion.as_str()),
        ])
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| e.without_url())
        .context("Google sign-in")?;
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    let Some(token) = body["access_token"].as_str() else {
        // `invalid_grant`: key deleted in the console, or the clock is far off.
        let why = body["error_description"]
            .as_str()
            .or(body["error"].as_str())
            .unwrap_or_default();
        return Err(anyhow!(
            "Google refused the service account key (HTTP {status}): {why}"
        ));
    };
    let lifetime = body["expires_in"].as_u64().unwrap_or(3600);
    // Renewed five minutes early.
    let until = Instant::now() + Duration::from_secs(lifetime.saturating_sub(300));
    if let Ok(mut t) = TOKENS.lock() {
        t.insert(account.client_email.clone(), (token.to_string(), until));
    }
    Ok(token.to_string())
}

/// Forgets the access token of `account` (rejected by FCM).
pub fn forget(account: &Account) {
    if let Ok(mut t) = TOKENS.lock() {
        t.remove(&account.client_email);
    }
}

/// The `messages:send` URL of the account's project.
pub fn send_url(account: &Account) -> String {
    format!(
        "https://fcm.googleapis.com/v1/projects/{}/messages:send",
        account.project_id
    )
}

/// Android notification channels created by the app (`MainActivity.java`).
pub const CHANNEL_URGENT: &str = "urgent";
pub const CHANNEL_EVENTS: &str = "events";

/// The message for one phone. A notification message: Android shows it even when the
/// app is not running; urgent ones are delivered at once and pop up.
pub fn message(token: &str, text: &str, urgent: bool) -> Value {
    json!({
        "message": {
            "token": token,
            "notification": { "title": "TwitchMiner", "body": text },
            "android": {
                "priority": if urgent { "HIGH" } else { "NORMAL" },
                "notification": {
                    "channel_id": if urgent { CHANNEL_URGENT } else { CHANNEL_EVENTS },
                },
            },
        }
    })
}

/// A readable reason from an FCM error answer (`UNREGISTERED`: the app was removed or
/// its data cleared; the phone must be added again).
pub fn error_reason(body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    let err = &v["error"];
    let code = err["details"]
        .as_array()
        .and_then(|d| d.iter().find_map(|x| x["errorCode"].as_str()))
        .or(err["status"].as_str())?;
    Some(code.to_string())
}

/// A parsed key, or why it is unusable.
pub type Parsed = Result<Arc<Account>, String>;

/// Cached parse of the key in the settings: parsing checks the RSA key, done once per
/// distinct key.
pub fn account_for(key: &str) -> Option<Parsed> {
    static LAST: LazyLock<Mutex<Option<(String, Parsed)>>> = LazyLock::new(Default::default);
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    let mut last = LAST.lock().ok()?;
    if let Some((k, a)) = last.as_ref()
        && k == key
    {
        return Some(a.clone());
    }
    let a = Account::parse(key)
        .map(Arc::new)
        .map_err(|e| format!("{e:#}"));
    *last = Some((key.to_string(), a.clone()));
    Some(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway 2048-bit RSA key (PKCS#8), made for this test only.
    const TEST_KEY: &str = include_str!("fcm_test_key.pem");

    fn key_file(private_key: &str) -> String {
        json!({
            "type": "service_account",
            "project_id": "twitchminer-test",
            "private_key_id": "abc",
            "private_key": private_key,
            "client_email": "firebase-adminsdk@twitchminer-test.iam.gserviceaccount.com",
            "token_uri": "https://oauth2.googleapis.com/token",
        })
        .to_string()
    }

    #[test]
    fn reads_a_key_file_and_signs() {
        let a = Account::parse(&key_file(TEST_KEY)).unwrap();
        assert_eq!(a.project_id, "twitchminer-test");
        assert_eq!(
            send_url(&a),
            "https://fcm.googleapis.com/v1/projects/twitchminer-test/messages:send"
        );
        let jwt = a.assertion(1_700_000_000).unwrap();
        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3);
        let claims: Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(claims["scope"], SCOPE);
        assert_eq!(claims["exp"], 1_700_003_600);
        // RSA PKCS#1 v1.5 is deterministic: the signature checks with the public key.
        let public = ring::signature::UnparsedPublicKey::new(
            &ring::signature::RSA_PKCS1_2048_8192_SHA256,
            a.key.public().as_ref(),
        );
        let sig = URL_SAFE_NO_PAD.decode(parts[2]).unwrap();
        public
            .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &sig)
            .unwrap();
    }

    #[test]
    fn rejects_what_is_not_a_key() {
        assert!(Account::parse("{}").is_err());
        assert!(Account::parse(r#"{"type": "authorized_user"}"#).is_err());
        let google_services = r#"{"project_info": {"project_id": "x"}, "client": []}"#;
        assert!(Account::parse(google_services).is_err());
        assert!(
            Account::parse(&key_file(
                "-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----\n"
            ))
            .is_err()
        );
        assert!(account_for("  ").is_none());
        assert!(matches!(account_for("nope"), Some(Err(_))));
    }

    #[test]
    fn messages_and_errors() {
        let m = message("tok", "hi", true);
        assert_eq!(m["message"]["android"]["priority"], "HIGH");
        assert_eq!(
            m["message"]["android"]["notification"]["channel_id"],
            CHANNEL_URGENT
        );
        let body = r#"{"error": {"code": 404, "status": "NOT_FOUND", "details": [
            {"@type": "type.googleapis.com/google.firebase.fcm.v1.FcmError", "errorCode": "UNREGISTERED"}]}}"#;
        assert_eq!(error_reason(body).as_deref(), Some("UNREGISTERED"));
        assert_eq!(
            error_reason(r#"{"error": {"status": "PERMISSION_DENIED"}}"#).as_deref(),
            Some("PERMISSION_DENIED")
        );
        assert_eq!(error_reason("<html>"), None);
    }
}
