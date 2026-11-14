//! Achievement icons: downloaded once from the launcher's CDN, then served from the disk cache.

use std::path::{Path, PathBuf};

use base64::Engine;

use crate::achievement::Source;

/// Steam serves achievement icons from these hosts. Newer icons exist only on the first.
const STEAM_CDNS: [&str; 2] = [
    "https://shared.akamai.steamstatic.com/community_assets/images/apps",
    "https://cdn.akamai.steamstatic.com/steamcommunity/public/images/apps",
];

/// GOG stores full icon URLs; only ones on its image host are fetched.
const GOG_IMAGE_HOST: &str = "https://images.gog.com/";

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
    let bytes = match tokio_read(&path).await {
        Some(bytes) => bytes,
        None => {
            let Some(bytes) = download(client, &urls).await else {
                eprintln!("achievement-shelf: icon {icon} for {source} game {game_id} not found");
                return None;
            };
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = std::fs::write(&path, &bytes) {
                eprintln!("achievement-shelf: could not cache icon {}: {e}", path.display());
            }
            bytes
        }
    };
    let mime = if icon.ends_with(".png") { "image/png" } else { "image/jpeg" };
    Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
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
}
