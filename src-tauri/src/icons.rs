//! Achievement icons and game banners: downloaded once from the launcher's CDN, then served
//! from the disk cache.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;

use crate::achievement::Source;
use crate::steam;

/// Steam serves achievement icons from these hosts. Newer icons exist only on the first.
const STEAM_CDNS: [&str; 2] = [
    "https://shared.akamai.steamstatic.com/community_assets/images/apps",
    "https://cdn.akamai.steamstatic.com/steamcommunity/public/images/apps",
];

/// GOG stores full icon URLs; only ones on its image host are fetched.
const GOG_IMAGE_HOST: &str = "https://images.gog.com/";
/// GOG Galaxy stores 60x60 icons (`{hash}_gac_60.jpg`); the unsized `{hash}.webp` is the
/// 240x240 original. The stored URL stays as a fallback.
const GOG_ICON_FORMAT: &str = ".webp";

/// Steam serves a game's store header (460x215) from these hosts, under `/{app_id}/header.jpg`.
const STEAM_BANNER_HOSTS: [&str; 2] = [
    "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps",
    "https://cdn.akamai.steamstatic.com/steam/apps",
];

/// GOG store images are served from this host; only banners on it are fetched.
const GOG_STORE_IMAGE_HOST: &str = "https://images.gog-statics.com/";
/// Fill the `{formatter}` of a GOG `horizontalCover` template, best first: a 1020x574 JPEG,
/// then the 432x243 tile.
const GOG_BANNER_FORMATS: [&str; 2] = ["_ggvgm_2x.jpg", "_product_tile_extended_432x243.jpg"];

/// How long an image that could not be found is left before it is looked for again.
const MISS_RETRY: Duration = Duration::from_secs(10 * 60);

/// Images already turned into `data:` URLs this session, by cache path, with when each was
/// looked up. The shelf asks for the same images on every refresh; reading, encoding and,
/// for missing ones, downloading them again each time held every refresh up.
static ENCODED: LazyLock<Mutex<HashMap<PathBuf, (Option<String>, Instant)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The remembered result for `path`: `Some(None)` for a recent miss, `None` if it must be loaded.
fn remembered(path: &Path) -> Option<Option<String>> {
    match ENCODED.lock().unwrap().get(path) {
        Some((Some(url), _)) => Some(Some(url.clone())),
        Some((None, at)) if at.elapsed() < MISS_RETRY => Some(None),
        _ => None,
    }
}

fn remember(path: PathBuf, url: &Option<String>) {
    ENCODED.lock().unwrap().insert(path, (url.clone(), Instant::now()));
}

/// Looks for missing images again on the next request, as on a manual refresh.
pub fn forget_misses() {
    ENCODED.lock().unwrap().retain(|_, (url, _)| url.is_some());
}

fn is_safe_file_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
}

/// Where the icon is cached and the URLs to try, or `None` if the values are unsafe.
/// They come from launcher files; refuse anything that could escape the cache folder.
fn locate(cache_dir: &Path, source: Source, game_id: &str, icon: &str) -> Option<(PathBuf, Vec<String>)> {
    let icons = cache_dir.join("icons");
    match source {
        Source::Steam => {
            let app_id: u32 = game_id.parse().ok()?;
            if !is_safe_file_name(icon) {
                return None;
            }
            let urls = STEAM_CDNS.iter().map(|cdn| format!("{cdn}/{app_id}/{icon}")).collect();
            Some((icons.join(app_id.to_string()).join(icon), urls))
        }
        Source::Gog => {
            let file = icon.strip_prefix(GOG_IMAGE_HOST)?;
            if !is_safe_file_name(file) {
                return None;
            }
            let stem = file.split_once('.').map_or(file, |(stem, _)| stem);
            let hash = stem.split_once('_').map_or(stem, |(hash, _)| hash);
            if hash.is_empty() {
                return None;
            }
            let full = format!("{hash}{GOG_ICON_FORMAT}");
            let urls = vec![format!("{GOG_IMAGE_HOST}{full}"), icon.to_string()];
            Some((icons.join("gog").join(full), urls))
        }
    }
}

