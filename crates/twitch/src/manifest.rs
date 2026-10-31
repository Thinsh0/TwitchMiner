//! The Twitch "manifest": every volatile identifier Twitch can change under our feet
//! (persisted-query hashes, client IDs, user agents, spade URL).
//!
//! A copy is embedded at build time; a newer revision can be fetched at runtime
//! (see [`Manifest::fetch`]) so that a hash change on Twitch's side is fixed by
//! publishing a JSON file instead of shipping a new build.
//!
//! A downloaded manifest is used only if its detached ed25519 signature
//! (`<url>.sig`, made offline by the maintainer with `tools/sign-manifest.sh`)
//! checks with [`MANIFEST_PUBLIC_KEY`] over the exact bytes received, before any
//! parsing. Without a key in this build, no downloaded manifest is used.

use std::collections::BTreeMap;
use std::sync::Arc;

use arc_swap::ArcSwap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};

pub const EMBEDDED: &str = include_str!("../../../manifest/twitch.json");

/// Highest manifest schema understood by this build.
pub const SUPPORTED_SCHEMA: u32 = 1;

/// A manifest more than this many revisions ahead of the current one is refused: a
/// `revision` near `u32::MAX` would otherwise block every later fix.
pub const MAX_REVISION_JUMP: u32 = 1000;

/// A downloaded manifest larger than this is refused (the real one is a few KB).
pub const MAX_BYTES: usize = 256 * 1024;

/// Public key checking downloaded manifests: the 32 bytes of the ed25519 key, in
/// hex. Its private key is kept offline by the maintainer and is not the release
/// key (see docs/DEVELOPMENT.md, « Signer le manifeste Twitch »). Empty: this build
/// uses no downloaded manifest, only the embedded one.
#[rustfmt::skip]
pub const MANIFEST_PUBLIC_KEY: &str = "1499a6dbfe7c2f3e54f2da17ef116a3abd923e8592799eba01b47399df1222e1";

/// The signature of `<url>` is at `<url>` + this suffix (`twitch.json.sig`).
pub const SIGNATURE_SUFFIX: &str = ".sig";

/// A signature file larger than this is not one (64 raw bytes, or 128 hex digits).
pub const MAX_SIGNATURE_BYTES: usize = 1024;

/// How a downloaded manifest is trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    /// Its signature must check with this ed25519 public key.
    Key([u8; 32]),
    /// No signature check: only for an address the owner chose themselves, on their
    /// explicit request (`engine.manifest_unsigned_ok`).
    Unsigned,
}

impl Trust {
    /// The key built into this program; `None` when [`MANIFEST_PUBLIC_KEY`] is empty
    /// (or not a key), and then no downloaded manifest may be used.
    pub fn built_in() -> Option<Self> {
        built_in_key().map(Trust::Key)
    }
}

/// [`MANIFEST_PUBLIC_KEY`], when it is a key.
pub fn built_in_key() -> Option<[u8; 32]> {
    public_key(MANIFEST_PUBLIC_KEY)
}

/// A 32-byte ed25519 public key from its 64 hex digits.
pub fn public_key(hex: &str) -> Option<[u8; 32]> {
    hex_decode(hex)?.try_into().ok()
}

/// A signature file: the 64 raw bytes (what `openssl pkeyutl -sign` writes, as for
/// the release files), or their hex.
pub fn decode_signature(bytes: &[u8]) -> Option<[u8; 64]> {
    if let Ok(raw) = <[u8; 64]>::try_from(bytes) {
        return Some(raw);
    }
    hex_decode(std::str::from_utf8(bytes).ok()?)?
        .try_into()
        .ok()
}

/// Checks the ed25519 `signature` of exactly `data` with `key`.
pub fn verify_signature(data: &[u8], signature: &[u8], key: &[u8; 32]) -> Result<()> {
    let signature = decode_signature(signature)
        .ok_or_else(|| Error::Signature("unreadable manifest signature".into()))?;
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key)
        .verify(data, &signature)
        .map_err(|_| Error::Signature("manifest not signed by TwitchMiner's key".into()))
}

