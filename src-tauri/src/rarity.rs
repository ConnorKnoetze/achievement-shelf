//! Fills in Steam rarities the library cache lacks, from the Web API, with a disk cache.
//! GOG rarities all come from its local database.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::achievement::{Achievement, Source};
use crate::steam::rarity::{fetch_percents, Percents};

/// Percentages drift slowly, so a game is asked about again after a week.
const REFRESH_AFTER_SECS: u64 = 7 * 24 * 3600;
/// A game the API had nothing for (offline, or no public stats) is retried after an hour.
const RETRY_AFTER_SECS: u64 = 3600;
/// Requests in flight at once, to stay polite to the API on a first run with many games.
const CONCURRENT_REQUESTS: usize = 8;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Entry {
    checked_at: u64,
    percents: Percents,
}

impl Entry {
    fn is_fresh(&self, now: u64) -> bool {
        let ttl = if self.percents.is_empty() { RETRY_AFTER_SECS } else { REFRESH_AFTER_SECS };
        now.saturating_sub(self.checked_at) < ttl
    }
}

pub struct RarityCache {
    path: PathBuf,
    /// Keyed by Steam app ID.
    entries: Mutex<HashMap<u32, Entry>>,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl RarityCache {
    pub fn load(path: PathBuf) -> Self {
        let entries = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Self { path, entries: Mutex::new(entries) }
    }

    /// Sets `rarity` on Steam achievements that have none, asking the API only about games
    /// whose cached answer is missing or stale.
    pub async fn fill(&self, client: &reqwest::Client, achievements: &mut [Achievement]) {
        let mut app_ids: Vec<u32> = achievements
            .iter()
            .filter(|a| a.source == Source::Steam && a.rarity.is_none())
            .filter_map(|a| a.game_id.parse().ok())
            .collect();
        app_ids.sort_unstable();
        app_ids.dedup();

        let checked_at = now();
        let stale: Vec<u32> = {
            let entries = self.entries.lock().unwrap();
            app_ids.into_iter().filter(|id| !entries.get(id).is_some_and(|e| e.is_fresh(checked_at))).collect()
        };
        for chunk in stale.chunks(CONCURRENT_REQUESTS) {
            let tasks: Vec<_> = chunk
                .iter()
                .map(|&app_id| {
                    let client = client.clone();
                    tauri::async_runtime::spawn(async move { (app_id, fetch_percents(&client, app_id).await) })
                })
                .collect();
            for task in tasks {
                let Ok((app_id, fetched)) = task.await else { continue };
                let mut entries = self.entries.lock().unwrap();
                let entry = entries.entry(app_id).or_default();
                entry.checked_at = checked_at;
                // A failed request keeps the last good answer.
                if let Some(percents) = fetched.filter(|p| !p.is_empty()) {
                    entry.percents = percents;
                }
            }
        }

        let entries = self.entries.lock().unwrap();
        for a in achievements.iter_mut().filter(|a| a.source == Source::Steam && a.rarity.is_none()) {
            a.rarity = a
                .game_id
                .parse::<u32>()
                .ok()
                .and_then(|id| entries.get(&id)?.percents.get(&a.api_name).copied());
        }
        if !stale.is_empty() {
            if let Some(dir) = self.path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Err(e) = std::fs::write(&self.path, serde_json::to_string(&*entries).unwrap_or_default()) {
                eprintln!("achievement-shelf: could not save rarity cache: {e}");
            }
        }
    }
}