/// Returns the icon as a `data:` URL, or `None` if it is not cached and cannot be downloaded.
pub async fn icon_data_url(
    client: &reqwest::Client,
    cache_dir: &Path,
    source: Source,
    game_id: &str,
    icon: &str,
) -> Option<String> {
    let (path, urls) = locate(cache_dir, source, game_id, icon)?;
    if let Some(url) = remembered(&path) {
        return url;
    }
    let url = cached_data_url(client, &path, &urls, &format!("icon {icon} for {source} game {game_id}")).await;
    remember(path, &url);
    url
}

/// Where the game's banner is cached and the URLs to try, or `None` if there is none or the
/// values are unsafe. Steam banners come from the app id; GOG ones from the store's
/// `horizontalCover` URL template, which holds a `{formatter}` placeholder for the size.
fn locate_banner(cache_dir: &Path, source: Source, game_id: &str, cover: Option<&str>) -> Option<(PathBuf, Vec<String>)> {
    let banners = cache_dir.join("banners");
    match source {
        Source::Steam => {
            let app_id: u32 = game_id.parse().ok()?;
            let urls = STEAM_BANNER_HOSTS.iter().map(|host| format!("{host}/{app_id}/header.jpg")).collect();
            Some((banners.join(format!("{app_id}.jpg")), urls))
        }
        Source::Gog => {
            let (id, _) = cover?.strip_prefix(GOG_STORE_IMAGE_HOST)?.split_once("_{formatter}")?;
            if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
                return None;
            }
            let urls = GOG_BANNER_FORMATS.iter().map(|f| format!("{GOG_STORE_IMAGE_HOST}{id}{f}")).collect();
            Some((banners.join("gog").join(format!("{id}{}", GOG_BANNER_FORMATS[0])), urls))
        }
    }
}

/// Returns the game's banner as a `data:` URL, or `None` if it has none or it cannot be downloaded.
pub async fn banner_data_url(
    client: &reqwest::Client,
    cache_dir: &Path,
    source: Source,
    game_id: &str,
    cover: Option<&str>,
) -> Option<String> {
    let (path, urls) = locate_banner(cache_dir, source, game_id, cover)?;
    if let Some(url) = remembered(&path) {
        return url;
    }
    let url = load_banner(client, &path, &urls, source, game_id).await;
    remember(path, &url);
    url
}

async fn load_banner(client: &reqwest::Client, path: &Path, urls: &[String], source: Source, game_id: &str) -> Option<String> {
    let what = format!("banner for {source} game {game_id}");
    if let Some(bytes) = tokio_read(path).await {
        return Some(data_url(&bytes));
    }
    let mut bytes = download(client, urls).await;
    if bytes.is_none() && source == Source::Steam {
        // Newer games keep their header under a hashed path that only the store API knows.
        let app_id: u32 = game_id.parse().ok()?;
        if let Some(url) = steam::names::fetch_store_header(client, app_id).await {
            if is_steam_header_url(app_id, &url) {
                bytes = download(client, &[url]).await;
            }
        }
    }
    let Some(bytes) = bytes else {
        eprintln!("achievement-shelf: {what} not found");
        return None;
    };
    store(path, &bytes, &what);
    Some(data_url(&bytes))
}

/// Whether a header URL from the store API points at that app's images on a Steam image host.
fn is_steam_header_url(app_id: u32, url: &str) -> bool {
    STEAM_BANNER_HOSTS.iter().any(|host| url.starts_with(&format!("{host}/{app_id}/")))
}

/// Reads `path` from the disk cache, or downloads it from the first of `urls` that works and
/// caches it, and returns it as a `data:` URL. `what` names the image in log messages.
async fn cached_data_url(client: &reqwest::Client, path: &Path, urls: &[String], what: &str) -> Option<String> {
    let bytes = match tokio_read(path).await {
        Some(bytes) => bytes,
        None => {
            let Some(bytes) = download(client, urls).await else {
                eprintln!("achievement-shelf: {what} not found");
                return None;
            };
            store(path, &bytes, what);
            bytes
        }
    };
    Some(data_url(&bytes))
}

