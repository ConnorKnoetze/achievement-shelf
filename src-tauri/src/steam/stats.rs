//! Parses `UserGameStats_<account>_<appid>.bin` and joins it with the game's schema.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use super::kv::{self, Kv};
use super::paths::SteamPaths;
use super::rarity::{local_percents, local_progress, LibraryProgress};
use super::schema::{parse_schema, AchievementDef};
use crate::achievement::{crown_platinum, Achievement, Source};

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

/// Whether every achievement the game defines is unlocked, matched by stat and bit rather
/// than by count. Errs towards "not complete": a game with no definitions never is, and the
/// library cache vetoes it when it knows of more achievements than the schema (an update
/// added some the schema file does not have yet) or lists one the player lacks.
fn is_complete(defs: &[AchievementDef], unlocks: &Unlocks, library: &LibraryProgress) -> bool {
    if defs.is_empty() || !defs.iter().all(|def| unlocks.contains_key(&(def.stat_id, def.bit))) {
        return false;
    }
    if library.total.is_some_and(|total| total > defs.len()) {
        return false;
    }
    // Everything the schema knows is unlocked, so a listed name is either unknown to the
    // schema (out of date) or unlocked since Steam last wrote the cache (fine to ignore).
    let known: HashSet<&str> = defs.iter().map(|def| def.api_name.as_str()).collect();
    library.unachieved.iter().all(|name| known.contains(name.as_str()))
}

/// Unlocked achievements for one game.
pub fn load_game(paths: &SteamPaths, app_id: u32) -> Result<Vec<Achievement>, String> {
    let stats_dir = paths.stats_dir();
    let user = read_kv(&stats_dir.join(format!("UserGameStats_{}_{app_id}.bin", paths.account_id)))?;
    let schema = read_kv(&stats_dir.join(format!("UserGameStatsSchema_{app_id}.bin")))?;
    let unlocks = parse_user_stats(&user);
    let defs = parse_schema(&schema);
    let complete = is_complete(&defs, &unlocks, &local_progress(paths, app_id));
    let percents = local_percents(paths, app_id);
    let mut list: Vec<Achievement> = defs
        .into_iter()
        .filter_map(|def| {
            let time = *unlocks.get(&(def.stat_id, def.bit))?;
            Some(Achievement {
                source: Source::Steam,
                game_id: app_id.to_string(),
                rarity: percents.get(&def.api_name).copied(),
                api_name: def.api_name,
                title: def.title,
                description: def.description,
                hidden: def.hidden,
                icon: def.icon,
                unlocked_at: time,
                platinum: false,
            })
        })
        .collect();
    crown_platinum(&mut list, |_| complete);
    Ok(list)
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

    fn def(stat_id: u32, bit: u32, api_name: &str) -> AchievementDef {
        AchievementDef {
            stat_id,
            bit,
            api_name: api_name.into(),
            title: String::new(),
            description: String::new(),
            hidden: false,
            icon: String::new(),
        }
    }

    #[test]
    fn complete_only_when_every_defined_achievement_is_unlocked() {
        let defs = [def(1, 0, "A"), def(1, 1, "B")];
        let none = LibraryProgress::default();
        let one: Unlocks = [((1, 0), 10)].into();
        let both: Unlocks = [((1, 0), 10), ((1, 1), 20)].into();
        // A bit the schema no longer defines does not make up for a missing one.
        let wrong: Unlocks = [((1, 0), 10), ((2, 5), 20)].into();
        assert!(is_complete(&defs, &both, &none));
        assert!(!is_complete(&defs, &one, &none));
        assert!(!is_complete(&defs, &wrong, &none));
        assert!(!is_complete(&[], &both, &none), "a game without achievements has no platinum");
    }

    #[test]
    fn library_cache_vetoes_an_out_of_date_schema() {
        let defs = [def(1, 0, "A"), def(1, 1, "B")];
        let both: Unlocks = [((1, 0), 10), ((1, 1), 20)].into();
        let progress = |unachieved: &[&str], total| LibraryProgress {
            unachieved: unachieved.iter().map(|s| s.to_string()).collect(),
            total,
        };
        assert!(!is_complete(&defs, &both, &progress(&[], Some(3))), "more achievements than the schema has");
        assert!(!is_complete(&defs, &both, &progress(&["DLC"], None)), "a locked one the schema lacks");
        // A stale cache that still lists an unlocked achievement, or counts fewer, is ignored.
        assert!(is_complete(&defs, &both, &progress(&["B"], Some(2))));
        assert!(is_complete(&defs, &both, &progress(&[], Some(1))));
    }
}
