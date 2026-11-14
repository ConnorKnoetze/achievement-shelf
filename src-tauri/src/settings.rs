use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const PER_ROW: usize = 6;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Number of shelf rows (1 or 2).
    pub rows: usize,
    /// Keep the shelf above other windows instead of on the desktop.
    pub on_top: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { rows: 1, on_top: false }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        let mut settings: Self = fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        settings.rows = settings.rows.clamp(1, 2);
        settings
    }

    pub fn save(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let result = serde_json::to_string_pretty(self)
            .map_err(|e| e.to_string())
            .and_then(|json| fs::write(path, json).map_err(|e| e.to_string()));
        if let Err(e) = result {
            eprintln!("steam-shelf: could not save settings: {e}");
        }
    }
}
