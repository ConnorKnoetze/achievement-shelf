//! Parses `UserGameStats_<account>_<appid>.bin` and joins it with the game's schema.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use super::kv::{self, Kv};
use super::paths::SteamPaths;
use super::schema::parse_schema;
use crate::achievement::{Achievement, Source};

/// `(stat_id, bit) -> unlock unix time` (0 when Steam recorded no time).
pub type Unlocks = HashMap<(u32, u32), u32>;

pub fn parse_user_stats(root: &Kv) -> Unlocks {
    let mut unlocks = Unlocks::new();
    let Some(cache) = root.get("cache") else {
        return unlocks;
    };
    for (stat_key, stat) in cache.entries() {
        // Non-numeric keys such as `crc` and `PendingChanges` are not stats.
        let Ok(stat_id) = stat_key.parse::<u32>() else { continue };
        let Some(data) = stat.get("data").and_then(Kv::as_int) else { continue };
        let bits = data as u32;
        let times = stat.get("AchievementTimes");
        for bit in 0..32 {
            if bits & (1 << bit) != 0 {
                let time = times
                    .and_then(|t| t.get(&bit.to_string()))
                    .and_then(Kv::as_int)
                    .unwrap_or(0);
                unlocks.insert((stat_id, bit), time as u32);
            }
        }
    }
    unlocks
}

fn read_kv(path: &Path) -> Result<Kv, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    kv::parse(&bytes)
}

/// Unlocked achievements for one game.
pub fn load_game(paths: &SteamPaths, app_id: u32) -> Result<Vec<Achievement>, String> {
    let stats_dir = paths.stats_dir();
    let user = read_kv(&stats_dir.join(format!("UserGameStats_{}_{app_id}.bin", paths.account_id)))?;
    let schema = read_kv(&stats_dir.join(format!("UserGameStatsSchema_{app_id}.bin")))?;
    let unlocks = parse_user_stats(&user);
    Ok(parse_schema(&schema)
        .into_iter()
        .filter_map(|def| {
            let time = *unlocks.get(&(def.stat_id, def.bit))?;
            Some(Achievement {
                source: Source::Steam,
                game_id: app_id.to_string(),
                api_name: def.api_name,
                title: def.title,
                description: def.description,
                hidden: def.hidden,
                icon: def.icon,
                unlocked_at: time,
            })
        })
        .collect())
}

/// App IDs that have a stats file for the active account.
pub fn game_ids(paths: &SteamPaths) -> Vec<u32> {
    let prefix = format!("UserGameStats_{}_", paths.account_id);
    let Ok(dir) = fs::read_dir(paths.stats_dir()) else {
        return Vec::new();
    };
    dir.filter_map(|entry| {
        let name = entry.ok()?.file_name().into_string().ok()?;
        name.strip_prefix(&prefix)?.strip_suffix(".bin")?.parse().ok()
    })
    .collect()
}

/// All unlocked achievements, newest first. Games that fail to parse are skipped.
pub fn load_all(paths: &SteamPaths) -> Vec<Achievement> {
    let mut all: Vec<Achievement> = game_ids(paths)
        .into_iter()
        .flat_map(|app_id| {
            load_game(paths, app_id).unwrap_or_else(|e| {
                eprintln!("achievement-shelf: skipping app {app_id}: {e}");
                Vec::new()
            })
        })
        .collect();
    all.sort_by(|a, b| b.unlocked_at.cmp(&a.unlocked_at));
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Kv {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
        read_kv(&path).unwrap()
    }

    #[test]
    fn joins_real_stats_with_schema() {
        let unlocks = parse_user_stats(&fixture("UserGameStats_1061090.bin"));
        let defs = parse_schema(&fixture("UserGameStatsSchema_1061090.bin"));
        assert!(!defs.is_empty());

        let fall_100 = defs.iter().find(|d| d.api_name == "FALL_100").unwrap();
        assert_eq!(fall_100.title, "Just the Beginning");
        assert_eq!(fall_100.description, "If only you knew... (100 Falls)");
        assert!(fall_100.icon.ends_with(".jpg"));

        // Every unlock must map to a schema entry, and every unlock in this file has a time.
        for (key, time) in &unlocks {
            assert!(defs.iter().any(|d| (d.stat_id, d.bit) == *key), "no def for {key:?}");
            assert!(*time > 1_500_000_000);
        }
        assert_eq!(unlocks.len(), 17 + 7);
    }
}
