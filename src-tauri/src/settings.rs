use std::fs;
use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize};

use crate::achievement::{Achievement, Source};

pub const PER_ROW: usize = 6;
/// The trophy shelf is a single row.
pub const MAX_TROPHIES: usize = PER_ROW;

/// Identifies one achievement across games and launchers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrophyKey {
    /// Missing in settings saved before GOG support, which were all Steam.
    #[serde(default)]
    pub source: Source,
    /// Older settings stored this as a numeric `appId`.
    #[serde(alias = "appId", deserialize_with = "string_or_number")]
    pub game_id: String,
    pub api_name: String,
}

impl TrophyKey {
    pub fn matches(&self, achievement: &Achievement) -> bool {
        self.source == achievement.source && self.game_id == achievement.game_id && self.api_name == achievement.api_name
    }
}

fn string_or_number<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Id {
        Text(String),
        Number(u64),
    }
    Ok(match Id::deserialize(deserializer)? {
        Id::Text(text) => text,
        Id::Number(number) => number.to_string(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Number of shelf rows (1 or 2).
    pub rows: usize,
    /// Keep the shelf above other windows instead of on the desktop.
    pub on_top: bool,
    /// Show the trophy shelf row above the recent achievements.
    pub trophy_shelf: bool,
    /// Achievements the user chose for the trophy shelf, in display order.
    pub trophies: Vec<TrophyKey>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { rows: 1, on_top: false, trophy_shelf: true, trophies: Vec::new() }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        let mut settings: Self = fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        settings.rows = settings.rows.clamp(1, 2);
        settings.trophies.truncate(MAX_TROPHIES);
        settings
    }

    /// Rows the window must fit: the recent rows plus the trophy row when it is shown.
    pub fn total_rows(&self) -> usize {
        self.rows + usize::from(self.trophy_shelf)
    }

    pub fn save(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let result = serde_json::to_string_pretty(self)
            .map_err(|e| e.to_string())
            .and_then(|json| fs::write(path, json).map_err(|e| e.to_string()));
        if let Err(e) = result {
            eprintln!("achievement-shelf: could not save settings: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_settings_get_the_trophy_shelf_on() {
        let settings: Settings = serde_json::from_str(r#"{ "rows": 2, "onTop": true }"#).unwrap();
        assert_eq!(settings.rows, 2);
        assert!(settings.trophy_shelf);
        assert!(settings.trophies.is_empty());
        assert_eq!(settings.total_rows(), 3);
    }

    #[test]
    fn older_steam_trophies_still_load() {
        let settings: Settings =
            serde_json::from_str(r#"{ "trophies": [{ "appId": 1061090, "apiName": "FALL_100" }] }"#).unwrap();
        let key = &settings.trophies[0];
        assert_eq!(key.source, Source::Steam);
        assert_eq!(key.game_id, "1061090");
        assert_eq!(key.api_name, "FALL_100");

        let saved = serde_json::to_string(key).unwrap();
        assert_eq!(saved, r#"{"source":"steam","gameId":"1061090","apiName":"FALL_100"}"#);
        assert_eq!(serde_json::from_str::<TrophyKey>(&saved).unwrap(), *key);
    }
}
