//! Reads GOG Galaxy 2.0 achievements from its local SQLite database.
//!
//! Galaxy keeps the database open in WAL mode, so it is opened read-only and every
//! query runs in a short-lived connection that does not hold Galaxy up.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, OptionalExtension};

use crate::achievement::{crown_platinum, game_key, Achievement, Source};

pub const DB_FILE_NAME: &str = "galaxy-2.0.db";

#[derive(Debug, Clone)]
pub struct GogPaths {
    pub db: PathBuf,
}

impl GogPaths {
    pub fn storage_dir(&self) -> PathBuf {
        self.db.parent().map(Path::to_path_buf).unwrap_or_default()
    }

    pub fn locate() -> Result<Self, String> {
        let program_data = std::env::var_os("PROGRAMDATA").map(PathBuf::from).unwrap_or(PathBuf::from(r"C:\ProgramData"));
        let db = program_data.join("GOG.com").join("Galaxy").join("storage").join(DB_FILE_NAME);
        if db.is_file() {
            Ok(Self { db })
        } else {
            Err("GOG Galaxy not found".into())
        }
    }

    fn open(&self) -> Result<Connection, String> {
        let conn = Connection::open_with_flags(
            &self.db,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| format!("cannot open {}: {e}", self.db.display()))?;
        conn.busy_timeout(Duration::from_secs(2)).map_err(|e| e.to_string())?;
        Ok(conn)
    }

    /// All unlocked achievements, newest first, with each completed game's platinum marked.
    pub fn load_all(&self) -> Result<Vec<Achievement>, String> {
        let conn = self.open()?;
        let mut all = unlocked(&conn).map_err(|e| e.to_string())?;
        let complete = completed_games(&conn).map_err(|e| e.to_string())?;
        crown_platinum(&mut all, |a| complete.contains(&a.game_id));
        all.sort_by(|a, b| b.unlocked_at.cmp(&a.unlocked_at));
        Ok(all)
    }

    /// Titles of games with unlocked achievements, keyed by [`game_key`].
    pub fn names(&self) -> Result<HashMap<String, String>, String> {
        names(&self.open()?).map_err(|e| e.to_string())
    }

    /// Store banner URL templates (`horizontalCover`) of games with unlocked achievements,
    /// keyed by [`game_key`]. See `icons::banner_data_url` for how they are filled in.
    pub fn banners(&self) -> Result<HashMap<String, String>, String> {
        banners(&self.open()?).map_err(|e| e.to_string())
    }

    /// Changes whenever an achievement is unlocked. Galaxy writes to the database for many
    /// other reasons, so the watcher compares this instead of reacting to every write.
    pub fn fingerprint(&self) -> Option<String> {
        let conn = self.open().ok()?;
        conn.query_row(
            "SELECT count(*) || '/' || coalesce(max(unlockTime), '') FROM UserAchievements WHERE isUnlocked = 1",
            [],
            |row| row.get(0),
        )
        .ok()
    }
}

/// The signed-in user: the one who used the client most recently.
fn active_user(conn: &Connection) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT u.id FROM Users u
         LEFT JOIN UserRecentClientLanguages l ON l.userId = u.id
         ORDER BY l.lastUsed DESC LIMIT 1",
        [],
        |row| row.get(0),
    )
    .optional()
}

