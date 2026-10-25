//! Windows: embeds the icon and the version information in the executable (shown by
//! Explorer, the Start menu, the installed apps list and the task manager).

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap_or_default());
    let icon = manifest.join("assets").join("icon.ico");
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let mut parts: Vec<u32> = version
        .split(['.', '-', '+'])
        .take(3)
        .map(|p| p.parse().unwrap_or(0))
        .collect();
    parts.resize(4, 0);
    let numeric = parts
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let rc = format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "FileDescription", "TwitchMiner"
      VALUE "ProductName", "TwitchMiner"
      VALUE "FileVersion", "{version}"
      VALUE "ProductVersion", "{version}"
      VALUE "OriginalFilename", "twitchminer-desktop.exe"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = icon.display().to_string().replace('\\', "\\\\"),
    );
    let rc_path = out.join("twitchminer.rc");
    if let Err(e) = std::fs::write(&rc_path, rc) {
        println!("cargo:warning=icon not embedded: {e}");
        return;
    }
    // A missing resource compiler only costs the icon: the build goes on.
    if let Err(e) = embed_resource::compile(&rc_path, embed_resource::NONE).manifest_optional() {
        println!("cargo:warning=icon not embedded: {e}");
    }
}
