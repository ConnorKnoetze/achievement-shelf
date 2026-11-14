use std::fs;
use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize};

use crate::achievement::{Achievement, Source};

pub const PER_ROW: usize = 6;
/// Recent and trophy rows together.
pub const MAX_LEVELS: usize = 10;
/// The trophy shelf is always shown; the recent shelf can be hidden.
pub const MIN_TROPHY_ROWS: usize = 1;
/// Most trophies kept in settings: every level as a trophy row except the minimum recent (none).
const MAX_STORED_TROPHIES: usize = MAX_LEVELS * PER_ROW;

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
    /// Rows of recent achievements (0 or more).
    pub rows: usize,
    /// Rows of the trophy shelf above them (1 or more).
    pub trophy_rows: usize,
    /// Keep the shelf above other windows instead of on the desktop.
    pub on_top: bool,
    /// Achievements the user chose for the trophy shelf, in display order. Ones past the
    /// trophy rows' capacity are kept, so removing a row and adding it back restores them.
    pub trophies: Vec<TrophyKey>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { rows: 1, trophy_rows: MIN_TROPHY_ROWS, on_top: false, trophies: Vec::new() }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        let mut settings: Self = fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        settings.clamp_rows();
        settings.trophies.truncate(MAX_STORED_TROPHIES);
        settings
    }

    fn clamp_rows(&mut self) {
        self.trophy_rows = self.trophy_rows.clamp(MIN_TROPHY_ROWS, MAX_LEVELS);
        self.rows = self.rows.min(MAX_LEVELS - self.trophy_rows);
    }

    /// Rows the window must fit: recent and trophy rows.
    pub fn total_rows(&self) -> usize {
        self.rows + self.trophy_rows
    }

    /// How many trophies the trophy rows hold.
    pub fn trophy_capacity(&self) -> usize {
        self.trophy_rows * PER_ROW
    }

    pub fn can_add_row(&self) -> bool {
        self.total_rows() < MAX_LEVELS
    }

    pub fn can_remove_recent_row(&self) -> bool {
        self.rows > 0
    }

    pub fn can_remove_trophy_row(&self) -> bool {
        self.trophy_rows > MIN_TROPHY_ROWS
    }

    /// Adds (`delta` = 1) or removes (`delta` = -1) a recent or trophy row, within the limits.
    /// Returns whether anything changed.
    pub fn change_rows(&mut self, trophy: bool, delta: isize) -> bool {
        let allowed = match (trophy, delta > 0) {
            (_, true) => self.can_add_row(),
            (false, false) => self.can_remove_recent_row(),
            (true, false) => self.can_remove_trophy_row(),
        };
        if allowed {
            let rows = if trophy { &mut self.trophy_rows } else { &mut self.rows };
            *rows = rows.saturating_add_signed(delta);
        }
        allowed
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
    fn older_settings_get_one_trophy_row() {
        let settings: Settings =
            serde_json::from_str(r#"{ "rows": 2, "onTop": true, "trophyShelf": false }"#).unwrap();
        assert_eq!(settings.rows, 2);
        assert_eq!(settings.trophy_rows, 1);
        assert!(settings.trophies.is_empty());
        assert_eq!(settings.total_rows(), 3);
        assert_eq!(settings.trophy_capacity(), PER_ROW);
    }

    #[test]
    fn loading_clamps_rows_to_the_limits() {
        let mut settings: Settings = serde_json::from_str(r#"{ "rows": 9, "trophyRows": 0 }"#).unwrap();
        settings.clamp_rows();
        assert_eq!((settings.rows, settings.trophy_rows), (9, 1));

        let mut settings: Settings = serde_json::from_str(r#"{ "rows": 9, "trophyRows": 4 }"#).unwrap();
        settings.clamp_rows();
        assert_eq!((settings.rows, settings.trophy_rows), (6, 4));
    }

    #[test]
    fn rows_change_within_the_limits() {
        let mut settings = Settings { rows: 0, ..Settings::default() };
        assert!(!settings.change_rows(false, -1), "recent rows stop at none");
        assert!(!settings.change_rows(true, -1), "trophy rows stop at one");

        while settings.change_rows(true, 1) {}
        assert_eq!((settings.rows, settings.trophy_rows), (0, MAX_LEVELS));
        assert!(!settings.change_rows(false, 1), "levels stop at the maximum");

        assert!(settings.change_rows(true, -1));
        assert!(settings.change_rows(false, 1));
        assert_eq!((settings.rows, settings.trophy_rows), (1, MAX_LEVELS - 1));
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