fn unlocked(conn: &Connection) -> rusqlite::Result<Vec<Achievement>> {
    let Some(user) = active_user(conn)? else {
        return Ok(Vec::new());
    };
    // Names come in several languages. Prefer the user's client language, then US English,
    // then whatever exists.
    let mut stmt = conn.prepare(
        "SELECT ua.gameReleaseKey, ua.apikey, ua.unlockTime, a.isVisible, a.imageUnlockedUrl,
                la.name, la.description, a.rarity
         FROM UserAchievements ua
         JOIN Achievements a ON a.gameReleaseKey = ua.gameReleaseKey AND a.apikey = ua.apikey
         LEFT JOIN LocalizedAchievements la ON la.rowid = (
             SELECT l.rowid FROM LocalizedAchievements l
             WHERE l.gameReleaseKey = ua.gameReleaseKey AND l.apikey = ua.apikey
             ORDER BY l.languageId = (
                         SELECT languageId FROM UserRecentClientLanguages
                         WHERE userId = ?1 ORDER BY lastUsed DESC LIMIT 1
                      ) DESC,
                      l.languageId = (SELECT id FROM Languages WHERE code = 'en-US') DESC
             LIMIT 1
         )
         WHERE ua.isUnlocked = 1 AND ua.userId = ?1",
    )?;
    let rows = stmt.query_map([user], |row| {
        let unlock_time: Option<String> = row.get(2)?;
        let api_name: String = row.get(1)?;
        Ok(Achievement {
            source: Source::Gog,
            game_id: row.get(0)?,
            title: row.get::<_, Option<String>>(5)?.unwrap_or_else(|| api_name.clone()),
            api_name,
            description: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
            hidden: !row.get::<_, bool>(3)?,
            icon: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
            unlocked_at: unlock_time.as_deref().and_then(parse_unlock_time).unwrap_or(0),
            rarity: row.get::<_, Option<f64>>(7)?.map(|r| r as f32),
            platinum: false,
        })
    })?;
    rows.collect()
}

/// Release keys of games where the active user has unlocked every achievement Galaxy
/// defines. Each definition is matched by its API key, so an unlock of an achievement the
/// game no longer defines cannot stand in for a locked one, and a game with no definitions
/// is never complete.
fn completed_games(conn: &Connection) -> rusqlite::Result<HashSet<String>> {
    let Some(user) = active_user(conn)? else {
        return Ok(HashSet::new());
    };
    let mut stmt = conn.prepare(
        "SELECT DISTINCT a.gameReleaseKey FROM Achievements a
         WHERE NOT EXISTS (
             SELECT 1 FROM Achievements d
             WHERE d.gameReleaseKey = a.gameReleaseKey
               AND NOT EXISTS (
                   SELECT 1 FROM UserAchievements ua
                   WHERE ua.gameReleaseKey = d.gameReleaseKey AND ua.apikey = d.apikey
                     AND ua.userId = ?1 AND ua.isUnlocked = 1
               )
         )",
    )?;
    let rows = stmt.query_map([user], |row| row.get(0))?;
    rows.collect()
}

fn names(conn: &Connection) -> rusqlite::Result<HashMap<String, String>> {
    // `title` wins over `originalTitle` because it is read last.
    let mut stmt = conn.prepare(
        "SELECT gp.releaseKey, gp.value FROM GamePieces gp
         JOIN GamePieceTypes t ON t.id = gp.gamePieceTypeId
         WHERE t.type IN ('originalTitle', 'title')
           AND gp.releaseKey IN (SELECT gameReleaseKey FROM UserAchievements WHERE isUnlocked = 1)
         ORDER BY t.type = 'title'",
    )?;
    let mut names = HashMap::new();
    let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    for row in rows {
        let (key, value) = row?;
        let title = serde_json::from_str::<serde_json::Value>(&value)
            .ok()
            .and_then(|v| v.get("title")?.as_str().map(str::to_string));
        if let Some(title) = title.filter(|t| !t.trim().is_empty()) {
            names.insert(game_key(Source::Gog, &key), title);
        }
    }
    Ok(names)
}

fn banners(conn: &Connection) -> rusqlite::Result<HashMap<String, String>> {
    let mut stmt = conn.prepare(
        "SELECT gp.releaseKey, gp.value FROM GamePieces gp
         JOIN GamePieceTypes t ON t.id = gp.gamePieceTypeId
         WHERE t.type = 'storeImages'
           AND gp.releaseKey IN (SELECT gameReleaseKey FROM UserAchievements WHERE isUnlocked = 1)",
    )?;
    let mut banners = HashMap::new();
    let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    for row in rows {
        let (key, value) = row?;
        let cover = serde_json::from_str::<serde_json::Value>(&value)
            .ok()
            .and_then(|v| v.get("horizontalCover")?.as_str().map(str::to_string));
        if let Some(cover) = cover {
            banners.insert(game_key(Source::Gog, &key), cover);
        }
    }
    Ok(banners)
}

