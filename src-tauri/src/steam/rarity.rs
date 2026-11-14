//! Global unlock percentages ("rarity") for Steam achievements.
//!
//! The stats files hold none. The client caches percentages for the achievements its
//! library page shows in `userdata/<account>/config/librarycache/<appid>.json`, which
//! often misses some; the public Web API has them all.

use std::collections::HashMap;
use std::fs;

use serde_json::Value;

use super::paths::SteamPaths;

/// Achievement API name -> percentage of players who unlocked it.
pub type Percents = HashMap<String, f32>;

/// Lists in the library cache's `achievements.data` with real percentages. The third list,
/// `vecAchievedHidden`, always has `flAchieved: 0`, so those come from the Web API instead.
const LIBRARY_CACHE_LISTS: [&str; 2] = ["vecHighlight", "vecUnachieved"];

/// Percentages from the library cache. Empty when the file is missing or unreadable.
pub fn local_percents(paths: &SteamPaths, app_id: u32) -> Percents {
    fs::read(paths.library_cache_path(app_id))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .map(|json| parse_library_cache(&json))
        .unwrap_or_default()
}

/// The file is an array of `[key, value]` pairs; achievements sit under `"achievements"`.
fn parse_library_cache(json: &Value) -> Percents {
    let mut percents = Percents::new();
    let Some(data) = json
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|pair| (pair.get(0)?.as_str()? == "achievements").then(|| pair.get(1)?.get("data")).flatten())
    else {
        return percents;
    };
    for list in LIBRARY_CACHE_LISTS {
        for entry in data.get(list).and_then(Value::as_array).into_iter().flatten() {
            let id = entry.get("strID").and_then(Value::as_str);
            let percent = entry.get("flAchieved").and_then(Value::as_f64);
            if let (Some(id), Some(percent)) = (id, percent) {
                percents.insert(id.to_string(), percent as f32);
            }
        }
    }
    percents
}

/// Every achievement's percentage from the public Web API. `None` on network errors and
/// for apps without achievements (the API answers those with 403).
pub async fn fetch_percents(client: &reqwest::Client, app_id: u32) -> Option<Percents> {
    let url = format!(
        "https://api.steampowered.com/ISteamUserStats/GetGlobalAchievementPercentagesForApp/v2/?gameid={app_id}"
    );
    let response = client.get(url).send().await.ok()?.error_for_status().ok()?;
    parse_global(&response.json().await.ok()?)
}

fn parse_global(json: &Value) -> Option<Percents> {
    let list = json.get("achievementpercentages")?.get("achievements")?.as_array()?;
    Some(
        list.iter()
            .filter_map(|entry| {
                let name = entry.get("name")?.as_str()?;
                // Sent as a string such as "12.3"; older responses used a number.
                let percent = match entry.get("percent")? {
                    Value::String(s) => s.parse().ok()?,
                    other => other.as_f64()? as f32,
                };
                Some((name.to_string(), percent))
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_library_cache_lists_with_real_percentages() {
        let cache = json!([
            ["achievementmap", {"version": 5, "data": "[]"}],
            ["achievements", {"version": 2, "data": {
                "vecHighlight": [{"strID": "A", "bAchieved": true, "flAchieved": 1.7999999523162842}],
                "vecAchievedHidden": [{"strID": "B", "flAchieved": 0}],
                "vecUnachieved": [{"strID": "C", "flAchieved": 0.5}, {"strID": "NoPercent"}],
                "nTotal": 4
            }}]
        ]);
        let percents = parse_library_cache(&cache);
        assert_eq!(percents.len(), 2);
        assert!((percents["A"] - 1.8).abs() < 1e-6);
        assert!(!percents.contains_key("B"), "placeholder zeros are skipped");
        assert_eq!(percents["C"], 0.5);
    }

    #[test]
    fn library_cache_without_achievements_is_empty() {
        assert!(parse_library_cache(&json!([["usernews", {}]])).is_empty());
        assert!(parse_library_cache(&json!({"achievements": {}})).is_empty());
    }

    #[test]
    fn reads_global_percentages_as_strings_or_numbers() {
        let body = json!({"achievementpercentages": {"achievements": [
            {"name": "FALL_100", "percent": "69.7"},
            {"name": "OLD", "percent": 3.25},
            {"name": "BAD", "percent": "n/a"}
        ]}});
        let percents = parse_global(&body).unwrap();
        assert_eq!(percents.len(), 2);
        assert_eq!(percents["FALL_100"], 69.7);
        assert_eq!(percents["OLD"], 3.25);
        assert!(parse_global(&json!({})).is_none());
    }
}
