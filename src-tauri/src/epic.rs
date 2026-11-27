//! Epic Games (experimental): the signed-in account's achievements, from the Epic Games Store.
//!
//! Epic has no sign-in for other apps. This signs in as the Epic Games Launcher, with the
//! launcher's public client id, the way other achievement trackers do. Epic doesn't support
//! that and can stop it at any time.
//!
//! - Signing in opens Epic's own sign-in page in a window of the app. Epic then shows a page with
//!   a one-off code, which is traded for a refresh token, kept in Credential Manager.
//! - The account's games come from Epic's library service.
//! - Achievements come from the store's GraphQL service, which sits behind a Cloudflare bot check
//!   that refuses plain HTTP clients. Those queries run inside a hidden store window, a real
//!   browser with the user's Epic session. If Epic asks for a check there, the window is shown so
//!   the user can pass it themselves.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::achievement::{crown_platinum, game_key, parse_utc_time, Achievement, Source};
use crate::{accounts, store};

/// The Epic Games Launcher's public OAuth client, `client id:secret` in base64, as shipped in
/// the launcher and used by other launchers and trackers.
const LAUNCHER_CLIENT_ID: &str = "34a02cf8f4414e29b15921876da36f9a";
const LAUNCHER_BASIC: &str = "MzRhMDJjZjhmNDQxNGUyOWIxNTkyMTg3NmRhMzZmOWE6ZGFhZmJjY2M3Mzc3NDUwMzlkZmZlNTNkOTRmYzc2Y2Y=";
const TOKEN_URL: &str = "https://account-public-service-prod03.ol.epicgames.com/account/api/oauth/token";
const LIBRARY_URL: &str = "https://library-service.live.use1a.on.epicgames.com/library/api/public/items";
const CATALOG_URL: &str = "https://catalog-public-service-prod06.ol.epicgames.com/catalog/api/shared/namespace";
/// Cookies the store sets once it has a player session, such as after its profile page opens.
const STORE_SESSION_COOKIES: &[&str] = &["EPIC_EG1", "storeTokenExpires", "bearerTokenHash"];
/// After signing in, Epic sends the window here, which shows `{"authorizationCode": …}`.
const REDIRECT_PREFIX: &str = "https://www.epicgames.com/id/api/redirect";
const STORE_URL: &str = "https://store.epicgames.com/en-US/";
const STORE_HOST: &str = "store.epicgames.com";
const LOGIN_LABEL: &str = "epic-login";
const STORE_LABEL: &str = "epic-store";
const CREDENTIAL: &str = "epic";
const CACHE_FILE: &str = "epic.json";
/// The Epic windows' own browser profile, so their cookies stay apart and signing out can
/// delete them.
const WEBVIEW_DIR: &str = "epic-webview";
/// Games asked about in one GraphQL query.
const CHUNK: usize = 20;
const SYNC_EVERY: Duration = Duration::from_secs(30 * 60);
/// Achievement lists and rarity change rarely, so a game's are fetched again after this long.
const SCHEMA_STALE_SECS: u64 = 7 * 24 * 60 * 60;
const FETCH_DELAY: Duration = Duration::from_secs(1);
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// How long the store gets to load before its window is shown, in case Epic asks for a check.
const SHOW_STORE_AFTER: Duration = Duration::from_secs(15);
const STORE_TIMEOUT: Duration = Duration::from_secs(3 * 60);
const QUERY_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Account {
    account_id: String,
    display_name: String,
    refresh_token: String,
}

fn account() -> Option<Account> {
    accounts::load(CREDENTIAL).and_then(|text| serde_json::from_str(&text).ok())
}

fn save_account(account: &Account) -> Result<(), String> {
    accounts::save(CREDENTIAL, &serde_json::to_string(account).map_err(|e| e.to_string())?)
}

