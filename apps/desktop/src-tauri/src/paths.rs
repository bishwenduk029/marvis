//! Resolve model and config paths. Keep everything under one directory so a
//! first-run downloader can populate it later.

use std::path::PathBuf;

pub fn model_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("MARVIS_MODELS") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".local/share")
        });
    base.join("marvis/models")
}

/// Path to a model file, given its name inside the models dir.
pub fn model(name: &str) -> PathBuf {
    model_dir().join(name)
}