/// Where the signature of the manifest at `url` is: same address, `.sig` added to
/// the path (`…/twitch.json` → `…/twitch.json.sig`).
pub fn signature_url(url: &str) -> Result<String> {
    let mut u = url::Url::parse(url).map_err(|e| Error::decode(format!("manifest url: {e}")))?;
    let path = format!("{}{SIGNATURE_SUFFIX}", u.path());
    u.set_path(&path);
    Ok(u.to_string())
}

pub fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn hex_decode(text: &str) -> Option<Vec<u8>> {
    let t = text.trim();
    if !t.len().is_multiple_of(2) || !t.is_ascii() {
        return None;
    }
    (0..t.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&t[i..i + 2], 16).ok())
        .collect()
}

/// A downloaded manifest, with the exact text its signature covers (to cache both).
#[derive(Debug, Clone)]
pub struct Fetched {
    pub manifest: Manifest,
    pub raw: String,
    /// The signature as received; `None` when fetched with [`Trust::Unsigned`].
    pub signature: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub revision: u32,
    #[serde(default)]
    pub updated: String,
    pub default_profile: String,
    pub profiles: BTreeMap<String, ClientProfile>,
    pub web_user_agent: String,
    pub spade_url: String,
    /// Drop campaigns of these "games" (e.g. Special Events) count on any allowed
    /// channel, whatever it streams.
    #[serde(default)]
    pub special_game_ids: Vec<String>,
    pub operations: BTreeMap<String, Operation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientProfile {
    pub client_id: String,
    pub client_url: String,
    pub user_agents: Vec<String>,
    /// Whether this client may send non-persisted (raw) GQL queries.
    #[serde(default)]
    pub raw_queries: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    pub name: String,
    pub hash: String,
    /// Default variables, deep-merged under the caller's variables.
    #[serde(default)]
    pub variables: Value,
}

impl Manifest {
    pub fn embedded() -> Self {
        let m: Manifest = serde_json::from_str(EMBEDDED).expect("embedded manifest is valid");
        m.validate().expect("embedded manifest is consistent");
        m
    }

    pub fn parse(json: &str) -> Result<Self> {
        let m: Manifest = serde_json::from_str(json)?;
        m.validate()?;
        Ok(m)
    }

    /// Checks `signature` over the exact bytes of `raw` with `key`, and only then
    /// parses and validates it.
    pub fn parse_signed(raw: &[u8], signature: &[u8], key: &[u8; 32]) -> Result<Self> {
        verify_signature(raw, signature, key)?;
        let text = std::str::from_utf8(raw).map_err(|_| Error::decode("manifest is not UTF-8"))?;
        Self::parse(text)
    }

    /// Checks the schema and every identifier and address (only Twitch over https).
    pub fn validate(&self) -> Result<()> {
        if self.schema > SUPPORTED_SCHEMA {
            return Err(Error::decode(format!(
                "manifest schema {} unsupported",
                self.schema
            )));
        }
        if !self.profiles.contains_key(&self.default_profile) {
            return Err(Error::decode("default profile missing"));
        }
        for (key, op) in &self.operations {
            if op.hash.len() != 64 || !op.hash.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(Error::decode(format!("bad hash for {key}")));
            }
        }
        for p in self.profiles.values() {
            if p.client_id.is_empty() || p.user_agents.is_empty() {
                return Err(Error::decode("profile without client id / user agent"));
            }
            if !is_https(&p.client_url) {
                return Err(Error::decode("client_url must be an https:// address"));
            }
        }
        if !is_https(&self.spade_url) || !is_twitch_host(&self.spade_url) {
            return Err(Error::decode(
                "spade_url must be an https:// address on twitch.tv",
            ));
        }
        Ok(())
    }

    /// Whether this manifest may replace `current`: newer, but not absurdly so.
    pub fn may_replace(&self, current: &Manifest) -> bool {
        self.revision > current.revision && self.revision - current.revision <= MAX_REVISION_JUMP
    }

    pub fn profile(&self, name: &str) -> Option<&ClientProfile> {
        self.profiles.get(name)
    }

    pub fn operation(&self, key: &str) -> Result<&Operation> {
        self.operations
            .get(key)
            .ok_or_else(|| Error::UnknownOperation(key.to_string()))
    }

