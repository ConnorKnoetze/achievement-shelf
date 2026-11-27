use std::fs;
use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize};

use crate::achievement::{Achievement, Source};

/// Achievements each row holds, from [`MIN_PER_ROW`] to [`MAX_PER_ROW`].
pub const DEFAULT_PER_ROW: usize = 6;
pub const MIN_PER_ROW: usize = 3;
pub const MAX_PER_ROW: usize = 10;
/// Recent and trophy rows together.
pub const MAX_LEVELS: usize = 10;
/// The trophy shelf is always shown; the recent shelf can be hidden.
pub const MIN_TROPHY_ROWS: usize = 1;
/// Told to the shelf or picker when the trophies it moved are not the ones saved.
const TROPHIES_CHANGED: &str = "The trophy shelf changed; try again.";
/// Shelf size, as a share of its normal size: at 1, a shelf of [`MAX_LEVELS`] levels reaches
/// the top of the screen. A shelf with fewer levels can be drawn larger, up to [`MAX_SCALE`] or
/// as large as fits its display, whichever is smaller. Until one is chosen, the shelf is sized
/// as a share of its display (see `default_scale` in lib.rs).
pub const MIN_SCALE: f64 = 0.5;
pub const MAX_SCALE: f64 = 3.0;

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

/// Colour theme of the app's windows. The shelf itself always stays dark.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Dark,
    Light,
    /// Follows the Windows app theme, including when it changes.
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Rows of recent achievements (0 or more).
    pub rows: usize,
    /// Rows of the trophy shelf above them (1 or more).
    pub trophy_rows: usize,
    /// Achievements in each row, recent and trophy alike.
    pub per_row: usize,
    /// Keep the shelf above other windows instead of on the desktop.
    pub on_top: bool,
    /// Name of the display the shelf sits on, as Windows reports it. `None` is the main display,
    /// which is also used while the chosen one is not connected.
    pub display: Option<String>,
    /// Shelf size, from [`MIN_SCALE`] to [`MAX_SCALE`] of its normal size. It is drawn smaller
    /// while the shelf would not fit its display at this size. `None` until the user picks a
    /// size: the shelf is then sized for its display.
    pub scale: Option<f64>,
    pub theme: Theme,
    /// Achievements the user chose for the trophy shelf: one list per trophy level, top first,
    /// each in display order. A level holds up to `per_row`; it need not be full, and the levels
    /// below it can still hold trophies. Levels past `trophy_rows` and trophies past `per_row`
    /// are kept, so removing a row or narrowing the shelf and changing it back restores them.
    pub trophy_levels: Vec<Vec<TrophyKey>>,
    /// The single list older versions saved, which filled the levels in turn. Moved into
    /// `trophy_levels` on load.
    #[serde(skip_serializing)]
    trophies: Vec<TrophyKey>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            rows: 1,
            trophy_rows: MIN_TROPHY_ROWS,
            per_row: DEFAULT_PER_ROW,
            on_top: false,
            display: None,
            scale: None,
            theme: Theme::default(),
            trophy_levels: Vec::new(),
            trophies: Vec::new(),
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        let mut settings: Self = fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        settings.clamp_rows();
        if let Some(scale) = settings.scale {
            settings.set_scale(scale);
        }
        let legacy = std::mem::take(&mut settings.trophies);
        if settings.trophy_levels.is_empty() {
            settings.trophy_levels = legacy.chunks(settings.per_row).map(<[_]>::to_vec).collect();
        }
        settings.trophy_levels.truncate(MAX_LEVELS);
        for level in &mut settings.trophy_levels {
            level.truncate(MAX_PER_ROW);
        }
        settings
    }

    fn clamp_rows(&mut self) {
        self.trophy_rows = self.trophy_rows.clamp(MIN_TROPHY_ROWS, MAX_LEVELS);
        self.rows = self.rows.min(MAX_LEVELS - self.trophy_rows);
        self.per_row = self.per_row.clamp(MIN_PER_ROW, MAX_PER_ROW);
    }

    /// Puts every setting back to its default. The chosen trophies are kept: they are the
    /// user's collection rather than a setting, and the picker can clear them.
    pub fn reset(&mut self) {
        *self = Self { trophy_levels: std::mem::take(&mut self.trophy_levels), ..Self::default() };
    }

    /// Forgets the chosen shelf scale, so the shelf is sized for its display again.
    pub fn reset_scale(&mut self) {
        self.scale = None;
    }

    /// Sets the shelf scale, kept within the limits (a bad value gives the default, sized for the display).
    pub fn set_scale(&mut self, scale: f64) {
        self.scale = scale.is_finite().then(|| scale.clamp(MIN_SCALE, MAX_SCALE));
    }

    /// Rows the window must fit: recent and trophy rows.
    pub fn total_rows(&self) -> usize {
        self.rows + self.trophy_rows
    }

    /// The saved trophies of each level shown, top first; levels without any are empty.
    pub fn shown_trophy_levels(&self) -> Vec<Vec<TrophyKey>> {
        (0..self.trophy_rows).map(|l| self.trophy_levels.get(l).cloned().unwrap_or_default()).collect()
    }

    /// Whether each level shown has no room for another trophy.
    pub fn full_trophy_levels(&self) -> Vec<bool> {
        self.shown_trophy_levels().iter().map(|level| level.len() >= self.per_row).collect()
    }

    fn has_trophy(&self, key: &TrophyKey) -> bool {
        self.trophy_levels.iter().flatten().any(|k| k == key)
    }

    /// Puts `key` at the end of the first level shown that has room. Fails when every one is full.
    pub fn add_trophy(&mut self, key: TrophyKey) -> Result<(), String> {
        if self.has_trophy(&key) {
            return Ok(());
        }
        let Some(level) =
            (0..self.trophy_rows).find(|&l| self.trophy_levels.get(l).map_or(0, Vec::len) < self.per_row)
        else {
            return Err(format!(
                "The trophy shelf holds {} achievements. Remove one, or add a trophy level.",
                self.trophy_rows * self.per_row
            ));
        };
        if self.trophy_levels.len() <= level {
            self.trophy_levels.resize_with(level + 1, Vec::new);
        }
        self.trophy_levels[level].push(key);
        Ok(())
    }

    pub fn remove_trophy(&mut self, key: &TrophyKey) {
        for level in &mut self.trophy_levels {
            level.retain(|k| k != key);
        }
    }

    pub fn clear_trophies(&mut self) {
        self.trophy_levels.clear();
    }

    /// Rearranges the trophies on the levels shown. `levels` lists, top first, the trophies the
    /// shelf or picker shows on each level; every one must be saved on a level shown, and each
    /// appears once. Saved trophies left out (missing from disk, or past `per_row`) stay on their
    /// level, after the ones listed. A level can only take more trophies while it has room.
    pub fn move_trophies(&mut self, levels: Vec<Vec<TrophyKey>>) -> Result<(), String> {
        if levels.len() > self.trophy_rows {
            return Err(TROPHIES_CHANGED.into());
        }
        let shown = self.shown_trophy_levels();
        let listed: Vec<&TrophyKey> = levels.iter().flatten().collect();
        let distinct = listed.iter().enumerate().all(|(i, key)| !listed[..i].contains(key));
        let saved = listed.iter().all(|key| shown.iter().flatten().any(|k| k == *key));
        if !distinct || !saved {
            return Err(TROPHIES_CHANGED.into());
        }
        let mut next = Vec::with_capacity(levels.len());
        for (l, level) in levels.iter().enumerate() {
            let kept = shown[l].iter().filter(|k| !listed.contains(k));
            let moved: Vec<TrophyKey> = level.iter().chain(kept).cloned().collect();
            if moved.len() > self.per_row && moved.len() > shown[l].len() {
                return Err("That trophy level is full.".into());
            }
            next.push(moved);
        }
        if self.trophy_levels.len() < next.len() {
            self.trophy_levels.resize_with(next.len(), Vec::new);
        }
        for (l, level) in next.into_iter().enumerate() {
            self.trophy_levels[l] = level;
        }
        Ok(())
    }

    /// How many recent achievements the recent rows hold.
    pub fn recent_capacity(&self) -> usize {
        self.rows * self.per_row
    }

    /// Adds (`delta` = 1) or removes (`delta` = -1) an achievement from every row, within the
    /// limits. Trophies past the new capacity are kept, as with removing a row. Returns whether
    /// anything changed.
    pub fn change_per_row(&mut self, delta: isize) -> bool {
        let per_row = self.per_row.saturating_add_signed(delta).clamp(MIN_PER_ROW, MAX_PER_ROW);
        let changed = per_row != self.per_row;
        self.per_row = per_row;
        changed
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
        assert!(settings.trophy_levels.is_empty());
        assert_eq!(settings.total_rows(), 3);
        assert_eq!(settings.per_row, DEFAULT_PER_ROW);
        assert_eq!(settings.theme, Theme::Dark, "older settings keep the dark theme");
    }

    #[test]
    fn theme_saves_as_a_word() {
        let settings: Settings = serde_json::from_str(r#"{ "theme": "system" }"#).unwrap();
        assert_eq!(settings.theme, Theme::System);
        assert_eq!(serde_json::to_string(&Theme::Light).unwrap(), r#""light""#);
    }

    #[test]
    fn loading_clamps_rows_to_the_limits() {
        let mut settings: Settings = serde_json::from_str(r#"{ "rows": 9, "trophyRows": 0 }"#).unwrap();
        settings.clamp_rows();
        assert_eq!((settings.rows, settings.trophy_rows), (9, 1));

        let mut settings: Settings = serde_json::from_str(r#"{ "rows": 9, "trophyRows": 4 }"#).unwrap();
        settings.clamp_rows();
        assert_eq!((settings.rows, settings.trophy_rows), (6, 4));

        let mut settings: Settings = serde_json::from_str(r#"{ "perRow": 40 }"#).unwrap();
        settings.clamp_rows();
        assert_eq!(settings.per_row, MAX_PER_ROW);
        settings.per_row = 0;
        settings.clamp_rows();
        assert_eq!(settings.per_row, MIN_PER_ROW);
    }

    #[test]
    fn per_row_changes_within_the_limits() {
        let mut settings = Settings { trophy_rows: 2, ..Settings::default() };
        assert!(settings.change_per_row(1));
        assert_eq!(settings.per_row, DEFAULT_PER_ROW + 1);
        while settings.change_per_row(1) {}
        assert_eq!(settings.per_row, MAX_PER_ROW);
        while settings.change_per_row(-1) {}
        assert_eq!(settings.per_row, MIN_PER_ROW);
        assert_eq!(settings.recent_capacity(), MIN_PER_ROW);
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
    fn scale_stays_within_the_limits() {
        let mut settings = Settings::default();
        assert_eq!(settings.scale, None, "the shelf starts sized for its display");
        settings.set_scale(2.5);
        assert_eq!(settings.scale, Some(2.5), "a short shelf can be drawn larger than normal");
        settings.set_scale(40.0);
        assert_eq!(settings.scale, Some(MAX_SCALE));
        settings.set_scale(0.0);
        assert_eq!(settings.scale, Some(MIN_SCALE));
        settings.set_scale(0.75);
        assert_eq!(settings.scale, Some(0.75));
        settings.set_scale(f64::NAN);
        assert_eq!(settings.scale, None, "a bad value gives the default");
        settings.set_scale(1.5);
        settings.reset_scale();
        assert_eq!(settings.scale, None, "a reset sizes the shelf for its display again");
    }

    #[test]
    fn reset_restores_defaults_but_keeps_trophies() {
        let trophy = TrophyKey { source: Source::default(), game_id: "1".into(), api_name: "WIN".into() };
        let mut settings = Settings {
            rows: 3,
            trophy_rows: 2,
            per_row: MAX_PER_ROW,
            on_top: true,
            display: Some("DISPLAY2".into()),
            scale: Some(2.0),
            theme: Theme::Light,
            trophy_levels: vec![vec![trophy.clone()]],
            trophies: Vec::new(),
        };
        settings.reset();
        let defaults = Settings::default();
        assert_eq!((settings.rows, settings.trophy_rows, settings.per_row), (defaults.rows, defaults.trophy_rows, defaults.per_row));
        assert_eq!((settings.on_top, settings.display, settings.scale, settings.theme), (false, None, None, Theme::default()));
        assert_eq!(settings.trophy_levels, vec![vec![trophy]]);
    }

    fn key(name: &str) -> TrophyKey {
        TrophyKey { source: Source::Steam, game_id: "1".into(), api_name: name.into() }
    }

    fn keys(names: &[&str]) -> Vec<TrophyKey> {
        names.iter().map(|name| key(name)).collect()
    }

    #[test]
    fn older_trophies_fill_the_levels_in_turn() {
        let dir = std::env::temp_dir().join(format!("achievement-shelf-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        let old = r#"{ "perRow": 3, "trophies": [
            { "gameId": "1", "apiName": "a" }, { "gameId": "1", "apiName": "b" },
            { "gameId": "1", "apiName": "c" }, { "gameId": "1", "apiName": "d" } ] }"#;
        fs::write(&path, old).unwrap();
        let settings = Settings::load(&path);
        assert_eq!(settings.trophy_levels, vec![keys(&["a", "b", "c"]), keys(&["d"])]);
        settings.save(&path);
        let saved = fs::read_to_string(&path).unwrap();
        assert!(!saved.contains("\"trophies\""), "only the levels are saved");
        assert_eq!(Settings::load(&path).trophy_levels, settings.trophy_levels);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn added_trophies_go_on_the_first_level_with_room() {
        let mut settings = Settings { trophy_rows: 2, per_row: 3, ..Settings::default() };
        settings.trophy_levels = vec![keys(&["a", "b", "c"])];
        settings.add_trophy(key("d")).unwrap();
        assert_eq!(settings.trophy_levels, vec![keys(&["a", "b", "c"]), keys(&["d"])]);
        settings.remove_trophy(&key("b"));
        settings.add_trophy(key("e")).unwrap();
        assert_eq!(settings.trophy_levels, vec![keys(&["a", "c", "e"]), keys(&["d"])]);
        settings.add_trophy(key("f")).unwrap();
        settings.add_trophy(key("g")).unwrap();
        assert!(settings.add_trophy(key("h")).is_err(), "every level is full");
        settings.add_trophy(key("a")).unwrap();
        assert_eq!(settings.trophy_levels.iter().flatten().count(), 6, "a trophy is only kept once");
    }

    #[test]
    fn trophies_move_to_any_level_with_room() {
        let mut settings = Settings { trophy_rows: 3, per_row: 3, ..Settings::default() };
        settings.trophy_levels = vec![keys(&["a", "b"])];
        // A level need not be full for the one below it to hold trophies.
        settings.move_trophies(vec![keys(&["a"]), vec![], keys(&["b"])]).unwrap();
        assert_eq!(settings.trophy_levels, vec![keys(&["a"]), vec![], keys(&["b"])]);
        settings.add_trophy(key("c")).unwrap();
        assert_eq!(settings.trophy_levels[0], keys(&["a", "c"]));

        settings.trophy_levels[1] = keys(&["x", "y", "z"]);
        let crowded = vec![keys(&["a"]), keys(&["x", "y", "z", "c"]), keys(&["b"])];
        assert!(settings.move_trophies(crowded).is_err(), "level 2 is full");
        assert!(settings.move_trophies(vec![keys(&["a", "a"])]).is_err(), "a trophy listed twice");
        assert!(settings.move_trophies(vec![keys(&["q"])]).is_err(), "a trophy not on the shelf");
        assert!(settings.move_trophies(vec![vec![], vec![], vec![], vec![]]).is_err(), "more levels than shown");
    }

    #[test]
    fn trophies_left_out_of_a_move_stay_on_their_level() {
        let mut settings = Settings { trophy_rows: 2, per_row: 3, ..Settings::default() };
        // "gone" is no longer found on disk, so the shelf does not list it.
        settings.trophy_levels = vec![keys(&["a", "gone", "b"]), keys(&["c"])];
        settings.move_trophies(vec![keys(&["b"]), keys(&["a", "c"])]).unwrap();
        assert_eq!(settings.trophy_levels, vec![keys(&["b", "gone"]), keys(&["a", "c"])]);
    }

    #[test]
    fn a_chosen_scale_still_loads() {
        let settings: Settings = serde_json::from_str(r#"{ "scale": 1.5 }"#).unwrap();
        assert_eq!(settings.scale, Some(1.5));
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings.scale, None);
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
