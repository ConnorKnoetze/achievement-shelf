//! Xbox: the signed-in Microsoft account's Xbox achievements, from Xbox Live.
//!
//! Signing in opens Microsoft's sign-in page in the user's browser (OAuth with PKCE). The page
//! sends the browser back to a one-off listener on `localhost`. Only the refresh token is kept,
//! in Credential Manager. From it the sync thread ([`spawn`]) gets an Xbox Live (XSTS) token and
//! copies the account's games into a cache file, which the shelf reads like any launcher's files:
//!
//! - Every minute it reads the title history, which counts each game's unlocks.
//! - It fetches a game's achievements again only when that count changes, or the game's rarity
//!   is a week old.
//!
//! The sign-in runs as Achievement Shelf's Azure app ([`CLIENT_ID`]), or another one named by
//! `XBOX_CLIENT_ID` when the app is built.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::achievement::{crown_platinum, game_key, parse_utc_time, Achievement, Source};
use crate::{accounts, browser, store};

/// The public client id of the Azure app the sign-in runs as. Not a secret. A build can name
/// another app with the `XBOX_CLIENT_ID` environment variable.
const CLIENT_ID: Option<&str> = match option_env!("XBOX_CLIENT_ID") {
    Some(id) if !id.is_empty() => Some(id),
    _ => Some("8e0ad734-0829-4123-9f82-1ab9ad9330c5"),
};
const AUTHORIZE_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize";
const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const SCOPE: &str = "XboxLive.signin offline_access";
const USER_AUTH_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const TITLEHUB: &str = "https://titlehub.xboxlive.com";
const ACHIEVEMENTS: &str = "https://achievements.xboxlive.com";
/// Achievement and game art is served from these.
pub const IMAGE_HOSTS: &[&str] = &["https://images-eds-ssl.xboxlive.com/", "https://store-images.s-microsoft.com/"];
const CREDENTIAL: &str = "xbox";
const CACHE_FILE: &str = "xbox.json";
/// How long a sign-in waits for the browser to come back.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Rarity drifts as other people play, so a game is fetched again after this long anyway.
const STALE_AFTER_SECS: u64 = 7 * 24 * 60 * 60;
const POLL: Duration = Duration::from_secs(60);
/// Between the game fetches of a sync, to stay clear of Xbox Live's rate limits.
const FETCH_DELAY: Duration = Duration::from_secs(1);
const DEFAULT_RATE_PAUSE: Duration = Duration::from_secs(5 * 60);
/// A token this close to expiring is renewed first.
const RENEW_MARGIN_SECS: u64 = 5 * 60;
/// The store's image service resizes on request (its full size is up to 4K). The achievement
/// image service answers 400 to any size parameter, so icons come at their own size.
const BANNER_WIDTH: u32 = 640;
/// Bumped when cached games need fetching again; version 1 dropped the size parameter that
/// broke every icon URL.
const CACHE_VERSION: u32 = 1;

