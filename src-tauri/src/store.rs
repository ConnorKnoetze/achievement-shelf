//! JSON files the app keeps for itself, such as what it has read from a rotating log or fetched
//! from a web API.

use std::path::Path;

use serde::de::DeserializeOwned;
use serde::Serialize;

/// The value in `path`, or the default when it is missing or unreadable.
pub fn load<T: DeserializeOwned + Default>(path: &Path) -> T {
    std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

/// Writes `value` through a temporary file, so a crash cannot leave half of it.
pub fn save<T: Serialize>(path: &Path, value: &T) {
    let Ok(text) = serde_json::to_string(value) else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let temp = path.with_extension("json.tmp");
    if let Err(e) = std::fs::write(&temp, text).and_then(|()| std::fs::rename(&temp, path)) {
        eprintln!("achievement-shelf: cannot save {}: {e}", path.display());
    }
}