/// Parses Galaxy's `YYYY-MM-DD HH:MM:SS` (UTC) into unix seconds.
fn parse_unlock_time(text: &str) -> Option<u32> {
    let (date, time) = text.trim().split_once([' ', 'T'])?;
    let mut d = date.splitn(3, '-').map(|p| p.parse::<i64>().ok());
    let (year, month, day) = (d.next()??, d.next()??, d.next()??);
    let mut t = time.splitn(3, ':').map(|p| p.get(..2).unwrap_or(p).parse::<i64>().ok());
    let (hour, minute, second) = (t.next()??, t.next()??, t.next()??);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let secs = days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second;
    u32::try_from(secs).ok()
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tables and columns this module reads, as Galaxy 2.0 defines them (constraints trimmed).
    fn galaxy_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE Users(id INT64 NOT NULL PRIMARY KEY);
             CREATE TABLE Languages(id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, code TEXT NOT NULL);
             CREATE TABLE UserRecentClientLanguages(languageId INTEGER NOT NULL, userId INT64 NOT NULL, lastUsed TEXT NOT NULL);
             CREATE TABLE Achievements(gameReleaseKey TEXT NOT NULL, apikey TEXT NOT NULL, backendId TEXT,
                 imageUnlockedUrl TEXT, imageLockedUrl TEXT, isVisible BOOLEAN NOT NULL, rarity REAL, raritySlug TEXT);
             CREATE TABLE LocalizedAchievements(gameReleaseKey TEXT NOT NULL, apikey TEXT NOT NULL, name TEXT NOT NULL,
                 description TEXT, languageId INTEGER NOT NULL, isLocalized BOOLEAN NOT NULL);
             CREATE TABLE UserAchievements(gameReleaseKey TEXT NOT NULL, userId INT64 NOT NULL, apikey TEXT NOT NULL,
                 unlockTime TEXT, isUnlocked BOOLEAN NOT NULL);
             CREATE TABLE GamePieceTypes(id INTEGER PRIMARY KEY AUTOINCREMENT, type TEXT NOT NULL);
             CREATE TABLE GamePieces(releaseKey TEXT NOT NULL, gamePieceTypeId INTEGER NOT NULL, userId INT64,
                 value TEXT NOT NULL, languageId INTEGER);

             INSERT INTO Users VALUES (55);
             INSERT INTO Languages(id, name, code) VALUES (12, 'German', 'de-DE'), (16, 'English', 'en-US');
             INSERT INTO UserRecentClientLanguages VALUES (16, 55, '2026-10-21 21:09:34');

             INSERT INTO Achievements VALUES
                 ('gog_1', 'TheFool', NULL, 'https://images.gog.com/abc_gac_60.jpg', NULL, 1, 93.9, 'common'),
                 ('gog_1', 'Secret', NULL, 'https://images.gog.com/def_gac_60.jpg', NULL, 0, 1.0, 'rare'),
                 ('gog_1', 'Locked', NULL, NULL, NULL, 1, 5.0, 'rare');
             INSERT INTO LocalizedAchievements VALUES
                 ('gog_1', 'TheFool', 'Der Narr', 'Werde Söldner.', 12, 1),
                 ('gog_1', 'TheFool', 'The Fool', 'Become a mercenary.', 16, 0),
                 ('gog_1', 'Secret', 'Geheim', NULL, 12, 1);
             INSERT INTO UserAchievements VALUES
                 ('gog_1', 55, 'TheFool', '2022-08-07 00:27:27', 1),
                 ('gog_1', 55, 'Secret', NULL, 1),
                 ('gog_1', 55, 'Locked', NULL, 0);

             INSERT INTO GamePieceTypes(id, type) VALUES (1, 'originalTitle'), (2, 'title'), (3, 'storeImages');
             INSERT INTO GamePieces VALUES
                 ('gog_1', 1, NULL, '{\"title\":\"Cyberpunk 2077 (original)\"}', NULL),
                 ('gog_1', 2, NULL, '{\"title\":\"Cyberpunk 2077\"}', NULL),
                 ('gog_2', 2, NULL, '{\"title\":\"No unlocks\"}', NULL),
                 ('gog_1', 3, NULL, '{\"horizontalCover\":\"https://images.gog-statics.com/5a51d9_{formatter}.png\",\"logo\":\"x.png\"}', NULL),
                 ('gog_2', 3, NULL, '{\"horizontalCover\":\"https://images.gog-statics.com/ffffff_{formatter}.png\"}', NULL);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn reads_unlocked_achievements_in_the_client_language() {
        let mut list = unlocked(&galaxy_db()).unwrap();
        list.sort_by(|a, b| a.api_name.cmp(&b.api_name));
        assert_eq!(list.len(), 2, "locked achievements are skipped");

        let secret = &list[0];
        assert_eq!((secret.title.as_str(), secret.hidden, secret.unlocked_at), ("Geheim", true, 0));

        let fool = &list[1];
        assert_eq!(fool.source, Source::Gog);
        assert_eq!(fool.game_id, "gog_1");
        assert_eq!(fool.title, "The Fool");
        assert_eq!(fool.description, "Become a mercenary.");
        assert!(!fool.hidden);
        assert_eq!(fool.icon, "https://images.gog.com/abc_gac_60.jpg");
        assert_eq!(fool.unlocked_at, 1_659_832_047);
        assert_eq!(fool.rarity, Some(93.9));
    }

    #[test]
    fn completed_games_need_every_defined_achievement_unlocked() {
        let conn = galaxy_db();
        conn.execute_batch(
            "INSERT INTO Achievements VALUES
                 ('gog_3', 'One', NULL, NULL, NULL, 1, 50.0, 'common'),
                 ('gog_3', 'Two', NULL, NULL, NULL, 0, 2.0, 'rare'),
                 ('gog_4', 'Only', NULL, NULL, NULL, 1, 9.0, 'rare');
             INSERT INTO Users VALUES (77);
             INSERT INTO UserAchievements VALUES
                 ('gog_3', 55, 'One', '2023-01-01 10:00:00', 1),
                 ('gog_3', 55, 'Two', '2023-01-02 10:00:00', 1),
                 ('gog_3', 55, 'Removed', '2023-01-03 10:00:00', 1),
                 ('gog_4', 77, 'Only', '2023-01-01 10:00:00', 1),
                 ('gog_5', 55, 'Orphan', '2023-01-01 10:00:00', 1);",
        )
        .unwrap();
        // gog_1 has a locked achievement, gog_4 was finished by another user, and gog_5 has
        // no definitions at all.
        assert_eq!(completed_games(&conn).unwrap(), HashSet::from(["gog_3".to_string()]));
    }

    #[test]
    fn names_prefer_title_and_skip_games_without_unlocks() {
        let names = names(&galaxy_db()).unwrap();
        assert_eq!(names.len(), 1);
        assert_eq!(names["gog:gog_1"], "Cyberpunk 2077");
    }

    #[test]
    fn banners_read_horizontal_cover_of_games_with_unlocks() {
        let banners = banners(&galaxy_db()).unwrap();
        assert_eq!(banners.len(), 1);
        assert_eq!(banners["gog:gog_1"], "https://images.gog-statics.com/5a51d9_{formatter}.png");
    }

    #[test]
    fn parses_unlock_times() {
        assert_eq!(parse_unlock_time("1970-01-01 00:00:00"), Some(0));
        assert_eq!(parse_unlock_time("2000-03-01 12:30:15"), Some(951_913_815));
        assert_eq!(parse_unlock_time("2024-02-29 23:59:59"), Some(1_709_251_199));
        assert_eq!(parse_unlock_time("2022-08-07T00:27:27.000"), Some(1_659_832_047));
        assert_eq!(parse_unlock_time("not a date"), None);
        assert_eq!(parse_unlock_time("2022-13-01 00:00:00"), None);
    }
}
