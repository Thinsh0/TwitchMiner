//! Updates of the application itself: the latest GitHub release, and where the platform
//! allows it (`Platform::update_asset`), downloading it and replacing the running
//! executable. The engine checks its own version separately (`update` notice): with a
//! remote engine that one is about the server.

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
    /// Lowercase hex SHA-256 published by GitHub (`digest`), when known.
    pub sha256: Option<String>,
}

fn client() -> Result<reqwest::Client> {
    tm_core::twitch::tls::install_crypto_provider();
    Ok(reqwest::Client::builder()
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
    let name = release["tag_name"]
        .as_str()
        .filter(|_| installs)
        .and_then(asset_name);
    Ok(parse(&release, CURRENT, name.as_deref()))
}

fn parse(release: &Value, current: &str, asset_name: Option<&str>) -> Option<Release> {
    let version = release["tag_name"].as_str()?.to_string();
    if !tm_core::engine::is_newer(&version, current) {
        return None;
    }
    let asset = asset_name.and_then(|name| {
        let a = release["assets"]
            .as_array()?
            .iter()
            .find(|a| a["name"].as_str() == Some(name))?;
        Some(Asset {
            url: a["browser_download_url"].as_str()?.to_string(),
            size: a["size"].as_u64()?,
            sha256: a["digest"]
                .as_str()
                .and_then(|d| d.strip_prefix("sha256:"))
                .map(str::to_ascii_lowercase),
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
    let exe = std::env::current_exe()?;
    let new = sibling(&exe, "new");
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
    verify(&data, asset)?;
    tokio::fs::write(&new, &data)
        .await
        .with_context(|| format!("cannot write in {}", parent(&exe).display()))?;
    Ok(new)
}

fn verify(data: &[u8], asset: &Asset) -> Result<()> {
    if data.len() as u64 != asset.size {
        bail!(
            "incomplete download ({} of {} bytes)",
            data.len(),
            asset.size
        );
    }
    if let Some(expected) = &asset.sha256 {
        let digest = ring::digest::digest(&ring::digest::SHA256, data);
        let got: String = digest.as_ref().iter().map(|b| format!("{b:02x}")).collect();
        if &got != expected {
            bail!("corrupted download (SHA-256 mismatch)");
        }
    }
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
                  "browser_download_url": "https://example.invalid/exe" }
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
            a.sha256.as_deref(),
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
        assert!(parse(&release(), "0.1.2", None).unwrap().asset.is_none());
        assert!(parse(&release(), "0.2.0", None).is_none());
    }

    #[test]
    fn verifies_size_and_digest() {
        let asset = parse(&release(), "0.1.2", Some("TwitchMiner-v0.2.0-windows.exe"))
            .unwrap()
            .asset
            .unwrap();
        // SHA-256("abc").
        if !cfg!(windows) {
            assert!(verify(b"abc", &asset).is_ok());
        }
        assert!(verify(b"abd", &asset).is_err());
        assert!(verify(b"ab", &asset).is_err());
    }

    #[test]
    fn sibling_names() {
        let p = Path::new("/opt/tm/TwitchMiner.exe");
        assert_eq!(sibling(p, "old"), Path::new("/opt/tm/TwitchMiner.exe.old"));
    }
}