/// Writes a downloaded image to the disk cache; failures are logged, not fatal.
fn store(path: &Path, bytes: &[u8], what: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(path, bytes) {
        eprintln!("achievement-shelf: could not cache {what} at {}: {e}", path.display());
    }
}

/// Encodes an image as a `data:` URL, typed by its contents: a fallback URL may have
/// served a different format than the cache file's extension names.
fn data_url(bytes: &[u8]) -> String {
    let mime = if bytes.starts_with(b"\x89PNG") {
        "image/png"
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "image/jpeg"
    };
    format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

async fn download(client: &reqwest::Client, urls: &[String]) -> Option<Vec<u8>> {
    for url in urls {
        let Ok(response) = client.get(url).send().await else { continue };
        if let Ok(response) = response.error_for_status() {
            if let Ok(bytes) = response.bytes().await {
                return Some(bytes.to_vec());
            }
        }
    }
    None
}

async fn tokio_read(path: &Path) -> Option<Vec<u8>> {
    let path = path.to_path_buf();
    tauri::async_runtime::spawn_blocking(move || std::fs::read(path).ok())
        .await
        .ok()
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_unsafe_icon_locations() {
        let cache = Path::new("cache");
        assert!(locate(cache, Source::Steam, "1061090", "abc.jpg").is_some());
        assert!(locate(cache, Source::Steam, "1061090", "../x.jpg").is_none());
        assert!(locate(cache, Source::Steam, "../1", "abc.jpg").is_none());

        let (path, urls) = locate(cache, Source::Gog, "gog_1", "https://images.gog.com/abc_gac_60.jpg").unwrap();
        assert_eq!(path, cache.join("icons").join("gog").join("abc.webp"));
        assert_eq!(urls, ["https://images.gog.com/abc.webp", "https://images.gog.com/abc_gac_60.jpg"]);
        assert!(locate(cache, Source::Gog, "gog_1", "https://evil.example/abc.jpg").is_none());
        assert!(locate(cache, Source::Gog, "gog_1", "https://images.gog.com/../abc.jpg").is_none());
        assert!(locate(cache, Source::Gog, "gog_1", "https://images.gog.com/a/b.jpg").is_none());
    }

    #[test]
    fn refuses_unsafe_banner_locations() {
        let cache = Path::new("cache");
        let (path, urls) = locate_banner(cache, Source::Steam, "1061090", None).unwrap();
        assert_eq!(path, cache.join("banners").join("1061090.jpg"));
        assert_eq!(urls[0], "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/1061090/header.jpg");
        assert!(locate_banner(cache, Source::Steam, "../1", None).is_none());

        let hashed = "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/3527290/31bac6b2/header.jpg?t=1";
        assert!(is_steam_header_url(3527290, hashed));
        assert!(!is_steam_header_url(1061090, hashed), "another app's header");
        assert!(!is_steam_header_url(3527290, "https://evil.example/store_item_assets/steam/apps/3527290/header.jpg"));

        let cover = "https://images.gog-statics.com/5a51d9_{formatter}.png";
        let (path, urls) = locate_banner(cache, Source::Gog, "gog_1", Some(cover)).unwrap();
        assert_eq!(path, cache.join("banners").join("gog").join("5a51d9_ggvgm_2x.jpg"));
        assert_eq!(
            urls,
            [
                "https://images.gog-statics.com/5a51d9_ggvgm_2x.jpg",
                "https://images.gog-statics.com/5a51d9_product_tile_extended_432x243.jpg",
            ]
        );
        assert!(locate_banner(cache, Source::Gog, "gog_1", None).is_none());
        assert!(locate_banner(cache, Source::Gog, "gog_1", Some("https://evil.example/5a51d9_{formatter}.png")).is_none());
        assert!(locate_banner(cache, Source::Gog, "gog_1", Some("https://images.gog-statics.com/../x_{formatter}.png")).is_none());
        assert!(locate_banner(cache, Source::Gog, "gog_1", Some("https://images.gog-statics.com/5a51d9.png")).is_none());
    }
}
