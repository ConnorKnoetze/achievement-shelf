//! Achievement icons: downloaded once from the Steam CDN, then served from the disk cache.

use std::path::Path;

use base64::Engine;

/// Steam serves achievement icons from these hosts. Newer icons exist only on the first.
const CDNS: [&str; 2] = [
    "https://shared.akamai.steamstatic.com/community_assets/images/apps",
    "https://cdn.akamai.steamstatic.com/steamcommunity/public/images/apps",
];

fn is_safe_file_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
}

/// Returns the icon as a `data:` URL, or `None` if it is not cached and cannot be downloaded.
pub async fn icon_data_url(client: &reqwest::Client, cache_dir: &Path, app_id: u32, icon: &str) -> Option<String> {
    // The name comes from a Steam file; refuse anything that could escape the cache folder.
    if !is_safe_file_name(icon) || icon.starts_with('.') {
        return None;
    }
    let path = cache_dir.join("icons").join(app_id.to_string()).join(icon);
    let bytes = match tokio_read(&path).await {
        Some(bytes) => bytes,
        None => {
            let bytes = download(client, app_id, icon).await?;
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = std::fs::write(&path, &bytes) {
                eprintln!("steam-shelf: could not cache icon {}: {e}", path.display());
            }
            bytes
        }
    };
    let mime = if icon.ends_with(".png") { "image/png" } else { "image/jpeg" };
    Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

async fn download(client: &reqwest::Client, app_id: u32, icon: &str) -> Option<Vec<u8>> {
    for cdn in CDNS {
        let Ok(response) = client.get(format!("{cdn}/{app_id}/{icon}")).send().await else { continue };
        if let Ok(response) = response.error_for_status() {
            if let Ok(bytes) = response.bytes().await {
                return Some(bytes.to_vec());
            }
        }
    }
    eprintln!("steam-shelf: icon {icon} for app {app_id} not found on any CDN");
    None
}

async fn tokio_read(path: &Path) -> Option<Vec<u8>> {
    let path = path.to_path_buf();
    tauri::async_runtime::spawn_blocking(move || std::fs::read(path).ok())
        .await
        .ok()
        .flatten()
}
