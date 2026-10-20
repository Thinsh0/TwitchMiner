//! Every `@tr("…")` string of the interface must have a non-empty French translation.

use std::collections::BTreeSet;
use std::path::Path;

fn slint_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read ui dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            slint_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "slint") {
            out.push(path);
        }
    }
}

/// Reads a quoted string starting right after the opening quote; returns it (escapes
/// kept as written, like in a .po file) and the rest.
fn quoted(s: &str) -> Option<(&str, &str)> {
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        match c {
            '\\' if !escaped => escaped = true,
            '"' if !escaped => return Some((&s[..i], &s[i + 1..])),
            _ => escaped = false,
        }
    }
    None
}

fn msgids_in_ui() -> BTreeSet<String> {
    let mut files = Vec::new();
    slint_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("ui"),
        &mut files,
    );
    let mut ids = BTreeSet::new();
    for f in files {
        let text = std::fs::read_to_string(&f).expect("read slint");
        let mut rest = text.as_str();
        while let Some(i) = rest.find("@tr(\"") {
            let (id, after) = quoted(&rest[i + 5..]).expect("unterminated @tr string");
            ids.insert(id.to_string());
            rest = after;
        }
    }
    ids
}

fn french() -> Vec<(String, String)> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("translations/fr/LC_MESSAGES/tm-ui.po");
    let text = std::fs::read_to_string(path).expect("read fr po");
    let mut pairs = Vec::new();
    let mut id: Option<String> = None;
    for line in text.lines() {
        if let Some(r) = line.strip_prefix("msgid \"") {
            id = quoted(r).map(|(s, _)| s.to_string());
        } else if let Some(r) = line.strip_prefix("msgstr \"")
            && let Some(id) = id.take()
        {
            pairs.push((
                id,
                quoted(r).map(|(s, _)| s.to_string()).unwrap_or_default(),
            ));
        }
    }
    pairs
}

#[test]
fn every_ui_string_is_translated_to_french() {
    let fr = french();
    let translated: BTreeSet<&str> = fr
        .iter()
        .filter(|(id, s)| !id.is_empty() && !s.is_empty())
        .map(|(id, _)| id.as_str())
        .collect();
    let missing: Vec<String> = msgids_in_ui()
        .into_iter()
        .filter(|id| !translated.contains(id.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "missing French translations: {missing:#?}"
    );
}

#[test]
fn french_keeps_placeholders() {
    for (id, s) in french() {
        assert_eq!(
            id.matches("{}").count(),
            s.matches("{}").count(),
            "placeholder mismatch for {id:?}"
        );
    }
}
