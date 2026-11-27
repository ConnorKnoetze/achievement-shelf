//! RetroAchievements: the signed-in account's games and unlocks, from the site's Web API.
//!
//! The user gives their username and Web API key (kept in Credential Manager by
//! [`accounts`]). A background thread ([`spawn`]) copies the account's games into a cache file,
//! which the shelf reads like any launcher's files:
//!
//! - A full sync lists every game with `API_GetUserCompletionProgress` and fetches the ones whose
//!   progress changed (or whose rarity is a week old) with `API_GetGameInfoAndUserProgress`.
//! - Between full syncs, `API_GetUserRecentAchievements` asks for the latest unlocks: every few
//!   seconds while an emulator that reports to RetroAchievements runs, every few minutes otherwise.
//!
//! The API blocks a key for minutes after a burst of calls (answering 429), so the thread waits
//! that out and an interrupted sync picks up where it stopped.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::achievement::{crown_platinum, game_key, parse_utc_time, Achievement, Source};
use crate::{accounts, browser, processes, store};

/// Badges (`Badge/<name>.png`) and game images (`Images/<number>.png`) are served from here.
pub const MEDIA_HOST: &str = "https://media.retroachievements.org/";
const API_ROOT: &str = "https://retroachievements.org/API/";
const CREDENTIAL: &str = "retroachievements";
const CACHE_FILE: &str = "retroachievements.json";
const PAGE_SIZE: usize = 500;
/// Rarity drifts as other people play, so a game is fetched again after this long anyway.
const STALE_AFTER_SECS: u64 = 7 * 24 * 60 * 60;
const FULL_SYNC_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
const ACTIVE_POLL: Duration = Duration::from_secs(8);
const IDLE_POLL: Duration = Duration::from_secs(5 * 60);
/// Between the game fetches of a sync, to stay clear of the API's rate limit.
const FETCH_DELAY: Duration = Duration::from_secs(1);
const DEFAULT_RATE_PAUSE: Duration = Duration::from_secs(10 * 60);
/// How far back the first poll after a sync looks for unlocks.
const FIRST_POLL_MINUTES: u64 = 3;

