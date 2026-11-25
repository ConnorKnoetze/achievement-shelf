//! Achievement icons and game banners: downloaded once from the launcher's CDN, then served
//! from the disk cache.

use std::path::{Path, PathBuf};

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

/// Steam serves a game's store header (460x215) from these hosts, under `/{app_id}/header.jpg`.
const STEAM_BANNER_HOSTS: [&str; 2] = [
    "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps",
    "https://cdn.akamai.steamstatic.com/steam/apps",
];

/// GOG store images are served from this host; only banners on it are fetched.
const GOG_STORE_IMAGE_HOST: &str = "https://images.gog-statics.com/";
/// Fills the `{formatter}` of a GOG `horizontalCover` template: a 432x243 JPEG tile.
const GOG_BANNER_FORMAT: &str = "_product_tile_extended_432x243.jpg";

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
            Some((icons.join("gog").join(file), vec![icon.to_string()]))
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
    cached_data_url(client, &path, &urls, &format!("icon {icon} for {source} game {game_id}")).await
}

/// Where the game's banner is cached and the URLs to try, or `None` if there is none or the
/// values are unsafe. Steam banners come from the app id; GOG ones from the store's
/// `horizontalCover` URL template, which holds a `{formatter}` placeholder for the size.
fn locate_banner(cache_dir: &Path, source: Source, game_id: &str, gog_cover: Option<&str>) -> Option<(PathBuf, Vec<String>)> {
    let banners = cache_dir.join("banners");
    match source {
        Source::Steam => {
            let app_id: u32 = game_id.parse().ok()?;
            let urls = STEAM_BANNER_HOSTS.iter().map(|host| format!("{host}/{app_id}/header.jpg")).collect();
            Some((banners.join(format!("{app_id}.jpg")), urls))
        }
        Source::Gog => {
            let (id, _) = gog_cover?.strip_prefix(GOG_STORE_IMAGE_HOST)?.split_once("_{formatter}")?;
            if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
                return None;
            }
            let file = format!("{id}{GOG_BANNER_FORMAT}");
            let url = format!("{GOG_STORE_IMAGE_HOST}{file}");
            Some((banners.join("gog").join(file), vec![url]))
        }
    }
}

/// Returns the game's banner as a `data:` URL, or `None` if it has none or it cannot be downloaded.
pub async fn banner_data_url(
    client: &reqwest::Client,
    cache_dir: &Path,
    source: Source,
    game_id: &str,
    gog_cover: Option<&str>,
) -> Option<String> {
    let (path, urls) = locate_banner(cache_dir, source, game_id, gog_cover)?;
    let what = format!("banner for {source} game {game_id}");
    if let Some(bytes) = tokio_read(&path).await {
        return Some(data_url(&path, &bytes));
    }
    let mut bytes = download(client, &urls).await;
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
    store(&path, &bytes, &what);
    Some(data_url(&path, &bytes))
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
    Some(data_url(path, &bytes))
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

/// Encodes an image as a `data:` URL, typed by the cache file's extension.
fn data_url(path: &Path, bytes: &[u8]) -> String {
    let mime = if path.extension().is_some_and(|e| e == "png") { "image/png" } else { "image/jpeg" };
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
        assert_eq!(path, cache.join("icons").join("gog").join("abc_gac_60.jpg"));
        assert_eq!(urls, ["https://images.gog.com/abc_gac_60.jpg"]);
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
        assert_eq!(path, cache.join("banners").join("gog").join("5a51d9_product_tile_extended_432x243.jpg"));
        assert_eq!(urls, ["https://images.gog-statics.com/5a51d9_product_tile_extended_432x243.jpg"]);
        assert!(locate_banner(cache, Source::Gog, "gog_1", None).is_none());
        assert!(locate_banner(cache, Source::Gog, "gog_1", Some("https://evil.example/5a51d9_{formatter}.png")).is_none());
        assert!(locate_banner(cache, Source::Gog, "gog_1", Some("https://images.gog-statics.com/../x_{formatter}.png")).is_none());
        assert!(locate_banner(cache, Source::Gog, "gog_1", Some("https://images.gog-statics.com/5a51d9.png")).is_none());
    }
}
