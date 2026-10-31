//! Updates of the application itself: the latest GitHub release, and where the platform
//! allows it (`Platform::update_asset`), downloading it and replacing the running
//! executable. The engine checks its own version separately (`update` notice): with a
//! remote engine that one is about the server.
//!
//! An update is installed only if its detached ed25519 signature (`<file>.sig`, made by
//! the Release workflow) checks with [`RELEASE_PUBLIC_KEY`], built into the program,
//! and its SHA-256 matches the one GitHub publishes. Anything missing (no key in this
//! build, no signature, no digest): only the release page is offered.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;
use tm_core::settings::DEFAULT_RELEASES_URL;

/// The application checks this often (and at start).
pub const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);
/// An update larger than this is refused (the Windows executable is ~30 MB).
const MAX_BYTES: u64 = 300 * 1024 * 1024;

pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// Public key checking the updates: the 32 bytes of the ed25519 key, in hex (see
/// docs/DESKTOP.md, « Signature des mises à jour », to make the key pair). Empty: this
/// build installs no update by itself.
pub const RELEASE_PUBLIC_KEY: &str = "";

/// A signature file larger than this is not one.
const MAX_SIGNATURE_BYTES: usize = 1024;

fn hex_bytes(text: &str) -> Option<Vec<u8>> {
    let t = text.trim();
    if !t.len().is_multiple_of(2) || !t.is_ascii() {
        return None;
    }
    (0..t.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&t[i..i + 2], 16).ok())
        .collect()
}

/// The built-in key, when there is a valid one.
fn release_key() -> Option<Vec<u8>> {
    hex_bytes(RELEASE_PUBLIC_KEY).filter(|k| k.len() == 32)
}

/// A newer published version.
#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    /// Tag, like `v0.1.3`.
    pub version: String,
    /// Release page.
    pub page: String,
    /// The file that replaces this executable, when the platform installs updates.
    pub asset: Option<Asset>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Asset {
    pub url: String,
    pub size: u64,
    /// Lowercase hex SHA-256 published by GitHub (`digest`).
    pub sha256: String,
    /// The detached signature (`<name>.sig`).
    pub signature_url: String,
}