    /// Downloads a manifest (and, with [`Trust::Key`], its signature) and returns it
    /// only if the signature checks, then if it is valid and newer than `current`.
    pub async fn fetch(
        http: &reqwest::Client,
        url: &str,
        current: &Manifest,
        trust: Trust,
    ) -> Result<Option<Fetched>> {
        let body = download(http, url, MAX_BYTES, "manifest too large").await?;
        let signature = match trust {
            Trust::Key(_) => {
                let sig_url = signature_url(url)?;
                let sig = download(http, &sig_url, MAX_SIGNATURE_BYTES, "not a signature")
                    .await
                    .map_err(|e| match e {
                        Error::Http { status: 404, .. } => {
                            Error::Signature("manifest signature missing".into())
                        }
                        e => e,
                    })?;
                Some(sig)
            }
            Trust::Unsigned => None,
        };
        Self::accept(body, signature, current, trust)
    }

    /// What [`Manifest::fetch`] does with what it downloaded: the signature first,
    /// on the exact bytes, then the content and the revision.
    pub fn accept(
        body: Vec<u8>,
        signature: Option<Vec<u8>>,
        current: &Manifest,
        trust: Trust,
    ) -> Result<Option<Fetched>> {
        let m = match (trust, &signature) {
            (Trust::Key(key), Some(sig)) => Manifest::parse_signed(&body, sig, &key)?,
            (Trust::Key(_), None) => {
                return Err(Error::Signature("manifest signature missing".into()));
            }
            (Trust::Unsigned, _) => {
                let text = std::str::from_utf8(&body)
                    .map_err(|_| Error::decode("manifest is not UTF-8"))?;
                Manifest::parse(text)?
            }
        };
        if m.revision > current.revision && !m.may_replace(current) {
            return Err(Error::decode(format!(
                "manifest revision {} is too far ahead of {}",
                m.revision, current.revision
            )));
        }
        if !m.may_replace(current) {
            return Ok(None);
        }
        let raw = String::from_utf8(body).map_err(|_| Error::decode("manifest is not UTF-8"))?;
        Ok(Some(Fetched {
            manifest: m,
            raw,
            signature: signature.filter(|_| matches!(trust, Trust::Key(_))),
        }))
    }
}