/// Whether this build can sign in to Xbox.
pub fn available() -> bool {
    CLIENT_ID.is_some_and(|id| !id.trim().is_empty())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Account {
    gamertag: String,
    xuid: String,
    refresh_token: String,
}

fn account() -> Option<Account> {
    accounts::load(CREDENTIAL).and_then(|text| serde_json::from_str(&text).ok())
}

fn save_account(account: &Account) -> Result<(), String> {
    accounts::save(CREDENTIAL, &serde_json::to_string(account).map_err(|e| e.to_string())?)
}

/// The signed-in gamertag, if any.
pub fn gamertag() -> Option<String> {
    account().map(|a| a.gamertag)
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
struct Cache {
    #[serde(default)]
    version: u32,
    /// Whose games these are; another account's are thrown away.
    xuid: String,
    /// Keyed by title id.
    games: BTreeMap<String, Game>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
struct Game {
    name: String,
    /// Hero art, used as the game's banner. Empty when the title history has none.
    banner: String,
    /// What the title history said when the game was fetched; it changes with unlocks.
    fingerprint: String,
    fetched_at: u64,
    /// Keyed by achievement id. Empty for a game Xbox Live listed no achievements for, such as
    /// an Xbox 360 game, so it isn't fetched again on every sync.
    achievements: BTreeMap<String, Entry>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
struct Entry {
    title: String,
    description: String,
    icon: String,
    secret: bool,
    /// Percentage of players who unlocked it.
    rarity: Option<f32>,
    /// Unix seconds; 0 while locked, 1 when unlocked at an unknown time.
    unlocked_at: u32,
}

fn cache_path(data_dir: &Path) -> PathBuf {
    data_dir.join(CACHE_FILE)
}

/// What the shelf reads: the cached games of the signed-in account.
pub struct Xbox {
    cache: Cache,
}

impl Xbox {
    /// The account's cached games, or `None` when nobody is signed in.
    pub fn locate(data_dir: &Path) -> Option<Self> {
        let account = account()?;
        let cache: Cache = store::load(&cache_path(data_dir));
        let cache = if cache.xuid == account.xuid { cache } else { Cache::default() };
        Some(Self { cache })
    }

    /// All unlocked achievements, newest first, with each completed game's platinum marked.
    pub fn load_all(&self) -> Vec<Achievement> {
        achievements(&self.cache)
    }

    /// Each game's name, keyed by [`game_key`].
    pub fn names(&self) -> HashMap<String, String> {
        let games = self.cache.games.iter().filter(|(_, game)| !game.name.is_empty());
        games.map(|(id, game)| (game_key(Source::Xbox, id), game.name.clone())).collect()
    }

    /// Hero art URLs, keyed by [`game_key`].
    pub fn banners(&self) -> HashMap<String, String> {
        let games = self.cache.games.iter().filter(|(_, game)| !game.banner.is_empty());
        games.map(|(id, game)| (game_key(Source::Xbox, id), game.banner.clone())).collect()
    }
}

fn achievements(cache: &Cache) -> Vec<Achievement> {
    let mut all = Vec::new();
    let mut complete = HashSet::new();
    for (game_id, game) in &cache.games {
        for (id, entry) in &game.achievements {
            if entry.unlocked_at == 0 {
                continue;
            }
            all.push(Achievement {
                source: Source::Xbox,
                game_id: game_id.clone(),
                api_name: id.clone(),
                title: entry.title.clone(),
                description: entry.description.clone(),
                hidden: entry.secret,
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

/// An Xbox Live image URL on one of [`IMAGE_HOSTS`], asking for `width` pixels if given, or
/// empty. Xbox Live hands some out over plain http; the same files are served over https.
fn image_url(url: &str, width: Option<u32>) -> String {
    let url = url.trim();
    let url = match url.strip_prefix("http://") {
        Some(rest) => format!("https://{}", rest.strip_prefix("images-eds.xboxlive.com/").map_or(rest.to_string(), |r| format!("images-eds-ssl.xboxlive.com/{r}"))),
        None => url.to_string(),
    };
    if !is_image_url(&url) {
        return String::new();
    }
    let Some(width) = width else { return url };
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}w={width}")
}

/// Whether `url` is on one of [`IMAGE_HOSTS`] and safe to request.
pub fn is_image_url(url: &str) -> bool {
    IMAGE_HOSTS.iter().any(|host| url.starts_with(host))
        && !url.contains(['\\', '#'])
        && url.chars().all(|c| c.is_ascii_graphic())
}

/// A number Xbox Live gives as a JSON number or as a string.
fn number(value: &Value) -> u64 {
    match value {
        Value::Number(n) => n.as_u64().unwrap_or(0),
        Value::String(s) => s.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

fn text(value: &Value) -> String {
    match value {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// A game from the title history, with achievements the account has unlocked.
#[derive(Debug, PartialEq)]
struct Title {
    id: String,
    name: String,
    banner: String,
    fingerprint: String,
}

/// The title-history rows worth fetching: games with at least one unlock.
fn parse_titles(payload: &Value) -> Vec<Title> {
    let rows = payload["titles"].as_array().map(Vec::as_slice).unwrap_or_default();
    rows.iter()
        .filter_map(|row| {
            let id = number(&row["titleId"]);
            let progress = &row["achievement"];
            if id == 0 || number(&progress["currentAchievements"]) == 0 {
                return None;
            }
            let fingerprint = ["currentAchievements", "totalAchievements", "currentGamerscore"]
                .iter()
                .map(|key| text(&progress[key]))
                .collect::<Vec<_>>()
                .join("|");
            let images = row["images"].as_array().map(Vec::as_slice).unwrap_or_default();
            let art = ["TitledHeroArt", "SuperHeroArt", "BrandedKeyArt", "Hero"]
                .iter()
                .find_map(|kind| images.iter().find(|image| text(&image["type"]).eq_ignore_ascii_case(kind)))
                .map(|image| text(&image["url"]))
                .unwrap_or_else(|| text(&row["displayImage"]));
            Some(Title { id: id.to_string(), name: text(&row["name"]), banner: image_url(&art, Some(BANNER_WIDTH)), fingerprint })
        })
        .collect()
}

/// One achievement from the achievements service, or `None` without an id.
fn parse_achievement(raw: &Value) -> Option<(String, Entry)> {
    let id = text(&raw["id"]);
    if id.is_empty() {
        return None;
    }
    let achieved = text(&raw["progressState"]).eq_ignore_ascii_case("achieved");
    let unlocked_at = if achieved {
        // Unknown times come as 0001-01-01, which isn't a unix time.
        raw["progression"]["timeUnlocked"].as_str().and_then(parse_utc_time).filter(|&t| t > 0).unwrap_or(1)
    } else {
        0
    };
    let media = raw["mediaAssets"].as_array().map(Vec::as_slice).unwrap_or_default();
    let icon = media
        .iter()
        .find(|m| text(&m["type"]).eq_ignore_ascii_case("icon"))
        .or(media.first())
        .map(|m| image_url(&text(&m["url"]), None))
        .unwrap_or_default();
    let description = [text(&raw["description"]), text(&raw["lockedDescription"])].into_iter().find(|d| !d.is_empty());
    let title = text(&raw["name"]);
    let rarity = raw["rarity"]["currentPercentage"].as_f64().map(|p| p.clamp(0.0, 100.0) as f32);
    Some((
        id.clone(),
        Entry {
            title: if title.is_empty() { format!("Achievement {id}") } else { title },
            description: description.unwrap_or_default(),
            icon,
            secret: raw["isSecret"].as_bool().unwrap_or(false),
            rarity,
            unlocked_at,
        },
    ))
}

/// Puts a freshly fetched game in the cache. An unlock the cache already has keeps its earlier
/// time, and survives an answer fetched just before it.
fn merge_game(cache: &mut Cache, id: &str, mut game: Game) {
    if let Some(old) = cache.games.get(id) {
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
    cache.games.insert(id.to_string(), game);
}

#[derive(Debug)]
enum ApiError {
    /// Microsoft refused the refresh token: the user has to sign in again.
    SignedOut,
    /// The Xbox Live token was refused; get a new one.
    Expired,
    /// Too many calls; wait this long.
    RateLimited(Duration),
    Other(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::SignedOut => f.write_str("the sign-in was refused"),
            ApiError::Expired => f.write_str("the Xbox Live token was refused"),
            ApiError::RateLimited(wait) => write!(f, "rate limited for {}s", wait.as_secs()),
            ApiError::Other(e) => f.write_str(e),
        }
    }
}

fn request_error(e: reqwest::Error) -> ApiError {
    ApiError::Other(e.without_url().to_string())
}

fn retry_after(response: &reqwest::Response) -> Duration {
    let header = response.headers().get(reqwest::header::RETRY_AFTER).and_then(|v| v.to_str().ok());
    header.and_then(|v| v.trim().parse().ok()).map_or(DEFAULT_RATE_PAUSE, Duration::from_secs)
}

/// Tokens from Microsoft's token endpoint. The refresh token may be replaced by a new one.
struct Tokens {
    access_token: String,
    refresh_token: String,
}

async fn token_request(client: &reqwest::Client, form: &[(&str, &str)]) -> Result<Tokens, ApiError> {
    let response = client.post(TOKEN_URL).form(form).send().await.map_err(request_error)?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or_default();
    if !status.is_success() {
        // invalid_grant: the refresh token or code was revoked, expired or already used.
        return Err(match body["error"].as_str() {
            Some("invalid_grant" | "interaction_required") => ApiError::SignedOut,
            Some(error) => ApiError::Other(format!("Microsoft sign-in answered {error}")),
            None => ApiError::Other(format!("Microsoft sign-in answered {status}")),
        });
    }
    let access_token = text(&body["access_token"]);
    if access_token.is_empty() {
        return Err(ApiError::Other("Microsoft sign-in gave no token".into()));
    }
    Ok(Tokens { access_token, refresh_token: text(&body["refresh_token"]) })
}

/// What Xbox Live calls need: the user hash and XSTS token, and when the token expires.
struct Session {
    xuid: String,
    uhs: String,
    token: String,
    expires_at: u64,
}

impl Session {
    fn header(&self) -> String {
        format!("XBL3.0 x={};{}", self.uhs, self.token)
    }

    fn fresh(&self) -> bool {
        self.expires_at > now() + RENEW_MARGIN_SECS
    }
}

async fn post_json(client: &reqwest::Client, url: &str, body: &Value) -> Result<Value, (reqwest::StatusCode, Value)> {
    let response = client
        .post(url)
        .header("Accept", "application/json")
        .json(body)
        .send()
        .await
        .map_err(|_| (reqwest::StatusCode::SERVICE_UNAVAILABLE, Value::Null))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or_default();
    if status.is_success() { Ok(body) } else { Err((status, body)) }
}

/// Trades a Microsoft access token for an Xbox Live session and the account's gamertag.
async fn xbox_session(client: &reqwest::Client, access_token: &str) -> Result<(Session, String), String> {
    let user = json!({
        "RelyingParty": "http://auth.xboxlive.com",
        "TokenType": "JWT",
        "Properties": { "AuthMethod": "RPS", "SiteName": "user.auth.xboxlive.com", "RpsTicket": format!("d={access_token}") },
    });
    let user = post_json(client, USER_AUTH_URL, &user).await.map_err(|(status, _)| format!("Xbox Live sign-in answered {status}"))?;
    let user_token = text(&user["Token"]);
    let xsts = json!({
        "RelyingParty": "http://xboxlive.com",
        "TokenType": "JWT",
        "Properties": { "SandboxId": "RETAIL", "UserTokens": [user_token] },
    });
    let xsts = post_json(client, XSTS_URL, &xsts).await.map_err(|(status, body)| xsts_error(status, &body))?;
    parse_xsts(&xsts).ok_or_else(|| "Xbox Live gave an unexpected answer.".into())
}

fn parse_xsts(xsts: &Value) -> Option<(Session, String)> {
    let claims = &xsts["DisplayClaims"]["xui"][0];
    let session = Session {
        xuid: text(&claims["xid"]),
        uhs: text(&claims["uhs"]),
        token: text(&xsts["Token"]),
        expires_at: xsts["NotAfter"].as_str().and_then(parse_utc_time).map_or(0, u64::from),
    };
    if session.xuid.is_empty() || session.uhs.is_empty() || session.token.is_empty() {
        return None;
    }
    let gamertag = Some(text(&claims["gtg"])).filter(|g| !g.is_empty()).unwrap_or_else(|| "your Xbox account".into());
    Some((session, gamertag))
}

/// The reasons Xbox Live gives for refusing an account, by `XErr` code.
fn xsts_error(status: reqwest::StatusCode, body: &Value) -> String {
    match body["XErr"].as_u64() {
        Some(2_148_916_233) => "That Microsoft account has no Xbox profile yet. Sign in at xbox.com once to make one.".into(),
        Some(2_148_916_235) => "Xbox Live isn't available in your account's country.".into(),
        Some(2_148_916_236 | 2_148_916_237) => "That account needs adult verification on xbox.com first.".into(),
        Some(2_148_916_238) => "That's a child account. An adult has to add it to a Microsoft family first.".into(),
        _ => format!("Xbox Live sign-in answered {status}"),
    }
}

/// A new session from the stored refresh token, saving the token Microsoft hands back.
async fn renew(client: &reqwest::Client, account: &mut Account) -> Result<Session, ApiError> {
    let client_id = CLIENT_ID.unwrap_or_default();
    let form = [("client_id", client_id), ("grant_type", "refresh_token"), ("refresh_token", &account.refresh_token), ("scope", SCOPE)];
    let tokens = token_request(client, &form).await?;
    if !tokens.refresh_token.is_empty() && tokens.refresh_token != account.refresh_token {
        account.refresh_token = tokens.refresh_token;
        if let Err(e) = save_account(account) {
            eprintln!("achievement-shelf: Xbox: {e}");
        }
    }
    let (session, _) = xbox_session(client, &tokens.access_token).await.map_err(ApiError::Other)?;
    Ok(session)
}

/// One Xbox Live GET.
async fn xbl_get(client: &reqwest::Client, session: &Session, url: &str, contract: &str) -> Result<Value, ApiError> {
    let response = client
        .get(url)
        .header("Authorization", session.header())
        .header("x-xbl-contract-version", contract)
        .header("Accept", "application/json")
        .header("Accept-Language", "en-US")
        .send()
        .await
        .map_err(request_error)?;
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(ApiError::Expired);
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(ApiError::RateLimited(retry_after(&response)));
    }
    if !status.is_success() {
        return Err(ApiError::Other(format!("Xbox Live answered {status}")));
    }
    response.json().await.map_err(request_error)
}

async fn title_history(client: &reqwest::Client, session: &Session) -> Result<Vec<Title>, ApiError> {
    let url = format!("{TITLEHUB}/users/xuid({})/titles/titlehistory/decoration/achievement,image?maxItems=1000", session.xuid);
    Ok(parse_titles(&xbl_get(client, session, &url, "2").await?))
}

/// Every achievement of a title, locked ones too, following the service's pages.
async fn title_achievements(client: &reqwest::Client, session: &Session, title_id: &str) -> Result<BTreeMap<String, Entry>, ApiError> {
    let base = format!("{ACHIEVEMENTS}/users/xuid({})/achievements", session.xuid);
    let mut all = BTreeMap::new();
    let mut continuation = String::new();
    for _ in 0..20 {
        let mut params = vec![("titleId", title_id.to_string()), ("maxItems", "1000".to_string())];
        if !continuation.is_empty() {
            params.push(("continuationToken", continuation.clone()));
        }
        let url = reqwest::Url::parse_with_params(&base, &params).map_err(|e| ApiError::Other(e.to_string()))?;
        let page = xbl_get(client, session, url.as_str(), "4").await?;
        let rows = page["achievements"].as_array().map(Vec::as_slice).unwrap_or_default();
        all.extend(rows.iter().filter_map(parse_achievement));
        let next = text(&page["pagingInfo"]["continuationToken"]);
        if next.is_empty() || next == continuation {
            break;
        }
        continuation = next;
    }
    Ok(all)
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Bumped by each sign-in and by [`cancel_sign_in`]; a sign-in that sees it change stops waiting.
static SIGN_IN: AtomicU64 = AtomicU64::new(0);

/// Stops a sign-in that is waiting for the browser.
pub fn cancel_sign_in() {
    SIGN_IN.fetch_add(1, Ordering::SeqCst);
}

fn random_token(bytes: usize) -> Result<String, String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| e.to_string())?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Signs in through the user's browser and returns the gamertag. Signing in as someone else
/// throws the previous account's cache away.
pub async fn connect(client: &reqwest::Client, data_dir: &Path) -> Result<String, String> {
    let client_id = CLIENT_ID.filter(|id| !id.trim().is_empty()).ok_or("This build of Achievement Shelf can't sign in to Xbox.")?;
    let attempt = SIGN_IN.fetch_add(1, Ordering::SeqCst) + 1;
    let verifier = random_token(48)?;
    let state = random_token(16)?;

    // `localhost` may resolve to either loopback address, so listen on both.
    let v4 = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("Couldn't start the sign-in: {e}"))?;
    let port = v4.local_addr().map_err(|e| e.to_string())?.port();
    let mut listeners = vec![v4];
    listeners.extend(TcpListener::bind(("::1", port)).ok());
    let redirect = format!("http://localhost:{port}");

    let url = reqwest::Url::parse_with_params(
        AUTHORIZE_URL,
        &[
            ("client_id", client_id),
            ("response_type", "code"),
            ("redirect_uri", &redirect),
            ("response_mode", "query"),
            ("scope", SCOPE),
            ("state", &state),
            ("code_challenge", &pkce_challenge(&verifier)),
            ("code_challenge_method", "S256"),
            ("prompt", "select_account"),
        ],
    )
    .map_err(|e| e.to_string())?;
    browser::open(url.as_str())?;

    let code = tauri::async_runtime::spawn_blocking(move || wait_for_code(&listeners, &state, attempt))
        .await
        .map_err(|e| e.to_string())??;
    let form = [
        ("client_id", client_id),
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("redirect_uri", &redirect),
        ("code_verifier", &verifier),
        ("scope", SCOPE),
    ];
    let tokens = token_request(client, &form).await.map_err(|e| match e {
        ApiError::SignedOut => "Microsoft didn't accept the sign-in. Try again.".to_string(),
        e => format!("Couldn't sign in: {e}"),
    })?;
    if tokens.refresh_token.is_empty() {
        return Err("Microsoft didn't allow staying signed in. Try again.".into());
    }
    let (session, gamertag) = xbox_session(client, &tokens.access_token).await?;
    save_account(&Account { gamertag: gamertag.clone(), xuid: session.xuid.clone(), refresh_token: tokens.refresh_token })?;
    let cache: Cache = store::load(&cache_path(data_dir));
    if cache.xuid != session.xuid {
        let _ = std::fs::remove_file(cache_path(data_dir));
    }
    Ok(gamertag)
}

/// Waits for the browser to come back from Microsoft's sign-in page with a code.
fn wait_for_code(listeners: &[TcpListener], state: &str, attempt: u64) -> Result<String, String> {
    for listener in listeners {
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    }
    let deadline = Instant::now() + SIGN_IN_TIMEOUT;
    while Instant::now() < deadline {
        if SIGN_IN.load(Ordering::SeqCst) != attempt {
            return Err("Sign-in cancelled.".into());
        }
        for listener in listeners {
            match listener.accept() {
                Ok((stream, _)) => {
                    if let Some(result) = answer(stream, state) {
                        return result;
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                Err(e) => return Err(format!("Couldn't finish the sign-in: {e}")),
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err("Sign-in timed out. Try again.".into())
}

/// Reads one request to the sign-in listener and answers it. `None` for a request that isn't
/// the sign-in coming back (such as the browser asking for a favicon), so waiting goes on.
fn answer(mut stream: TcpStream, state: &str) -> Option<Result<String, String>> {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut request = Vec::new();
    let mut buf = [0u8; 2048];
    while request.len() < 16 * 1024 && !request.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => request.extend_from_slice(&buf[..n]),
        }
    }
    let result = parse_redirect(&String::from_utf8_lossy(&request), state);
    let (status, message) = match &result {
        None => ("404 Not Found", "Not found."),
        Some(Ok(_)) => ("200 OK", "You're signed in to Xbox. You can close this tab and go back to Achievement Shelf."),
        Some(Err(_)) => ("200 OK", "Xbox sign-in didn't finish. You can close this tab and try again in Achievement Shelf."),
    };
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>Achievement Shelf</title>\
         <body style=\"font:16px system-ui,sans-serif;margin:4em auto;max-width:32em;text-align:center\"><p>{message}</p>"
    );
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    result
}

/// The code from a `GET /?code=…&state=…` request, an error for `GET /?error=…`, and `None`
/// for anything else, including a request whose state doesn't match this sign-in.
fn parse_redirect(request: &str, state: &str) -> Option<Result<String, String>> {
    let target = request.lines().next()?.strip_prefix("GET ")?.split(' ').next()?;
    let url = reqwest::Url::parse(&format!("http://localhost{target}")).ok()?;
    if url.path() != "/" {
        return None;
    }
    let params: HashMap<String, String> = url.query_pairs().into_owned().collect();
    if params.get("state").map(String::as_str) != Some(state) {
        return None;
    }
    if let Some(error) = params.get("error") {
        return Some(Err(if error == "access_denied" { "Sign-in cancelled.".into() } else { format!("Microsoft sign-in answered {error}.") }));
    }
    params.get("code").filter(|c| !c.is_empty()).map(|code| Ok(code.clone()))
}

/// Signs out and forgets the account's games.
pub fn disconnect(data_dir: &Path) {
    accounts::clear(CREDENTIAL);
    let _ = std::fs::remove_file(cache_path(data_dir));
}

/// What a sync did: whether unlocks changed, and the error that stopped it, if any.
struct Outcome {
    changed: bool,
    error: Option<ApiError>,
}

/// Fetches every game whose unlocks changed, saving after each so an interrupted sync resumes.
async fn sync(client: &reqwest::Client, data_dir: &Path, session: &Session) -> Outcome {
    let path = cache_path(data_dir);
    let mut cache: Cache = store::load(&path);
    if cache.xuid != session.xuid {
        cache = Cache { version: CACHE_VERSION, xuid: session.xuid.clone(), games: BTreeMap::new() };
    }
    let before = cache.clone();
    if cache.version != CACHE_VERSION {
        cache.games.values_mut().for_each(|game| game.fingerprint.clear());
        cache.version = CACHE_VERSION;
    }
    let titles = match title_history(client, session).await {
        Ok(titles) => titles,
        Err(error) => return Outcome { changed: false, error: Some(error) },
    };
    let mut error = None;
    for title in titles {
        let fresh = cache.games.get(&title.id).is_some_and(|g| {
            g.fingerprint == title.fingerprint && now().saturating_sub(g.fetched_at) < STALE_AFTER_SECS
        });
        if fresh {
            continue;
        }
        match title_achievements(client, session, &title.id).await {
            Ok(achievements) => {
                let game = Game { name: title.name, banner: title.banner, fingerprint: title.fingerprint, fetched_at: now(), achievements };
                merge_game(&mut cache, &title.id, game);
                store::save(&path, &cache);
            }
            Err(e @ (ApiError::RateLimited(_) | ApiError::Expired)) => {
                error = Some(e);
                break;
            }
            Err(e) => eprintln!("achievement-shelf: cannot fetch Xbox title {}: {e}", title.id),
        }
        std::thread::sleep(FETCH_DELAY);
    }
    // Any change shows: a refetch can bring new icons or names without new unlocks.
    let changed = cache != before;
    if changed {
        store::save(&path, &cache);
    }
    Outcome { changed, error }
}

/// Asks the sync thread to sync now, as after signing in or a manual refresh.
#[derive(Clone)]
pub struct Waker(mpsc::Sender<()>);

impl Waker {
    pub fn sync_now(&self) {
        let _ = self.0.send(());
    }
}

/// Starts the thread that keeps the cache up to date. `on_change` is called when unlocks change,
/// including when Microsoft ends the sign-in.
pub fn spawn(client: reqwest::Client, data_dir: PathBuf, on_change: impl Fn() + Send + 'static) -> Waker {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut session: Option<Session> = None;
        let mut paused_until: Option<Instant> = None;
        loop {
            let wait = match account() {
                Some(mut account) if available() => {
                    if paused_until.is_none_or(|until| Instant::now() >= until) {
                        paused_until = None;
                        let outcome = tauri::async_runtime::block_on(async {
                            if !session.as_ref().is_some_and(|s| s.fresh() && s.xuid == account.xuid) {
                                session = Some(renew(&client, &mut account).await?);
                            }
                            let outcome = sync(&client, &data_dir, session.as_ref().expect("set above")).await;
                            match outcome.error {
                                Some(error) => Err(error),
                                None => Ok(outcome.changed),
                            }
                        });
                        match outcome {
                            Ok(changed) => {
                                if changed {
                                    on_change();
                                }
                            }
                            Err(ApiError::SignedOut) => {
                                eprintln!("achievement-shelf: Xbox: the sign-in ended; sign in again in Settings");
                                disconnect(&data_dir);
                                session = None;
                                on_change();
                            }
                            Err(ApiError::Expired) => session = None,
                            Err(ApiError::RateLimited(wait)) => paused_until = Some(Instant::now() + wait),
                            Err(ApiError::Other(e)) => eprintln!("achievement-shelf: Xbox: {e}"),
                        }
                    }
                    POLL
                }
                // Nobody signed in: sleep until someone does.
                _ => {
                    session = None;
                    Duration::from_secs(60 * 60)
                }
            };
            match rx.recv_timeout(wait) {
                Ok(()) => paused_until = None,
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

    fn achievement(id: &str, state: &str, time: &str) -> Value {
        json!({
            "id": id,
            "name": format!("Achievement {id}"),
            "description": "Did the thing",
            "lockedDescription": "Do the thing",
            "progressState": state,
            "progression": { "timeUnlocked": time },
            "isSecret": false,
            "mediaAssets": [{ "name": "a", "type": "Icon", "url": "http://images-eds.xboxlive.com/image?url=abc" }],
            "rarity": { "currentCategory": "Rare", "currentPercentage": 8.25 },
        })
    }

    #[test]
    fn reads_achievements() {
        let (id, entry) = parse_achievement(&achievement("1", "Achieved", "2019-05-02T05:03:08.1870000Z")).unwrap();
        assert_eq!(id, "1");
        assert_eq!(entry.unlocked_at, 1_556_773_388);
        assert_eq!(entry.icon, "https://images-eds-ssl.xboxlive.com/image?url=abc");
        assert_eq!(entry.rarity, Some(8.25));
        assert_eq!(entry.description, "Did the thing");

        let (_, locked) = parse_achievement(&achievement("2", "NotStarted", "0001-01-01T00:00:00.0000000Z")).unwrap();
        assert_eq!(locked.unlocked_at, 0);
        let (_, unknown) = parse_achievement(&achievement("3", "Achieved", "0001-01-01T00:00:00.0000000Z")).unwrap();
        assert_eq!(unknown.unlocked_at, 1, "unlocked at an unknown time");
    }

    #[test]
    fn reads_only_titles_with_unlocks() {
        let payload = json!({ "titles": [
            { "titleId": "1144039928", "name": "Halo MCC", "displayImage": "http://store-images.s-microsoft.com/image/apps.1",
              "achievement": { "currentAchievements": 3, "totalAchievements": 700, "currentGamerscore": 30 },
              "images": [{ "type": "Logo", "url": "https://store-images.s-microsoft.com/image/apps.logo" },
                         { "type": "SuperHeroArt", "url": "https://store-images.s-microsoft.com/image/apps.hero" }] },
            { "titleId": "2", "name": "Unplayed", "achievement": { "currentAchievements": 0 } },
            { "titleId": "3", "name": "Elsewhere", "displayImage": "https://evil.example/x.png", "achievement": { "currentAchievements": 1 } },
        ]});
        let titles = parse_titles(&payload);
        assert_eq!(titles.len(), 2);
        assert_eq!(titles[0].banner, "https://store-images.s-microsoft.com/image/apps.hero?w=640");
        assert_eq!(titles[0].fingerprint, "3|700|30");
        assert_eq!(titles[1].banner, "", "art on other hosts is left out");
    }

    #[test]
    fn a_refetch_keeps_earlier_unlocks_and_crowns_a_completed_game() {
        let game = |rows: &[Value]| Game { achievements: rows.iter().filter_map(parse_achievement).collect(), ..Game::default() };
        let mut cache = Cache::default();
        merge_game(&mut cache, "7", game(&[achievement("1", "Achieved", "2020-01-01T00:00:00Z"), achievement("2", "NotStarted", "")]));
        assert!(!achievements(&cache)[0].platinum);
        merge_game(&mut cache, "7", game(&[achievement("1", "Achieved", "2021-01-01T00:00:00Z"), achievement("2", "Achieved", "2022-01-01T00:00:00Z")]));
        assert_eq!(cache.games["7"].achievements["1"].unlocked_at, 1_577_836_800);
        let list = achievements(&cache);
        assert_eq!(list.len(), 2);
        assert!(list[0].platinum && list[0].api_name == "2");
    }

    #[test]
    fn reads_the_xsts_answer() {
        let xsts = json!({ "Token": "tok", "NotAfter": "2026-11-28T10:00:00.0000000Z",
                           "DisplayClaims": { "xui": [{ "gtg": "Player One", "xid": "2535400000000000", "uhs": "123" }] } });
        let (session, gamertag) = parse_xsts(&xsts).unwrap();
        assert_eq!(gamertag, "Player One");
        assert_eq!(session.header(), "XBL3.0 x=123;tok");
        assert!(parse_xsts(&json!({ "Token": "tok" })).is_none());
    }

    #[test]
    fn takes_only_the_matching_redirect() {
        let ok = "GET /?code=M.abc&state=s1 HTTP/1.1\r\nHost: localhost\r\n\r\n";
        assert_eq!(parse_redirect(ok, "s1"), Some(Ok("M.abc".into())));
        assert_eq!(parse_redirect(ok, "other"), None, "a stray request with another state is ignored");
        assert_eq!(parse_redirect("GET /favicon.ico HTTP/1.1\r\n\r\n", "s1"), None);
        assert_eq!(parse_redirect("GET /?error=access_denied&state=s1 HTTP/1.1\r\n\r\n", "s1"), Some(Err("Sign-in cancelled.".into())));
    }

    #[test]
    fn builds_the_pkce_challenge() {
        // RFC 7636, appendix B.
        assert_eq!(pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn accepts_only_xbox_image_urls() {
        assert!(is_image_url("https://images-eds-ssl.xboxlive.com/image?url=abc&w=256"));
        assert!(!is_image_url("https://evil.example/image"));
        assert!(!is_image_url("https://images-eds-ssl.xboxlive.com.evil.example/x"));
        assert!(!is_image_url("https://store-images.s-microsoft.com/a b"));
    }
}