fn client() -> Result<reqwest::Client> {
    Ok(crate::http_client()
        .user_agent(concat!("TwitchMiner/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(20))
        .build()?)
}

/// The release file that replaces this executable (see the Release workflow).
fn asset_name(version: &str) -> Option<String> {
    cfg!(windows).then(|| format!("TwitchMiner-{version}-windows.exe"))
}

/// The latest release when it is newer than this application. `installs`: the
/// platform can install it (otherwise only its page is offered).
pub async fn check(installs: bool) -> Result<Option<Release>> {
    let resp = client()?
        .get(DEFAULT_RELEASES_URL)
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(20))
        .send()
        .await?;
    if resp.status().as_u16() == 404 {
        return Ok(None);
    }
    let release: Value = resp.error_for_status()?.json().await?;
    // Without a key to check them, updates are never installed by the application.
    let name = release["tag_name"]
        .as_str()
        .filter(|_| installs && release_key().is_some())
        .and_then(asset_name);
    Ok(parse(&release, CURRENT, name.as_deref()))
}

fn parse(release: &Value, current: &str, asset_name: Option<&str>) -> Option<Release> {
    let version = release["tag_name"].as_str()?.to_string();
    if !tm_core::engine::is_newer(&version, current) {
        return None;
    }
    // Installable only with its digest and its signature.
    let asset = asset_name.and_then(|name| {
        let assets = release["assets"].as_array()?;
        let find = |n: &str| assets.iter().find(|a| a["name"].as_str() == Some(n));
        let a = find(name)?;
        let sig = find(&format!("{name}.sig"))?;
        Some(Asset {
            url: a["browser_download_url"].as_str()?.to_string(),
            size: a["size"].as_u64()?,
            sha256: a["digest"]
                .as_str()
                .and_then(|d| d.strip_prefix("sha256:"))
                .filter(|d| d.len() == 64)
                .map(str::to_ascii_lowercase)?,
            signature_url: sig["browser_download_url"].as_str()?.to_string(),
        })
    });
    Some(Release {
        version,
        page: release["html_url"].as_str().unwrap_or_default().to_string(),
        asset,
    })
}

/// Downloads `asset` next to the running executable and checks it; `progress` gets
/// the downloaded fraction. Returns the downloaded file.
pub async fn download(asset: &Asset, progress: impl Fn(f32)) -> Result<PathBuf> {
    if asset.size > MAX_BYTES {
        bail!("update too large ({} bytes)", asset.size);
    }
    let key = release_key()
        .context("this build cannot check updates: install it from the release page")?;
    let exe = std::env::current_exe()?;
    let new = sibling(&exe, "new");
    let mut sig_resp = client()?
        .get(&asset.signature_url)
        .timeout(Duration::from_secs(60))
        .send()
        .await?
        .error_for_status()?;
    let mut signature = Vec::new();
    while let Some(chunk) = sig_resp.chunk().await? {
        signature.extend_from_slice(&chunk);
        if signature.len() > MAX_SIGNATURE_BYTES {
            bail!("not a signature");
        }
    }
    let mut resp = client()?
        .get(&asset.url)
        .timeout(Duration::from_secs(600))
        .send()
        .await?
        .error_for_status()?;
    let mut data = Vec::with_capacity(asset.size as usize);
    while let Some(chunk) = resp.chunk().await? {
        data.extend_from_slice(&chunk);
        if data.len() as u64 > MAX_BYTES {
            bail!("update too large");
        }
        progress(data.len() as f32 / asset.size.max(1) as f32);
    }
    verify(&data, asset, &signature, &key)?;
    tokio::fs::write(&new, &data)
        .await
        .with_context(|| format!("cannot write in {}", parent(&exe).display()))?;
    Ok(new)
}

/// Size, SHA-256 and the ed25519 signature (raw 64 bytes, or their hex) of the file
/// with `key`. Fails closed: nothing is installed unless everything checks.
fn verify(data: &[u8], asset: &Asset, signature: &[u8], key: &[u8]) -> Result<()> {
    if data.len() as u64 != asset.size {
        bail!(
            "incomplete download ({} of {} bytes)",
            data.len(),
            asset.size
        );
    }
    let digest = ring::digest::digest(&ring::digest::SHA256, data);
    let got: String = digest.as_ref().iter().map(|b| format!("{b:02x}")).collect();
    if got != asset.sha256 {
        bail!("corrupted download (SHA-256 mismatch)");
    }
    let signature = match signature.len() {
        64 => signature.to_vec(),
        _ => std::str::from_utf8(signature)
            .ok()
            .and_then(hex_bytes)
            .filter(|s| s.len() == 64)
            .context("unreadable update signature")?,
    };
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key)
        .verify(data, &signature)
        .map_err(|_| anyhow!("the update is not signed by TwitchMiner's key: not installed"))?;
    if cfg!(windows) && !data.starts_with(b"MZ") {
        bail!("the downloaded file is not a Windows program");
    }
    Ok(())
}

/// Puts `new` in place of the running executable, which is kept as `.old` until the
/// next start (Windows cannot delete a running program, but can rename it). Returns
/// the executable to start.
pub fn install(new: &Path) -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let old = sibling(&exe, "old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).with_context(|| format!("cannot replace {}", exe.display()))?;
    if let Err(e) = std::fs::rename(new, &exe) {
        // Put the running version back rather than leaving nothing to start.
        let _ = std::fs::rename(&old, &exe);
        return Err(anyhow!(e).context(format!("cannot replace {}", exe.display())));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755));
    }
    Ok(exe)
}

/// Removes what an update left behind (the previous executable, a failed download).
pub fn cleanup() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(sibling(&exe, "old"));
        let _ = std::fs::remove_file(sibling(&exe, "new"));
    }
}

/// `TwitchMiner.exe` → `TwitchMiner.exe.<suffix>`.
fn sibling(exe: &Path, suffix: &str) -> PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(suffix);
    exe.with_file_name(name)
}