/// GETs `url`, refusing more than `max` bytes.
async fn download(
    http: &reqwest::Client,
    url: &str,
    max: usize,
    too_large: &str,
) -> Result<Vec<u8>> {
    let mut resp = http
        .get(url)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await?
        .error_for_status()?;
    if resp.content_length().is_some_and(|n| n > max as u64) {
        return Err(Error::decode(too_large));
    }
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        if body.len() + chunk.len() > max {
            return Err(Error::decode(too_large));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn is_https(raw: &str) -> bool {
    url::Url::parse(raw).is_ok_and(|u| u.scheme() == "https" && u.host_str().is_some())
}

fn is_twitch_host(raw: &str) -> bool {
    url::Url::parse(raw).is_ok_and(|u| {
        u.host_str()
            .is_some_and(|h| h == "twitch.tv" || h.ends_with(".twitch.tv"))
    })
}

/// Shared, hot-swappable manifest handle.
pub type SharedManifest = Arc<ArcSwap<Manifest>>;

pub fn shared(m: Manifest) -> SharedManifest {
    Arc::new(ArcSwap::from_pointee(m))
}

/// Recursively merges `overlay` into `base` (objects merged, everything else replaced).
pub fn merge_json(base: &mut Value, overlay: &Value) {
    match (base, overlay) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                merge_json(b.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (b, o) => *b = o.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn embedded_is_valid() {
        let m = Manifest::embedded();
        assert!(m.operation("Inventory").is_ok());
        assert!(m.profile(&m.default_profile).is_some());
    }

    #[test]
    fn addresses_and_revisions_are_checked() {
        let base = Manifest::embedded();
        let with = |f: &dyn Fn(&mut Manifest)| {
            let mut m = base.clone();
            f(&mut m);
            m.validate()
        };
        assert!(with(&|m| m.spade_url = "http://spade.twitch.tv/track".into()).is_err());
        assert!(with(&|m| m.spade_url = "https://evil.example/track".into()).is_err());
        assert!(with(&|m| m.spade_url = "https://twitch.tv.evil.example/x".into()).is_err());
        assert!(with(&|m| m.spade_url = "https://video-edge.twitch.tv/x".into()).is_ok());
        assert!(
            with(&|m| {
                for p in m.profiles.values_mut() {
                    p.client_url = "http://www.twitch.tv".into();
                }
            })
            .is_err()
        );
        let mut next = base.clone();
        next.revision = base.revision + 1;
        assert!(next.may_replace(&base));
        assert!(!base.may_replace(&next));
        next.revision = u32::MAX;
        assert!(!next.may_replace(&base));
    }

    /// A key pair made for this test only (never stored).
    fn test_key() -> (ring::signature::Ed25519KeyPair, [u8; 32]) {
        use ring::signature::{Ed25519KeyPair, KeyPair};
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
        let pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let public = pair.public_key().as_ref().try_into().unwrap();
        (pair, public)
    }

    /// The embedded manifest with the next revision, as served text.
    fn next_revision() -> (Manifest, Vec<u8>) {
        let current = Manifest::embedded();
        let mut next = current.clone();
        next.revision += 1;
        (current, serde_json::to_vec_pretty(&next).unwrap())
    }

    #[test]
    fn a_downloaded_manifest_needs_a_valid_signature() {
        let (pair, key) = test_key();
        let (current, body) = next_revision();
        let sig = pair.sign(&body).as_ref().to_vec();
        let trust = Trust::Key(key);

        // Valid, raw 64 bytes or hex (with a newline, as `od | tr` + echo writes).
        let f = Manifest::accept(body.clone(), Some(sig.clone()), &current, trust)
            .unwrap()
            .unwrap();
        assert_eq!(f.manifest.revision, current.revision + 1);
        assert_eq!(f.raw.as_bytes(), body.as_slice());
        assert_eq!(f.signature.as_deref(), Some(sig.as_slice()));
        let hex = format!("{}\n", hex_encode(&sig)).into_bytes();
        assert!(Manifest::accept(body.clone(), Some(hex), &current, trust).is_ok());

        // Missing.
        let e = Manifest::accept(body.clone(), None, &current, trust).unwrap_err();
        assert!(matches!(e, Error::Signature(_)), "{e}");
        // Invalid (one bit), unreadable, or from another key.
        let mut bad = sig.clone();
        bad[10] ^= 1;
        let e = Manifest::accept(body.clone(), Some(bad), &current, trust).unwrap_err();
        assert!(matches!(e, Error::Signature(_)), "{e}");
        let e =
            Manifest::accept(body.clone(), Some(b"nope".to_vec()), &current, trust).unwrap_err();
        assert!(matches!(e, Error::Signature(_)), "{e}");
        let (_, other) = test_key();
        assert!(
            Manifest::accept(body.clone(), Some(sig.clone()), &current, Trust::Key(other)).is_err()
        );
        // Content changed by one byte (a space added keeps valid JSON).
        let mut changed = body.clone();
        changed.push(b' ');
        let e = Manifest::accept(changed, Some(sig.clone()), &current, trust).unwrap_err();
        assert!(matches!(e, Error::Signature(_)), "{e}");
        let mut flipped = body.clone();
        let i = flipped.iter().position(|&b| b == b'a').unwrap();
        flipped[i] = b'b';
        assert!(Manifest::accept(flipped, Some(sig.clone()), &current, trust).is_err());

        // The signature is checked before the content: garbage signed is refused by
        // the parser, unsigned garbage by the signature.
        let garbage = b"{not json".to_vec();
        let e = Manifest::accept(garbage.clone(), Some(sig.clone()), &current, trust).unwrap_err();
        assert!(matches!(e, Error::Signature(_)), "{e}");
        let gsig = pair.sign(&garbage).as_ref().to_vec();
        let e = Manifest::accept(garbage, Some(gsig), &current, trust).unwrap_err();
        assert!(matches!(e, Error::Decode(_)), "{e}");

        // A valid signature does not make an older revision acceptable.
        assert!(
            Manifest::accept(body.clone(), Some(sig), &current.clone(), trust)
                .unwrap()
                .is_some()
        );
        let mut newer = current.clone();
        newer.revision += 1;
        let same = serde_json::to_vec(&newer).unwrap();
        let ssig = pair.sign(&same).as_ref().to_vec();
        assert!(
            Manifest::accept(same, Some(ssig), &newer, trust)
                .unwrap()
                .is_none()
        );

        // Unsigned only when asked for.
        let f = Manifest::accept(body, None, &current, Trust::Unsigned)
            .unwrap()
            .unwrap();
        assert!(f.signature.is_none());
    }

    #[test]
    fn keys_and_signature_addresses() {
        // An empty or malformed key is no key: nothing downloaded is trusted.
        assert!(public_key("").is_none());
        assert!(public_key("abcd").is_none());
        assert!(public_key(&"zz".repeat(32)).is_none());
        assert!(public_key(&"ab".repeat(32)).is_some());
        if MANIFEST_PUBLIC_KEY.is_empty() {
            assert!(Trust::built_in().is_none());
        } else {
            assert!(
                Trust::built_in().is_some(),
                "MANIFEST_PUBLIC_KEY: 64 hex characters"
            );
        }
        assert_eq!(
            signature_url("https://raw.githubusercontent.com/o/r/HEAD/manifest/twitch.json")
                .unwrap(),
            "https://raw.githubusercontent.com/o/r/HEAD/manifest/twitch.json.sig"
        );
        assert_eq!(
            signature_url("https://example.com/m/twitch.json?ref=x").unwrap(),
            "https://example.com/m/twitch.json.sig?ref=x"
        );
    }

    /// CI guard: once the key is set, the committed `manifest/twitch.json.sig` must
    /// sign the embedded `manifest/twitch.json`, so a manifest changed without being
    /// signed again (tools/sign-manifest.sh) fails the tests.
    #[test]
    fn the_committed_manifest_is_signed() {
        let Some(key) = built_in_key() else {
            assert!(
                MANIFEST_PUBLIC_KEY.is_empty(),
                "MANIFEST_PUBLIC_KEY must be 64 hex characters"
            );
            eprintln!(
                "MANIFEST_PUBLIC_KEY is empty: downloaded manifests are refused, nothing to check"
            );
            return;
        };
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../manifest/twitch.json.sig"
        );
        let sig = std::fs::read(path).unwrap_or_else(|e| {
            panic!("{path}: {e} (sign the manifest: tools/sign-manifest.sh <private key>)")
        });
        Manifest::parse_signed(EMBEDDED.as_bytes(), &sig, &key).unwrap_or_else(|e| {
            panic!("manifest/twitch.json.sig does not sign manifest/twitch.json ({e}): run tools/sign-manifest.sh <private key>")
        });
    }

    #[tokio::test]
    async fn fetch_downloads_the_signature_next_to_the_manifest() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        crate::tls::install_crypto_provider();
        let (pair, key) = test_key();
        let (current, body) = next_revision();
        let sig = pair.sign(&body).as_ref().to_vec();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let served = (body.clone(), sig);
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let mut buf = [0u8; 4096];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let line = String::from_utf8_lossy(&buf[..n])
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .to_string();
                let found = match line.split(' ').nth(1).unwrap_or_default() {
                    "/signed/twitch.json" | "/unsigned/twitch.json" => Some(served.0.clone()),
                    "/signed/twitch.json.sig" => Some(served.1.clone()),
                    _ => None,
                };
                let head = match &found {
                    Some(b) => format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        b.len()
                    ),
                    None => {
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .into()
                    }
                };
                let _ = sock.write_all(head.as_bytes()).await;
                if let Some(b) = found {
                    let _ = sock.write_all(&b).await;
                }
            }
        });
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let trust = Trust::Key(key);
        let f = Manifest::fetch(
            &http,
            &format!("{base}/signed/twitch.json"),
            &current,
            trust,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(f.raw.as_bytes(), body.as_slice());
        let e = Manifest::fetch(
            &http,
            &format!("{base}/unsigned/twitch.json"),
            &current,
            trust,
        )
        .await
        .unwrap_err();
        assert!(matches!(e, Error::Signature(_)), "{e}");
        let f = Manifest::fetch(
            &http,
            &format!("{base}/unsigned/twitch.json"),
            &current,
            Trust::Unsigned,
        )
        .await
        .unwrap();
        assert!(f.is_some());
    }

    #[test]
    fn merge_is_deep() {
        let mut a = json!({"a": 1, "o": {"x": 1, "y": 2}});
        merge_json(&mut a, &json!({"o": {"y": 3, "z": 4}, "b": true}));
        assert_eq!(a, json!({"a": 1, "b": true, "o": {"x": 1, "y": 3, "z": 4}}));
    }
}