/// Executable name stems of emulators that report to RetroAchievements, from its list of
/// supported emulators.
const EMULATORS: &[&str] = &[
    "retroarch", "ralibretro", "emuhawk", "rapplewin", "flycast", "winarcadia", "ranes", "ravba", "rasnes9x",
    "rap64", "raproject64", "raquasi88", "rameka", "ragens", "raoricutron", "project64", "skyemu", "rmg",
    "gopher64", "dolphin", "melonds", "mesen", "stella", "gearboy", "gearsystem", "gearcoleco", "duckstation",
    "pcsx2", "ppsspp", "xbsx2",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Account {
    username: String,
    api_key: String,
}

fn account() -> Option<Account> {
    accounts::load(CREDENTIAL).and_then(|text| serde_json::from_str(&text).ok())
}

/// The signed-in username, if any.
pub fn username() -> Option<String> {
    account().map(|a| a.username)
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
struct Cache {
    /// Whose games these are; another account's are thrown away.
    username: String,
    games: BTreeMap<u64, Game>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
struct Game {
    title: String,
    console: String,
    /// Title screen picture, used as the game's banner. Empty when the site has none.
    banner: String,
    players: u64,
    /// What the completion list said when the game was fetched; it changes with progress.
    fingerprint: String,
    fetched_at: u64,
    achievements: BTreeMap<u64, Entry>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
struct Entry {
    title: String,
    description: String,
    badge: String,
    /// Players who have earned it, in either mode.
    awarded: u64,
    /// Unix seconds, the earlier of softcore and hardcore; 0 while locked.
    unlocked_at: u32,
}

impl Game {
    fn name(&self) -> String {
        if self.console.is_empty() { self.title.clone() } else { format!("{} ({})", self.title, self.console) }
    }
}

fn cache_path(data_dir: &Path) -> PathBuf {
    data_dir.join(CACHE_FILE)
}

/// What the shelf reads: the cached games of the signed-in account.
pub struct Retro {
    cache: Cache,
}

impl Retro {
    /// The account's cached games, or `None` when nobody is signed in.
    pub fn locate(data_dir: &Path) -> Option<Self> {
        let username = username()?;
        let cache: Cache = store::load(&cache_path(data_dir));
        let cache = if cache.username.eq_ignore_ascii_case(&username) { cache } else { Cache::default() };
        Some(Self { cache })
    }

    /// All unlocked achievements, newest first, with each completed game's platinum marked.
    pub fn load_all(&self) -> Vec<Achievement> {
        achievements(&self.cache)
    }

    /// `Title (Console)` of each game, keyed by [`game_key`].
    pub fn names(&self) -> std::collections::HashMap<String, String> {
        self.cache.games.iter().map(|(id, game)| (game_key(Source::Retro, &id.to_string()), game.name())).collect()
    }

    /// Title screen URLs, keyed by [`game_key`].
    pub fn banners(&self) -> std::collections::HashMap<String, String> {
        let games = self.cache.games.iter().filter(|(_, game)| !game.banner.is_empty());
        games.map(|(id, game)| (game_key(Source::Retro, &id.to_string()), game.banner.clone())).collect()
    }
}

fn achievements(cache: &Cache) -> Vec<Achievement> {
    let mut all = Vec::new();
    let mut complete = std::collections::HashSet::new();
    for (game_id, game) in &cache.games {
        let game_id = game_id.to_string();
        for (id, entry) in &game.achievements {
            if entry.unlocked_at == 0 {
                continue;
            }
            all.push(Achievement {
                source: Source::Retro,
                game_id: game_id.clone(),
                api_name: id.to_string(),
                title: entry.title.clone(),
                description: entry.description.clone(),
                hidden: false,
                icon: badge_url(&entry.badge),
                unlocked_at: entry.unlocked_at,
                rarity: (game.players > 0).then(|| (entry.awarded as f32 / game.players as f32 * 100.0).min(100.0)),
                platinum: false,
            });
        }
        if !game.achievements.is_empty() && game.achievements.values().all(|a| a.unlocked_at > 0) {
            complete.insert(game_id);
        }
    }
    crown_platinum(&mut all, |a| complete.contains(&a.game_id));
    all.sort_by(|a, b| b.unlocked_at.cmp(&a.unlocked_at));
    all
}

fn badge_url(badge: &str) -> String {
    let safe = !badge.is_empty() && badge.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if safe { format!("{MEDIA_HOST}Badge/{badge}.png") } else { String::new() }
}

/// A full URL for a site image path such as `/Images/012345.png`. The site fills missing
/// pictures with "No Screenshot Found" images (`/Images/000000.png` to `000002.png`), which
/// are left out.
fn image_url(path: &str) -> String {
    let file = path.trim().trim_start_matches('/');
    let file = file.strip_prefix(MEDIA_HOST).unwrap_or(file);
    let Some(name) = file.strip_prefix("Images/") else { return String::new() };
    let placeholder = ["000000.png", "000001.png", "000002.png"].contains(&name);
    let safe = !name.starts_with('.') && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-');
    if placeholder || !safe { String::new() } else { format!("{MEDIA_HOST}Images/{name}") }
}

/// A number the API gives as a JSON number or as a string.
fn number(value: &Value) -> u64 {
    match value {
        Value::Number(n) => n.as_u64().unwrap_or(0),
        Value::String(s) => s.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

fn text(value: &Value) -> String {
    value.as_str().map(str::trim).unwrap_or_default().to_string()
}

/// The site's `YYYY-MM-DD HH:MM:SS` dates are UTC; 0 when missing.
fn date(value: &Value) -> u32 {
    value.as_str().and_then(parse_utc_time).unwrap_or(0)
}

/// What a completion-list row says about a game; while it stays the same, nothing is new.
fn fingerprint(row: &Value) -> String {
    ["MaxPossible", "NumAwarded", "NumAwardedHardcore", "MostRecentAwardedDate"]
        .iter()
        .map(|key| match &row[key] {
            Value::String(s) => s.clone(),
            Value::Null => String::new(),
            other => other.to_string(),
        })
        .collect::<Vec<_>>()
        .join("|")
}

/// One `API_GetGameInfoAndUserProgress` answer as a cached game, or `None` without achievements.
fn parse_game(payload: &Value, fingerprint: String, now: u64) -> Option<Game> {
    let rows: Vec<&Value> = match &payload["Achievements"] {
        Value::Object(map) => map.values().collect(),
        Value::Array(list) => list.iter().collect(),
        _ => Vec::new(),
    };
    let mut achievements = BTreeMap::new();
    for row in rows {
        let id = number(&row["ID"]);
        if id == 0 {
            continue;
        }
        let earned = [date(&row["DateEarned"]), date(&row["DateEarnedHardcore"])];
        let unlocked_at = earned.into_iter().filter(|&t| t > 0).min().unwrap_or(0);
        let title = text(&row["Title"]);
        achievements.insert(
            id,
            Entry {
                title: if title.is_empty() { format!("Achievement {id}") } else { title },
                description: text(&row["Description"]),
                badge: text(&row["BadgeName"]),
                awarded: number(&row["NumAwarded"]),
                unlocked_at,
            },
        );
    }
    if achievements.is_empty() {
        return None;
    }
    let banner = [&payload["ImageTitle"], &payload["ImageIngame"]].into_iter().map(|v| image_url(&text(v))).find(|u| !u.is_empty());
    Some(Game {
        title: text(&payload["Title"]),
        console: text(&payload["ConsoleName"]),
        banner: banner.unwrap_or_default(),
        players: number(&payload["NumDistinctPlayers"]),
        fingerprint,
        fetched_at: now,
        achievements,
    })
}

/// Puts a freshly fetched game in the cache. An unlock the cache already has keeps its earlier
/// time, and survives an answer fetched just before it.
fn merge_game(cache: &mut Cache, id: u64, mut game: Game) {
    if let Some(old) = cache.games.get(&id) {
        for (aid, entry) in &mut game.achievements {
            let Some(before) = old.achievements.get(aid).map(|e| e.unlocked_at).filter(|&t| t > 0) else { continue };
            if entry.unlocked_at == 0 || before < entry.unlocked_at {
                entry.unlocked_at = before;
            }
        }
        if game.banner.is_empty() {
            game.banner = old.banner.clone();
        }
    }
    cache.games.insert(id, game);
}

/// The unlocked achievements in `cache`, to tell whether a sync found anything new.
fn unlock_count(cache: &Cache) -> usize {
    cache.games.values().flat_map(|g| g.achievements.values()).filter(|a| a.unlocked_at > 0).count()
}

#[derive(Debug)]
enum ApiError {
    /// The key was refused.
    Unauthorized,
    /// Too many calls; wait this long.
    RateLimited(Duration),
    Other(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::Unauthorized => f.write_str("the API key was refused"),
            ApiError::RateLimited(wait) => write!(f, "rate limited for {}s", wait.as_secs()),
            ApiError::Other(e) => f.write_str(e),
        }
    }
}

/// One Web API call. The key travels in the query string, so no error may include the URL.
async fn api_get(client: &reqwest::Client, endpoint: &str, api_key: &str, params: &[(&str, String)]) -> Result<Value, ApiError> {
    let response = client
        .get(format!("{API_ROOT}{endpoint}"))
        .query(&[("y", api_key)])
        .query(params)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| ApiError::Other(e.without_url().to_string()))?;
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(ApiError::Unauthorized);
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let wait = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse().ok())
            .map_or(DEFAULT_RATE_PAUSE, Duration::from_secs);
        return Err(ApiError::RateLimited(wait));
    }
    if !status.is_success() {
        return Err(ApiError::Other(format!("{endpoint} answered {status}")));
    }
    response.json().await.map_err(|e| ApiError::Other(e.without_url().to_string()))
}

async fn fetch_game(client: &reqwest::Client, account: &Account, id: u64, fingerprint: String) -> Result<Option<Game>, ApiError> {
    let params = [("u", account.username.clone()), ("g", id.to_string())];
    let payload = api_get(client, "API_GetGameInfoAndUserProgress.php", &account.api_key, &params).await?;
    Ok(parse_game(&payload, fingerprint, now()))
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Checks the username and key with the site, signs in, and returns the username as the site
/// spells it. Signing in as someone else throws the previous account's cache away.
pub async fn connect(client: &reqwest::Client, data_dir: &Path, username: &str, api_key: &str) -> Result<String, String> {
    let (username, api_key) = (username.trim(), api_key.trim());
    if username.is_empty() || api_key.is_empty() {
        return Err("Enter your RetroAchievements username and Web API key.".into());
    }
    let profile = match api_get(client, "API_GetUserProfile.php", api_key, &[("u", username.to_string())]).await {
        Ok(profile) => profile,
        Err(ApiError::Unauthorized) => return Err("RetroAchievements didn't accept that Web API key.".into()),
        Err(ApiError::RateLimited(_)) => return Err("RetroAchievements is busy. Try again in a few minutes.".into()),
        Err(ApiError::Other(e)) => return Err(format!("Couldn't reach RetroAchievements: {e}")),
    };
    let confirmed = text(&profile["User"]);
    if confirmed.is_empty() {
        return Err("RetroAchievements has no user with that name.".into());
    }
    let account = Account { username: confirmed.clone(), api_key: api_key.to_string() };
    accounts::save(CREDENTIAL, &serde_json::to_string(&account).map_err(|e| e.to_string())?)?;
    let cache: Cache = store::load(&cache_path(data_dir));
    if !cache.username.eq_ignore_ascii_case(&confirmed) {
        let _ = std::fs::remove_file(cache_path(data_dir));
    }
    Ok(confirmed)
}

/// The site's page that shows the user's Web API key.
const KEY_PAGE: &str = "https://retroachievements.org/settings?tab=applications";

/// Opens the page with the user's Web API key in their browser.
pub fn open_key_page() -> Result<(), String> {
    browser::open(KEY_PAGE)
}

/// Signs out and forgets the account's games.
pub fn disconnect(data_dir: &Path) {
    accounts::clear(CREDENTIAL);
    let _ = std::fs::remove_file(cache_path(data_dir));
}

/// What a sync or poll did: whether unlocks changed, and the error that stopped it, if any.
struct Outcome {
    changed: bool,
    error: Option<ApiError>,
}

/// Fetches every game whose progress changed, saving after each so an interrupted sync resumes.
async fn sync_all(client: &reqwest::Client, data_dir: &Path, account: &Account) -> Outcome {
    let path = cache_path(data_dir);
    let mut cache: Cache = store::load(&path);
    if !cache.username.eq_ignore_ascii_case(&account.username) {
        cache = Cache { username: account.username.clone(), games: BTreeMap::new() };
    }
    let before = cache.clone();
    let mut rows: Vec<Value> = Vec::new();
    loop {
        let params = [("u", account.username.clone()), ("c", PAGE_SIZE.to_string()), ("o", rows.len().to_string())];
        let page = match api_get(client, "API_GetUserCompletionProgress.php", &account.api_key, &params).await {
            Ok(page) => page,
            Err(error) => return Outcome { changed: false, error: Some(error) },
        };
        let results = page["Results"].as_array().cloned().unwrap_or_default();
        let total = number(&page["Total"]) as usize;
        let done = results.len() < PAGE_SIZE;
        rows.extend(results);
        if done || rows.len() >= total {
            break;
        }
    }
    // Recently played first, so an interrupted sync has the games most likely to be shown.
    rows.sort_by(|a, b| text(&b["MostRecentAwardedDate"]).cmp(&text(&a["MostRecentAwardedDate"])));

    let mut error = None;
    for row in &rows {
        let id = number(&row["GameID"]);
        if id == 0 || number(&row["MaxPossible"]) == 0 {
            continue;
        }
        let fingerprint = fingerprint(row);
        let fresh = cache
            .games
            .get(&id)
            .is_some_and(|g| g.fingerprint == fingerprint && now().saturating_sub(g.fetched_at) < STALE_AFTER_SECS);
        if fresh {
            continue;
        }
        match fetch_game(client, account, id, fingerprint).await {
            Ok(Some(game)) => {
                merge_game(&mut cache, id, game);
                store::save(&path, &cache);
            }
            Ok(None) => {}
            Err(e @ (ApiError::RateLimited(_) | ApiError::Unauthorized)) => {
                error = Some(e);
                break;
            }
            Err(e) => eprintln!("achievement-shelf: cannot fetch RetroAchievements game {id}: {e}"),
        }
        std::thread::sleep(FETCH_DELAY);
    }
    if cache != before {
        store::save(&path, &cache);
    }
    Outcome { changed: unlock_count(&cache) != unlock_count(&before) || cache.games.len() != before.games.len(), error }
}

/// Adds the unlocks of the last `minutes` minutes, fetching any game the cache doesn't have.
async fn poll_recent(client: &reqwest::Client, data_dir: &Path, account: &Account, minutes: u64) -> Outcome {
    let params = [("u", account.username.clone()), ("m", minutes.clamp(1, 24 * 60).to_string())];
    let rows = match api_get(client, "API_GetUserRecentAchievements.php", &account.api_key, &params).await {
        Ok(Value::Array(rows)) => rows,
        Ok(_) => Vec::new(),
        Err(error) => return Outcome { changed: false, error: Some(error) },
    };
    let path = cache_path(data_dir);
    let mut cache: Cache = store::load(&path);
    if !cache.username.eq_ignore_ascii_case(&account.username) {
        // A sync has not run for this account yet; it will pick these up.
        return Outcome { changed: false, error: None };
    }
    let mut changed = false;
    let mut missing = Vec::new();
    for row in &rows {
        let (game_id, id, time) = (number(&row["GameID"]), number(&row["AchievementID"]), date(&row["Date"]));
        match cache.games.get_mut(&game_id).and_then(|g| g.achievements.get_mut(&id)) {
            Some(entry) if entry.unlocked_at == 0 || (time > 0 && time < entry.unlocked_at) => {
                entry.unlocked_at = time.max(1);
                changed = true;
            }
            Some(_) => {}
            None if game_id > 0 && !missing.contains(&game_id) => missing.push(game_id),
            None => {}
        }
    }
    let mut error = None;
    for game_id in missing {
        match fetch_game(client, account, game_id, String::new()).await {
            Ok(Some(game)) => {
                merge_game(&mut cache, game_id, game);
                changed = true;
            }
            Ok(None) => {}
            Err(e) => {
                error = Some(e);
                break;
            }
        }
    }
    if changed {
        store::save(&path, &cache);
    }
    Outcome { changed, error }
}

fn emulator_running() -> bool {
    processes::running().iter().any(|name| {
        let Some(stem) = name.strip_suffix(".exe") else { return false };
        let tool = stem.starts_with("dolphin-tool") || stem.starts_with("dolphinmemoryengine");
        !tool && EMULATORS.iter().any(|e| stem.starts_with(e))
    })
}

/// Asks the sync thread to do a full sync now, as after signing in or a manual refresh.
#[derive(Clone)]
pub struct Waker(mpsc::Sender<()>);

impl Waker {
    pub fn sync_now(&self) {
        let _ = self.0.send(());
    }
}

/// Starts the thread that keeps the cache up to date. `on_change` is called when unlocks change.
pub fn spawn(client: reqwest::Client, data_dir: PathBuf, on_change: impl Fn() + Send + 'static) -> Waker {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut next_full = Instant::now();
        let mut paused_until: Option<Instant> = None;
        let mut last_poll: Option<Instant> = None;
        loop {
            let wait = match account() {
                Some(account) => {
                    if paused_until.is_none_or(|until| Instant::now() >= until) {
                        paused_until = None;
                        let started = Instant::now();
                        let outcome = if started >= next_full {
                            let outcome = tauri::async_runtime::block_on(sync_all(&client, &data_dir, &account));
                            if outcome.error.is_none() {
                                next_full = started + FULL_SYNC_EVERY;
                            }
                            outcome
                        } else {
                            let minutes = last_poll.map_or(FIRST_POLL_MINUTES, |at| at.elapsed().as_secs() / 60 + 2);
                            tauri::async_runtime::block_on(poll_recent(&client, &data_dir, &account, minutes))
                        };
                        if outcome.error.is_none() {
                            last_poll = Some(started);
                        }
                        if outcome.changed {
                            on_change();
                        }
                        match outcome.error {
                            Some(ApiError::RateLimited(wait)) => paused_until = Some(Instant::now() + wait),
                            Some(ApiError::Unauthorized) => paused_until = Some(Instant::now() + DEFAULT_RATE_PAUSE),
                            Some(ApiError::Other(e)) => eprintln!("achievement-shelf: RetroAchievements: {e}"),
                            None => {}
                        }
                    }
                    if emulator_running() { ACTIVE_POLL } else { IDLE_POLL }
                }
                // Nobody signed in: sleep until someone does.
                None => Duration::from_secs(60 * 60),
            };
            match rx.recv_timeout(wait) {
                Ok(()) => {
                    next_full = Instant::now();
                    paused_until = None;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });
    Waker(tx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn payload() -> Value {
        json!({
            "Title": "Sonic the Hedgehog",
            "ConsoleName": "Genesis/Mega Drive",
            "ImageTitle": "/Images/000002.png",
            "ImageIngame": "/Images/085573.png",
            "NumDistinctPlayers": 200,
            "Achievements": {
                "5": { "ID": 5, "Title": "Speed", "Description": "Go fast", "BadgeName": "250336", "NumAwarded": "50",
                       "DateEarned": "2024-05-12 18:22:31", "DateEarnedHardcore": "2024-05-12 18:20:00" },
                "6": { "ID": "6", "Title": "Rings", "Description": "Collect 100 rings", "BadgeName": "250337", "NumAwarded": 10 }
            }
        })
    }

    #[test]
    fn reads_a_game_with_unlocks_and_rarity() {
        let game = parse_game(&payload(), "fp".into(), 7).unwrap();
        assert_eq!(game.name(), "Sonic the Hedgehog (Genesis/Mega Drive)");
        assert_eq!(game.banner, "https://media.retroachievements.org/Images/085573.png", "the placeholder is skipped");
        assert_eq!(game.achievements[&5].unlocked_at, 1_715_538_000, "the earlier of the two modes");
        assert_eq!(game.achievements[&6].unlocked_at, 0);

        let mut cache = Cache::default();
        merge_game(&mut cache, 1, game);
        let list = achievements(&cache);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].icon, "https://media.retroachievements.org/Badge/250336.png");
        assert_eq!(list[0].rarity, Some(25.0));
        assert!(!list[0].platinum);
    }

    #[test]
    fn a_refetch_keeps_earlier_unlocks_and_crowns_a_completed_game() {
        let mut cache = Cache::default();
        merge_game(&mut cache, 1, parse_game(&payload(), "a".into(), 1).unwrap());
        let mut later = payload();
        later["Achievements"]["5"]["DateEarned"] = Value::Null;
        later["Achievements"]["5"]["DateEarnedHardcore"] = Value::Null;
        later["Achievements"]["6"]["DateEarned"] = json!("2024-06-01 00:00:00");
        merge_game(&mut cache, 1, parse_game(&later, "b".into(), 2).unwrap());
        assert_eq!(cache.games[&1].achievements[&5].unlocked_at, 1_715_538_000);
        let list = achievements(&cache);
        assert_eq!(list.len(), 2);
        assert!(list[0].platinum && list[0].api_name == "6");
    }

    #[test]
    fn a_game_without_achievements_is_skipped() {
        assert!(parse_game(&json!({ "Title": "Empty", "Achievements": {} }), String::new(), 0).is_none());
    }

    #[test]
    fn builds_only_safe_media_urls() {
        assert_eq!(badge_url("250336"), "https://media.retroachievements.org/Badge/250336.png");
        assert_eq!(badge_url("../x"), "");
        assert_eq!(image_url("/Images/012345.png"), "https://media.retroachievements.org/Images/012345.png");
        assert_eq!(image_url("/Images/000001.png"), "");
        assert_eq!(image_url("/Images/../x.png"), "");
        assert_eq!(image_url("https://evil.example/Images/1.png"), "");
    }

    #[test]
    fn fingerprints_completion_rows() {
        let row = json!({ "MaxPossible": 30, "NumAwarded": "12", "NumAwardedHardcore": 10, "MostRecentAwardedDate": "2024-05-12T18:22:31+00:00" });
        assert_eq!(fingerprint(&row), "30|12|10|2024-05-12T18:22:31+00:00");
    }
}