fn parent(exe: &Path) -> &Path {
    exe.parent().unwrap_or(exe)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn release() -> Value {
        json!({
            "tag_name": "v0.2.0",
            "html_url": "https://github.com/Thinsh0/TwitchMiner/releases/tag/v0.2.0",
            "assets": [
                { "name": "twitchminer-v0.2.0-x86_64-pc-windows-msvc.zip", "size": 1,
                  "browser_download_url": "https://example.invalid/zip" },
                { "name": "TwitchMiner-v0.2.0-windows.exe", "size": 3,
                  "digest": "sha256:BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD",
                  "browser_download_url": "https://example.invalid/exe" },
                { "name": "TwitchMiner-v0.2.0-windows.exe.sig", "size": 64,
                  "browser_download_url": "https://example.invalid/exe.sig" }
            ]
        })
    }

    #[test]
    fn picks_the_asset_of_a_newer_release() {
        let r = parse(&release(), "0.1.2", Some("TwitchMiner-v0.2.0-windows.exe")).unwrap();
        assert_eq!(r.version, "v0.2.0");
        let a = r.asset.unwrap();
        assert_eq!(a.url, "https://example.invalid/exe");
        assert_eq!(a.size, 3);
        assert_eq!(
            a.sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(a.signature_url, "https://example.invalid/exe.sig");
        assert!(parse(&release(), "0.1.2", None).unwrap().asset.is_none());
        assert!(parse(&release(), "0.2.0", None).is_none());
    }

    #[test]
    fn no_signature_or_digest_means_no_installation() {
        let name = Some("TwitchMiner-v0.2.0-windows.exe");
        let mut unsigned = release();
        unsigned["assets"].as_array_mut().unwrap().pop();
        let r = parse(&unsigned, "0.1.2", name).unwrap();
        assert!(r.asset.is_none(), "the page is still offered");
        let mut no_digest = release();
        no_digest["assets"][1]
            .as_object_mut()
            .unwrap()
            .remove("digest");
        assert!(parse(&no_digest, "0.1.2", name).unwrap().asset.is_none());
    }

    #[test]
    fn the_built_in_key_is_valid_or_absent() {
        // Placeholder (empty): no automatic installation, without failing.
        if RELEASE_PUBLIC_KEY.is_empty() {
            assert!(release_key().is_none());
        } else {
            assert!(
                release_key().is_some(),
                "RELEASE_PUBLIC_KEY: 64 hex characters"
            );
        }
    }

    /// A key pair made for this test only (never stored).
    fn test_key() -> (ring::signature::Ed25519KeyPair, Vec<u8>) {
        use ring::signature::KeyPair;
        let pkcs8 =
            ring::signature::Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
                .unwrap();
        let pair = ring::signature::Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let public = pair.public_key().as_ref().to_vec();
        (pair, public)
    }

    #[test]
    fn verifies_size_digest_and_signature() {
        let asset = parse(&release(), "0.1.2", Some("TwitchMiner-v0.2.0-windows.exe"))
            .unwrap()
            .asset
            .unwrap();
        let (pair, public) = test_key();
        let sig = pair.sign(b"abc");
        let sig = sig.as_ref();
        let hex: String = sig.iter().map(|b| format!("{b:02x}")).collect();
        // SHA-256("abc"); a Windows build also wants a program.
        if !cfg!(windows) {
            assert!(verify(b"abc", &asset, sig, &public).is_ok());
            assert!(verify(b"abc", &asset, format!("{hex}\n").as_bytes(), &public).is_ok());
        }
        // Wrong key, damaged or missing signature, other content.
        let (_, other) = test_key();
        assert!(verify(b"abc", &asset, sig, &other).is_err());
        let mut bad = sig.to_vec();
        bad[0] ^= 1;
        assert!(verify(b"abc", &asset, &bad, &public).is_err());
        assert!(verify(b"abc", &asset, b"", &public).is_err());
        assert!(verify(b"abc", &asset, &sig[..32], &public).is_err());
        assert!(verify(b"abd", &asset, sig, &public).is_err());
        assert!(verify(b"ab", &asset, sig, &public).is_err());
    }

    #[test]
    fn sibling_names() {
        let p = Path::new("/opt/tm/TwitchMiner.exe");
        assert_eq!(sibling(p, "old"), Path::new("/opt/tm/TwitchMiner.exe.old"));
    }
}