/// The signed-in display name, if any.
pub fn display_name() -> Option<String> {
    account().map(|a| a.display_name)
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
struct Cache {
    /// Whose games these are; another account's are thrown away.
    account_id: String,
    /// Games with achievements, keyed by namespace (Epic's sandbox id).
    games: BTreeMap<String, Game>,
    /// Namespaces found to have no achievements, with when, so they aren't asked about each sync.
    without: BTreeMap<String, u64>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
struct Game {
    name: String,
    /// Wide store art, used as the game's banner. Empty when the catalog has none.
    #[serde(default)]
    banner: String,
    product_id: String,
    schema_at: u64,
    /// When the title and banner were last read from the catalog.
    #[serde(default)]
    catalog_at: u64,
    /// Keyed by achievement name.
    achievements: BTreeMap<String, Entry>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
struct Entry {
    title: String,
    description: String,
    icon: String,
    hidden: bool,
    rarity: Option<f32>,
    /// Unix seconds; 0 while locked, 1 when unlocked at an unknown time.
    unlocked_at: u32,
}

fn cache_path(data_dir: &Path) -> PathBuf {
    data_dir.join(CACHE_FILE)
}

/// What the shelf reads: the cached games of the signed-in account.
pub struct Epic {
    cache: Cache,
}

impl Epic {
    /// The account's cached games, or `None` when nobody is signed in.
    pub fn locate(data_dir: &Path) -> Option<Self> {
        let account = account()?;
        let cache: Cache = store::load(&cache_path(data_dir));
        let cache = if cache.account_id == account.account_id { cache } else { Cache::default() };
        Some(Self { cache })
    }

    /// All unlocked achievements, newest first, with each completed game's platinum marked.
    pub fn load_all(&self) -> Vec<Achievement> {
        achievements(&self.cache)
    }

    /// Each game's name, keyed by [`game_key`].
    pub fn names(&self) -> HashMap<String, String> {
        let games = self.cache.games.iter().filter(|(_, game)| !game.name.is_empty());
        games.map(|(id, game)| (game_key(Source::Epic, id), game.name.clone())).collect()
    }

    /// Store art URLs, keyed by [`game_key`].
    pub fn banners(&self) -> HashMap<String, String> {
        let games = self.cache.games.iter().filter(|(_, game)| !game.banner.is_empty());
        games.map(|(id, game)| (game_key(Source::Epic, id), game.banner.clone())).collect()
    }
}

fn achievements(cache: &Cache) -> Vec<Achievement> {
    let mut all = Vec::new();
    let mut complete = HashSet::new();
    for (game_id, game) in &cache.games {
        for (name, entry) in &game.achievements {
            if entry.unlocked_at == 0 {
                continue;
            }
            all.push(Achievement {
                source: Source::Epic,
                game_id: game_id.clone(),
                api_name: name.clone(),
                title: entry.title.clone(),
                description: entry.description.clone(),
                hidden: entry.hidden,
                icon: entry.icon.clone(),
                unlocked_at: entry.unlocked_at,
                rarity: entry.rarity,
                platinum: false,
            });
        }
        if !game.achievements.is_empty() && game.achievements.values().all(|a| a.unlocked_at > 0) {
            complete.insert(game_id.clone());
        }
    }
    crown_platinum(&mut all, |a| complete.contains(&a.game_id));
    all.sort_by(|a, b| b.unlocked_at.cmp(&a.unlocked_at));
    all
}

/// Whether `url` is an https image on Epic's own hosts.
pub fn is_image_url(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else { return false };
    let host = parsed.host_str().unwrap_or_default();
    parsed.scheme() == "https" && (host.ends_with(".epicgames.com") || host.ends_with(".epicgamescdn.com"))
}

fn text(value: &Value) -> String {
    value.as_str().map(str::trim).unwrap_or_default().to_string()
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[derive(Debug)]
enum SyncError {
    /// Epic refused the refresh token: the user has to sign in again.
    SignedOut,
    Other(String),
}

impl From<String> for SyncError {
    fn from(e: String) -> Self {
        SyncError::Other(e)
    }
}

struct Tokens {
    access_token: String,
    refresh_token: String,
    account_id: String,
    display_name: String,
}

async fn token_request(client: &reqwest::Client, form: &[(&str, &str)]) -> Result<Tokens, SyncError> {
    let response = client
        .post(TOKEN_URL)
        .header("Authorization", format!("basic {LAUNCHER_BASIC}"))
        .form(form)
        .send()
        .await
        .map_err(|e| SyncError::Other(e.without_url().to_string()))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or_default();
    if !status.is_success() {
        let code = text(&body["errorCode"]);
        if code.ends_with("invalid_grant") || code.contains("refresh_token") {
            return Err(SyncError::SignedOut);
        }
        if code.ends_with("corrective_action_required") {
            return Err(SyncError::Other(corrective_action(&text(&body["metadata"]["correctiveAction"]))));
        }
        return Err(SyncError::Other(format!("Epic sign-in answered {status} {code}")));
    }
    let tokens = Tokens {
        access_token: text(&body["access_token"]),
        refresh_token: text(&body["refresh_token"]),
        account_id: text(&body["account_id"]),
        display_name: text(&body["displayName"]),
    };
    if tokens.access_token.is_empty() || tokens.account_id.is_empty() {
        return Err(SyncError::Other("Epic sign-in gave an unexpected answer".into()));
    }
    Ok(tokens)
}

/// What to tell the user when Epic wants something done on its website before signing in.
fn corrective_action(action: &str) -> String {
    let what = match action {
        "PRIVACY_POLICY_ACCEPTANCE" => "accept its updated privacy policy",
        "DATE_OF_BIRTH" => "confirm your date of birth",
        "DISPLAY_NAME_CHANGE" => "choose a display name",
        "EMAIL_VERIFICATION" => "verify your email address",
        _ => "update your account",
    };
    format!("Epic needs you to {what} first. Sign in at epicgames.com, do that, then sign in here again.")
}

/// A fresh access token from the stored refresh token, saving the one Epic hands back.
async fn renew(client: &reqwest::Client, account: &mut Account) -> Result<String, SyncError> {
    let tokens = token_request(client, &[("grant_type", "refresh_token"), ("refresh_token", &account.refresh_token), ("token_type", "eg1")]).await?;
    if !tokens.refresh_token.is_empty() && tokens.refresh_token != account.refresh_token {
        account.refresh_token = tokens.refresh_token;
        if let Err(e) = save_account(account) {
            eprintln!("achievement-shelf: Epic: {e}");
        }
    }
    Ok(tokens.access_token)
}

/// A game in the account's library.
struct LibraryGame {
    namespace: String,
    /// The catalog item, for the title and artwork.
    item_id: String,
    name: String,
}

/// The account's games, one per namespace.
async fn library(client: &reqwest::Client, access_token: &str) -> Result<Vec<LibraryGame>, String> {
    let mut games: BTreeMap<String, LibraryGame> = BTreeMap::new();
    let mut cursor = String::new();
    for _ in 0..50 {
        let mut request = client.get(LIBRARY_URL).bearer_auth(access_token).query(&[("includeMetadata", "true")]);
        if !cursor.is_empty() {
            request = request.query(&[("cursor", &cursor)]);
        }
        let response = request.send().await.map_err(|e| e.without_url().to_string())?;
        if !response.status().is_success() {
            return Err(format!("Epic's library answered {}", response.status()));
        }
        let page: Value = response.json().await.map_err(|e| e.without_url().to_string())?;
        for record in page["records"].as_array().map(Vec::as_slice).unwrap_or_default() {
            let namespace = text(&record["namespace"]);
            // `ue` holds Unreal Engine marketplace assets, not games.
            if namespace.is_empty() || namespace == "ue" {
                continue;
            }
            let name = [text(&record["sandboxName"]), text(&record["appName"])].into_iter().find(|n| !n.is_empty()).unwrap_or_default();
            let item_id = text(&record["catalogItemId"]);
            games.entry(namespace.clone()).or_insert(LibraryGame { namespace, item_id, name });
        }
        let next = text(&page["responseMetadata"]["nextCursor"]);
        if next.is_empty() || next == cursor {
            break;
        }
        cursor = next;
    }
    Ok(games.into_values().collect())
}

/// Wide store art, best first, for a game's banner.
const BANNER_TYPES: &[&str] = &["DieselGameBox", "OfferImageWide", "DieselStoreFrontWide", "Featured"];

/// A catalog item's title and wide art, from Epic's catalog service.
async fn catalog_item(client: &reqwest::Client, access_token: &str, namespace: &str, item_id: &str) -> Result<(String, String), String> {
    let id_ok = |id: &str| !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric());
    if !id_ok(namespace) || !id_ok(item_id) {
        return Err("unexpected catalog id".into());
    }
    let url = format!("{CATALOG_URL}/{namespace}/bulk/items");
    let response = client
        .get(url)
        .bearer_auth(access_token)
        .query(&[("id", item_id), ("country", "US"), ("locale", "en-US"), ("includeMainGameDetails", "true")])
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    if !response.status().is_success() {
        return Err(format!("Epic's catalog answered {}", response.status()));
    }
    let items: Value = response.json().await.map_err(|e| e.without_url().to_string())?;
    Ok(parse_catalog_item(&items[item_id]))
}

fn parse_catalog_item(item: &Value) -> (String, String) {
    let images = item["keyImages"].as_array().map(Vec::as_slice).unwrap_or_default();
    let banner = BANNER_TYPES
        .iter()
        .find_map(|kind| images.iter().find(|image| text(&image["type"]) == *kind))
        .map(|image| text(&image["url"]))
        .filter(|url| is_image_url(url))
        .unwrap_or_default();
    (text(&item["title"]), banner)
}

fn webview_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(WEBVIEW_DIR)
}

/// Runs `js` in `window` and returns its value. The script must return its result directly; a
/// promise comes back empty.
fn eval(window: &WebviewWindow, js: &str) -> Result<Value, String> {
    let (tx, rx) = mpsc::channel();
    window
        .eval_with_callback(js, move |out| {
            let _ = tx.send(out);
        })
        .map_err(|e| e.to_string())?;
    let out = rx.recv_timeout(Duration::from_secs(10)).map_err(|_| "the Epic window didn't answer".to_string())?;
    Ok(serde_json::from_str(&out).unwrap_or(Value::Null))
}

/// The hidden store window, opened and loaded, with any check Epic asks for passed.
fn store_window(app: &AppHandle, data_dir: &Path) -> Result<WebviewWindow, String> {
    let window = match app.get_webview_window(STORE_LABEL) {
        Some(window) => window,
        None => WebviewWindowBuilder::new(app, STORE_LABEL, WebviewUrl::External(STORE_URL.parse().map_err(|e| format!("{e}"))?))
            .title("Epic Games Store - Achievement Shelf")
            .inner_size(1000.0, 760.0)
            .center()
            .visible(false)
            .skip_taskbar(true)
            .data_directory(webview_dir(data_dir))
            .build()
            .map_err(|e| format!("Couldn't open the Epic store: {e}"))?,
    };
    wait_for_store(&window)?;
    Ok(window)
}

/// Waits until `window` shows the store, with any check Epic asks for passed.
fn wait_for_store(window: &WebviewWindow) -> Result<(), String> {
    let started = Instant::now();
    let mut shown = false;
    while started.elapsed() < STORE_TIMEOUT {
        let state = eval(window, "[location.hostname, document.readyState, document.title].join('|')").unwrap_or_default();
        let state = state.as_str().unwrap_or_default();
        let mut parts = state.splitn(3, '|');
        let (host, ready, title) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        if host == STORE_HOST && ready == "complete" && !title.to_lowercase().contains("just a moment") {
            if shown {
                let _ = window.hide();
            }
            return Ok(());
        }
        if !shown && started.elapsed() > SHOW_STORE_AFTER {
            eprintln!("achievement-shelf: Epic: the store is still loading ({title}); showing its window");
            let _ = window.set_skip_taskbar(false);
            let _ = window.show();
            let _ = window.set_focus();
            shown = true;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    let _ = window.destroy();
    Err("Epic's store didn't load.".into())
}

static NEXT_QUERY: AtomicU64 = AtomicU64::new(1);

/// Runs a GraphQL query from inside the store page, so it carries the page's session and passes
/// Epic's bot check like the store itself.
fn graphql(window: &WebviewWindow, access_token: Option<&str>, query: &str, variables: &Value) -> Result<Value, String> {
    let id = NEXT_QUERY.fetch_add(1, Ordering::SeqCst);
    let body = serde_json::to_string(&json!({ "query": query, "variables": variables })).map_err(|e| e.to_string())?;
    // Both values go in as JSON string literals, so nothing in them can break out of the script.
    let body = serde_json::to_string(&body).map_err(|e| e.to_string())?;
    // Without a token the request carries only the page's own cookies.
    let auth = match access_token {
        Some(token) => format!(",'Authorization':{}", serde_json::to_string(&format!("bearer {token}")).map_err(|e| e.to_string())?),
        None => String::new(),
    };
    let start = format!(
        "(function(){{var s=window.__shelf=window.__shelf||{{}};\
         fetch('/graphql',{{method:'POST',credentials:'include',headers:{{'Content-Type':'application/json'{auth}}},body:{body}}})\
         .then(function(r){{return r.text().then(function(t){{s[{id}]={{status:r.status,body:t}};}});}})\
         .catch(function(e){{s[{id}]={{status:0,body:String(e)}};}});return true;}})()"
    );
    eval(window, &start)?;
    let collect = format!("(function(){{var s=window.__shelf||{{}};var r=s[{id}];if(!r)return null;delete s[{id}];return r;}})()");
    let started = Instant::now();
    while started.elapsed() < QUERY_TIMEOUT {
        std::thread::sleep(Duration::from_millis(300));
        let answer = eval(window, &collect)?;
        if answer.is_null() {
            continue;
        }
        let status = answer["status"].as_u64().unwrap_or(0);
        let body = answer["body"].as_str().unwrap_or_default();
        if status != 200 {
            return Err(format!("Epic's store answered {status}: {}", body.chars().take(600).collect::<String>()));
        }
        let parsed: Value = serde_json::from_str(body).map_err(|_| "Epic's store gave an unexpected answer".to_string())?;
        if let Some(error) = parsed["errors"].as_array().and_then(|e| e.first()) {
            eprintln!("achievement-shelf: Epic: GraphQL said {}", text(&error["message"]));
        }
        return Ok(parsed);
    }
    Err("Epic's store didn't answer in time".into())
}

/// One aliased field per item, each with its own variable, so values never go in the query text.
fn aliased_query(prefix: &str, variable_type: &str, count: usize, field: impl Fn(usize) -> String) -> (String, String) {
    let declarations = (0..count).map(|i| format!("${prefix}{i}: {variable_type}")).collect::<Vec<_>>().join(", ");
    let fields = (0..count).map(field).collect::<Vec<_>>().join(" ");
    (declarations, fields)
}

const SCHEMA_FIELDS: &str = "productId sandboxId totalAchievements achievements { achievement { name unlockedDisplayName unlockedDescription unlockedIconLink hidden rarity { percent } } }";
const PLAYER_FIELDS: &str = "__typename ... on PlayerProductAchievementsResponseSuccess { data { playerAchievements { playerAchievement { achievementName unlocked unlockDate } } } }";

fn schema_query(count: usize) -> String {
    let (declarations, fields) = aliased_query("s", "String!", count, |i| format!("a{i}: productAchievementsRecordBySandbox(sandboxId: $s{i}, locale: $locale) {{ {SCHEMA_FIELDS} }}"));
    format!("query ShelfSchemas($locale: String!, {declarations}) {{ Achievement {{ {fields} }} }}")
}

fn player_query(count: usize) -> String {
    let (declarations, fields) = aliased_query("p", "String!", count, |i| format!("a{i}: productAchievements(productId: $p{i}) {{ {PLAYER_FIELDS} }}"));
    format!("query ShelfPlayer($account: String!, {declarations}) {{ PlayerProfile {{ playerProfile(epicAccountId: $account) {{ {fields} }} }} }}")
}

/// A game's achievement list from one schema record, or `None` when it has none.
fn parse_schema(record: &Value, name: &str) -> Option<Game> {
    let rows = record["achievements"].as_array()?;
    let mut achievements = BTreeMap::new();
    for row in rows {
        let a = &row["achievement"];
        let id = text(&a["name"]);
        if id.is_empty() {
            continue;
        }
        let icon = text(&a["unlockedIconLink"]);
        achievements.insert(
            id.clone(),
            Entry {
                title: Some(text(&a["unlockedDisplayName"])).filter(|t| !t.is_empty()).unwrap_or(id),
                description: text(&a["unlockedDescription"]),
                icon: if is_image_url(&icon) { icon } else { String::new() },
                hidden: a["hidden"].as_bool().unwrap_or(false),
                rarity: a["rarity"]["percent"].as_f64().map(|p| p.clamp(0.0, 100.0) as f32),
                unlocked_at: 0,
            },
        );
    }
    if achievements.is_empty() {
        return None;
    }
    Some(Game { name: name.to_string(), product_id: text(&record["productId"]), schema_at: now(), achievements, ..Game::default() })
}

/// Marks a game's unlocks from one player record. An unlock the cache already has keeps its
/// earlier time. Returns whether anything changed.
fn apply_player(game: &mut Game, record: &Value) -> bool {
    let rows = record["data"]["playerAchievements"].as_array().map(Vec::as_slice).unwrap_or_default();
    let mut changed = false;
    for row in rows {
        let p = &row["playerAchievement"];
        if p["unlocked"].as_bool() != Some(true) {
            continue;
        }
        let Some(entry) = game.achievements.get_mut(&text(&p["achievementName"])) else { continue };
        let time = p["unlockDate"].as_str().and_then(parse_utc_time).filter(|&t| t > 0).unwrap_or(1);
        if entry.unlocked_at == 0 || (time > 1 && time < entry.unlocked_at) {
            entry.unlocked_at = time;
            changed = true;
        }
    }
    changed
}

/// Whether the store itself is signed in. Signing in to the account site isn't enough: the store
/// sets its own session cookie when it is visited signed in, and only answers for the player then.
fn store_signed_in(window: &WebviewWindow) -> bool {
    let url: reqwest::Url = STORE_URL.parse().expect("a valid URL");
    window.cookies_for_url(url).is_ok_and(|cookies| cookies.iter().any(|c| STORE_SESSION_COOKIES.contains(&c.name())))
}

/// Waits up to `limit` for the store to be signed in. Not on the main thread: reading cookies
/// there deadlocks on Windows.
fn wait_for_store_session(window: &WebviewWindow, limit: Duration) -> bool {
    let started = Instant::now();
    loop {
        if store_signed_in(window) {
            return true;
        }
        if started.elapsed() >= limit {
            return false;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// Gives the store page a moment to set up its session (see [`store_signed_in`]). Only logs when
/// it doesn't: whether Epic answers for the player is what counts, and [`sync`] retries through
/// the profile page when it doesn't.
fn settle_store(window: &WebviewWindow) {
    if !wait_for_store_session(window, Duration::from_secs(5)) {
        eprintln!("achievement-shelf: Epic: the store has no session cookie yet");
    }
}

/// Opens the account's own profile page in the store window. The store has been seen to set up
/// its player session there, after which it answers for the player.
fn visit_profile(window: &WebviewWindow, account_id: &str) -> Result<(), String> {
    if account_id.is_empty() || !account_id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("unexpected account id".into());
    }
    let url: reqwest::Url = format!("https://{STORE_HOST}/u/{account_id}").parse().map_err(|e| format!("{e}"))?;
    window.navigate(url).map_err(|e| e.to_string())?;
    std::thread::sleep(Duration::from_secs(3));
    wait_for_store(window)?;
    wait_for_store_session(window, Duration::from_secs(15));
    Ok(())
}

/// Whether a player answer has no game Epic answered for: every one a refusal (`ServiceError`).
fn all_refused(profile: &Value, count: usize) -> bool {
    count > 0 && (0..count).all(|i| profile[format!("a{i}")]["__typename"].as_str() != Some("PlayerProductAchievementsResponseSuccess"))
}

/// Epic's sign-in page, going back to the store once signed in.
fn store_login_url() -> reqwest::Url {
    reqwest::Url::parse_with_params("https://www.epicgames.com/id/login", &[("redirectUrl", STORE_URL)]).expect("a valid URL")
}

/// Brings the cache up to date: the library, achievement lists that are missing or stale, every
/// game's unlocks, then titles and banners from the catalog. Returns whether anything changed.
async fn sync(app: &AppHandle, client: &reqwest::Client, data_dir: &Path, account: &mut Account) -> Result<bool, SyncError> {
    let access_token = renew(client, account).await?;
    let games = library(client, &access_token).await?;
    let path = cache_path(data_dir);
    let mut cache: Cache = store::load(&path);
    if cache.account_id != account.account_id {
        cache = Cache { account_id: account.account_id.clone(), ..Cache::default() };
    }
    let before = cache.clone();
    let fresh = |at: u64| now().saturating_sub(at) < SCHEMA_STALE_SECS;
    let stale: Vec<&LibraryGame> = games
        .iter()
        .filter(|g| {
            !cache.games.get(&g.namespace).is_some_and(|c| fresh(c.schema_at)) && !cache.without.get(&g.namespace).is_some_and(|&at| fresh(at))
        })
        .collect();

    let window = store_window(app, data_dir)?;
    settle_store(&window);
    let result = (|| -> Result<(), String> {
        for chunk in stale.chunks(CHUNK) {
            let mut variables = json!({ "locale": "en-US" });
            for (i, game) in chunk.iter().enumerate() {
                variables[format!("s{i}")] = json!(game.namespace);
            }
            let answer = graphql(&window, None, &schema_query(chunk.len()), &variables)?;
            for (i, listed) in chunk.iter().enumerate() {
                let ns = &listed.namespace;
                match parse_schema(&answer["data"]["Achievement"][format!("a{i}")], &listed.name) {
                    Some(mut game) => {
                        if let Some(old) = cache.games.get(ns) {
                            for (id, entry) in &mut game.achievements {
                                entry.unlocked_at = old.achievements.get(id).map_or(0, |e| e.unlocked_at);
                            }
                            game.name = old.name.clone();
                            game.banner = old.banner.clone();
                            game.catalog_at = old.catalog_at;
                        }
                        cache.games.insert(ns.clone(), game);
                        cache.without.remove(ns);
                    }
                    None => {
                        cache.without.insert(ns.clone(), now());
                    }
                }
            }
            store::save(&path, &cache);
            std::thread::sleep(FETCH_DELAY);
        }

        let with_products: Vec<String> = cache.games.iter().filter(|(_, g)| !g.product_id.is_empty()).map(|(ns, _)| ns.clone()).collect();
        let mut visited_profile = false;
        for chunk in with_products.chunks(CHUNK) {
            let mut variables = json!({ "account": account.account_id });
            for (i, ns) in chunk.iter().enumerate() {
                variables[format!("p{i}")] = json!(cache.games[ns].product_id);
            }
            // A game the account has never played answers with a ServiceError, which leaves it locked.
            let mut answer = graphql(&window, None, &player_query(chunk.len()), &variables)?;
            if all_refused(&answer["data"]["PlayerProfile"]["playerProfile"], chunk.len()) && !visited_profile {
                // Every game refused: the store has no player session yet. Its profile page sets one up.
                visited_profile = true;
                eprintln!("achievement-shelf: Epic: the store refused every game; opening the profile page and asking again");
                visit_profile(&window, &account.account_id)?;
                answer = graphql(&window, None, &player_query(chunk.len()), &variables)?;
                if all_refused(&answer["data"]["PlayerProfile"]["playerProfile"], chunk.len()) {
                    eprintln!("achievement-shelf: Epic: the store still refused every game");
                }
            }
            let profile = &answer["data"]["PlayerProfile"]["playerProfile"];
            for (i, ns) in chunk.iter().enumerate() {
                if let Some(game) = cache.games.get_mut(ns) {
                    apply_player(game, &profile[format!("a{i}")]);
                }
            }
            std::thread::sleep(FETCH_DELAY);
        }
        Ok(())
    })();
    let _ = window.destroy();

    // Titles and banners for games with achievements, when missing or a week old.
    for listed in &games {
        let Some(game) = cache.games.get(&listed.namespace) else { continue };
        if fresh(game.catalog_at) || listed.item_id.is_empty() {
            continue;
        }
        match catalog_item(client, &access_token, &listed.namespace, &listed.item_id).await {
            Ok((title, banner)) => {
                let game = cache.games.get_mut(&listed.namespace).expect("checked above");
                if !title.is_empty() {
                    game.name = title;
                }
                if !banner.is_empty() {
                    game.banner = banner;
                }
                game.catalog_at = now();
            }
            Err(e) => eprintln!("achievement-shelf: Epic: no catalog details for {}: {e}", listed.name),
        }
    }

    let changed = cache != before;
    if changed {
        store::save(&path, &cache);
    }
    result?;
    Ok(changed)
}

/// The sign-in waiting for its window, so it can be cancelled.
type SignInSender = Arc<Mutex<Option<mpsc::Sender<Result<String, String>>>>>;

fn finish(sender: &SignInSender, result: Result<String, String>) {
    if let Some(tx) = sender.lock().unwrap().take() {
        let _ = tx.send(result);
    }
}

/// What Epic's redirect page said.
#[derive(Debug, PartialEq)]
enum CodePage {
    Code(String),
    /// Valid, but without a code yet.
    NoCode,
    Unexpected,
}

/// The authorization code from the text of Epic's redirect page.
fn parse_code(page: &str) -> CodePage {
    let Ok(body) = serde_json::from_str::<Value>(page.trim()) else { return CodePage::Unexpected };
    let from_redirect = || {
        let url = reqwest::Url::parse(&text(&body["redirectUrl"])).ok()?;
        let code = url.query_pairs().find(|(k, _)| k == "code").map(|(_, v)| v.into_owned());
        code.filter(|c| !c.is_empty())
    };
    match text(&body["authorizationCode"]) {
        code if !code.is_empty() => CodePage::Code(code),
        _ => from_redirect().map_or(CodePage::NoCode, CodePage::Code),
    }
}

/// The redirect page with any codes or session ids masked, for the log.
fn masked(page: &str) -> String {
    match serde_json::from_str::<Value>(page.trim()) {
        Ok(Value::Object(map)) => {
            let shown: serde_json::Map<String, Value> = map
                .into_iter()
                .map(|(k, v)| {
                    let secret = ["code", "sid", "token"].iter().any(|s| k.to_lowercase().contains(s));
                    let v = match v {
                        Value::Null => v,
                        _ if secret => json!("<hidden>"),
                        // A URL's query may carry a code too.
                        Value::String(url) if url.contains('?') => json!(format!("{}?<hidden>", url.split('?').next().unwrap_or_default())),
                        v => v,
                    };
                    (k, v)
                })
                .collect();
            Value::Object(shown).to_string()
        }
        _ => format!("not JSON: {}", page.chars().take(200).collect::<String>()),
    }
}

/// Times the redirect page is loaded again when it comes without a code.
const CODE_RETRIES: u64 = 3;

/// Opens Epic's sign-in page in a window of the app and returns the display name once signed in.
/// Signing in as someone else throws the previous account's cache away.
pub async fn connect(app: &AppHandle, client: &reqwest::Client, data_dir: &Path) -> Result<String, String> {
    if let Some(window) = app.get_webview_window(LOGIN_LABEL) {
        let _ = window.destroy();
    }
    // Sign in with the store as where to go back, so the store gets its own session too (see
    // `store_signed_in`), then fetch the launcher's code page from there.
    let redirect: reqwest::Url = format!("{REDIRECT_PREFIX}?clientId={LAUNCHER_CLIENT_ID}&responseType=code").parse().map_err(|e| format!("{e}"))?;
    let login = store_login_url();
    let on_store = Arc::new(std::sync::atomic::AtomicBool::new(false));

    let (tx, rx) = mpsc::channel();
    let sender: SignInSender = Arc::new(Mutex::new(Some(tx)));
    let on_load = sender.clone();
    let on_close = sender.clone();
    let retries = Arc::new(AtomicU64::new(0));
    let window = WebviewWindowBuilder::new(app, LOGIN_LABEL, WebviewUrl::External(login))
        .title("Sign in to Epic Games - Achievement Shelf")
        .inner_size(540.0, 780.0)
        .center()
        .data_directory(webview_dir(data_dir))
        .on_page_load(move |window, payload| {
            if payload.event() != PageLoadEvent::Finished {
                return;
            }
            if payload.url().host_str() == Some(STORE_HOST) {
                if !on_store.swap(true, Ordering::SeqCst) {
                    // On to the code page. The store's player session is set up later, by the
                    // sync opening the profile page when it needs to.
                    let (window, redirect) = (window.clone(), redirect.clone());
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_secs(1));
                        let _ = window.navigate(redirect);
                    });
                }
                return;
            }
            if !payload.url().as_str().starts_with(REDIRECT_PREFIX) {
                return;
            }
            let sender = on_load.clone();
            let retries = retries.clone();
            let reload = window.clone();
            let read = window.eval_with_callback("document.body ? document.body.innerText : ''", move |out| {
                let page: String = serde_json::from_str(&out).unwrap_or_default();
                match parse_code(&page) {
                    CodePage::Code(code) => finish(&sender, Ok(code)),
                    outcome => {
                        eprintln!("achievement-shelf: Epic: the sign-in page said {}", masked(&page));
                        if retries.fetch_add(1, Ordering::SeqCst) < CODE_RETRIES {
                            // Epic sometimes shows the page before the sign-in has settled.
                            let reload = reload.clone();
                            std::thread::spawn(move || {
                                std::thread::sleep(Duration::from_secs(2));
                                let _ = reload.eval("location.reload()");
                            });
                        } else if outcome == CodePage::NoCode {
                            finish(&sender, Err("Epic gave no sign-in code. Try again.".into()));
                        } else {
                            finish(&sender, Err("Epic showed an unexpected page.".into()));
                        }
                    }
                }
            });
            if let Err(e) = read {
                finish(&on_load, Err(format!("Couldn't read Epic's sign-in page: {e}")));
            }
        })
        .build()
        .map_err(|e| format!("Couldn't open Epic's sign-in page: {e}"))?;
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            finish(&on_close, Err("Sign-in cancelled.".into()));
        }
    });

    let code = tauri::async_runtime::spawn_blocking(move || rx.recv_timeout(SIGN_IN_TIMEOUT).unwrap_or_else(|_| Err("Sign-in timed out.".into())))
        .await
        .map_err(|e| e.to_string())?;
    if let Some(window) = app.get_webview_window(LOGIN_LABEL) {
        let _ = window.destroy();
    }
    let code = code?;
    let tokens = token_request(client, &[("grant_type", "authorization_code"), ("code", &code), ("token_type", "eg1")])
        .await
        .map_err(|e| match e {
            SyncError::SignedOut => "Epic didn't accept the sign-in. Try again.".to_string(),
            SyncError::Other(e) => format!("Couldn't sign in: {e}"),
        })?;
    let display_name = Some(tokens.display_name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| "your Epic account".into());
    save_account(&Account { account_id: tokens.account_id.clone(), display_name: display_name.clone(), refresh_token: tokens.refresh_token })?;
    let cache: Cache = store::load(&cache_path(data_dir));
    if cache.account_id != tokens.account_id {
        let _ = std::fs::remove_file(cache_path(data_dir));
    }
    Ok(display_name)
}

/// Stops a sign-in by closing its window.
pub fn cancel_sign_in(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LOGIN_LABEL) {
        let _ = window.destroy();
    }
}

/// Signs out: forgets the account, its games, and the Epic windows' cookies.
pub fn disconnect(app: &AppHandle, data_dir: &Path) {
    accounts::clear(CREDENTIAL);
    let _ = std::fs::remove_file(cache_path(data_dir));
    for label in [LOGIN_LABEL, STORE_LABEL] {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.clear_all_browsing_data();
            let _ = window.destroy();
        }
    }
    // The browser may still hold the folder for a moment after its window closes; then the
    // cookies are cleared from it on the next sign-out instead.
    let _ = std::fs::remove_dir_all(webview_dir(data_dir));
}

/// Asks the sync thread to sync now, as after signing in or a manual refresh.
#[derive(Clone)]
pub struct Waker(mpsc::Sender<()>);

impl Waker {
    pub fn sync_now(&self) {
        let _ = self.0.send(());
    }
}

/// Starts the thread that keeps the cache up to date. `on_change` is called when the achievements
/// change, including when Epic ends the sign-in.
pub fn spawn(app: AppHandle, client: reqwest::Client, data_dir: PathBuf, on_change: impl Fn() + Send + 'static) -> Waker {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || loop {
        let wait = match account() {
            Some(mut account) => {
                match tauri::async_runtime::block_on(sync(&app, &client, &data_dir, &mut account)) {
                    Ok(true) => on_change(),
                    Ok(false) => {}
                    Err(SyncError::SignedOut) => {
                        eprintln!("achievement-shelf: Epic: the sign-in ended; sign in again in Settings");
                        disconnect(&app, &data_dir);
                        on_change();
                    }
                    Err(SyncError::Other(e)) => eprintln!("achievement-shelf: Epic: {e}"),
                }
                SYNC_EVERY
            }
            None => Duration::from_secs(60 * 60),
        };
        match rx.recv_timeout(wait) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    });
    Waker(tx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> Value {
        json!({
            "productId": "prod1",
            "sandboxId": "ns1",
            "totalAchievements": 2,
            "achievements": [
                { "achievement": { "name": "A_1", "unlockedDisplayName": "First", "unlockedDescription": "Do it",
                                   "unlockedIconLink": "https://cdn1.epicgames.com/a.png", "hidden": false, "rarity": { "percent": 12.5 } } },
                { "achievement": { "name": "A_2", "unlockedDisplayName": "", "unlockedIconLink": "https://evil.example/b.png" } },
            ]
        })
    }

    #[test]
    fn reads_a_schema_record() {
        let game = parse_schema(&record(), "Game").unwrap();
        assert_eq!(game.product_id, "prod1");
        assert_eq!(game.achievements["A_1"].icon, "https://cdn1.epicgames.com/a.png");
        assert_eq!(game.achievements["A_1"].rarity, Some(12.5));
        assert_eq!(game.achievements["A_2"].title, "A_2", "the id stands in for a missing title");
        assert_eq!(game.achievements["A_2"].icon, "", "icons on other hosts are left out");
        assert!(parse_schema(&Value::Null, "Game").is_none());
    }

    #[test]
    fn applies_unlocks_and_crowns_a_completed_game() {
        let mut game = parse_schema(&record(), "Game").unwrap();
        let player = json!({ "data": { "playerAchievements": [
            { "playerAchievement": { "achievementName": "A_1", "unlocked": true, "unlockDate": "2023-05-01T12:00:00.000Z" } },
            { "playerAchievement": { "achievementName": "A_2", "unlocked": false } },
        ]}});
        assert!(apply_player(&mut game, &player));
        assert!(!apply_player(&mut game, &player), "nothing new the second time");
        assert_eq!(game.achievements["A_1"].unlocked_at, 1_682_942_400);
        let mut cache = Cache::default();
        cache.games.insert("ns1".into(), game.clone());
        assert!(!achievements(&cache)[0].platinum);
        let all = json!({ "data": { "playerAchievements": [
            { "playerAchievement": { "achievementName": "A_2", "unlocked": true, "unlockDate": "2024-01-01T00:00:00Z" } },
        ]}});
        apply_player(cache.games.get_mut("ns1").unwrap(), &all);
        let list = achievements(&cache);
        assert!(list[0].platinum && list[0].api_name == "A_2");
    }

    #[test]
    fn keeps_values_out_of_query_text() {
        let query = schema_query(2);
        assert!(query.contains("a1: productAchievementsRecordBySandbox(sandboxId: $s1, locale: $locale)"));
        assert!(query.starts_with("query ShelfSchemas($locale: String!, $s0: String!, $s1: String!)"));
        assert!(player_query(1).contains("a0: productAchievements(productId: $p0)"));
    }

    #[test]
    fn reads_the_code_page() {
        assert_eq!(parse_code(r#"{"warning":"x","redirectUrl":"y","authorizationCode":"abc123","sid":null}"#), CodePage::Code("abc123".into()));
        assert_eq!(parse_code(r#"{"authorizationCode":null}"#), CodePage::NoCode);
        assert_eq!(parse_code("<html>"), CodePage::Unexpected);
        assert_eq!(parse_code(r#"{"authorizationCode":null,"redirectUrl":"https://localhost/launcher/authorized?code=xyz"}"#), CodePage::Code("xyz".into()));
        assert_eq!(masked(r#"{"redirectUrl":"https://a.example/x?code=1"}"#), r#"{"redirectUrl":"https://a.example/x?<hidden>"}"#);
        assert_eq!(masked(r#"{"authorizationCode":"abc","sid":"s","warning":"w"}"#), r#"{"authorizationCode":"<hidden>","sid":"<hidden>","warning":"w"}"#);
    }

    #[test]
    fn reads_catalog_titles_and_wide_art() {
        let item = json!({ "title": "Rocket League®", "keyImages": [
            { "type": "DieselGameBoxTall", "url": "https://cdn1.epicgames.com/tall.jpg" },
            { "type": "OfferImageWide", "url": "https://cdn1.epicgames.com/wide.jpg" },
            { "type": "DieselGameBox", "url": "https://cdn1.epicgames.com/box.jpg" },
        ]});
        assert_eq!(parse_catalog_item(&item), ("Rocket League®".into(), "https://cdn1.epicgames.com/box.jpg".into()));
        let elsewhere = json!({ "title": "X", "keyImages": [{ "type": "DieselGameBox", "url": "https://evil.example/a.jpg" }] });
        assert_eq!(parse_catalog_item(&elsewhere).1, "", "art on other hosts is left out");
    }

    #[test]
    fn tells_when_every_game_was_refused() {
        let refused = json!({ "a0": { "__typename": "ServiceError" }, "a1": { "__typename": "ServiceError" } });
        assert!(all_refused(&refused, 2));
        let mixed = json!({ "a0": { "__typename": "ServiceError" }, "a1": { "__typename": "PlayerProductAchievementsResponseSuccess" } });
        assert!(!all_refused(&mixed, 2));
        assert!(!all_refused(&Value::Null, 0));
    }

    #[test]
    fn explains_corrective_actions() {
        assert!(corrective_action("PRIVACY_POLICY_ACCEPTANCE").contains("accept its updated privacy policy"));
        assert!(corrective_action("SOMETHING_NEW").contains("update your account"));
    }

    #[test]
    fn accepts_only_epic_image_hosts() {
        assert!(is_image_url("https://cdn1.epicgames.com/x/a.png"));
        assert!(is_image_url("https://download.epicgames.com/a.png"));
        assert!(!is_image_url("http://cdn1.epicgames.com/a.png"));
        assert!(!is_image_url("https://epicgames.com.evil.example/a.png"));
    }
}
