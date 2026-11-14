//! Game names: installed games from `appmanifest_<appid>.acf`, others from the store API.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use super::paths::SteamPaths;
use super::vdf::{self, Vdf};

/// Library folders listed in `steamapps/libraryfolders.vdf`, plus the main install.
fn library_dirs(paths: &SteamPaths) -> Vec<PathBuf> {
    let mut dirs = vec![paths.root.join("steamapps")];
    if let Ok(text) = fs::read_to_string(paths.root.join("steamapps").join("libraryfolders.vdf")) {
        let doc = vdf::parse(&text);
        if let Some(folders) = doc.get("libraryfolders") {
            for (_, folder) in folders.entries() {
                if let Some(path) = folder.get("path").and_then(Vdf::as_str) {
                    let dir = PathBuf::from(path).join("steamapps");
                    if !dirs.contains(&dir) {
                        dirs.push(dir);
                    }
                }
            }
        }
    }
    dirs
}

pub fn installed_names(paths: &SteamPaths) -> HashMap<u32, String> {
    let mut names = HashMap::new();
    for dir in library_dirs(paths) {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let Some(file_name) = file_name.to_str() else { continue };
            if !(file_name.starts_with("appmanifest_") && file_name.ends_with(".acf")) {
                continue;
            }
            let Ok(text) = fs::read_to_string(entry.path()) else { continue };
            let doc = vdf::parse(&text);
            let Some(app) = doc.get("AppState") else { continue };
            let id = app.get("appid").and_then(Vdf::as_str).and_then(|s| s.parse().ok());
            let name = app.get("name").and_then(Vdf::as_str);
            if let (Some(id), Some(name)) = (id, name) {
                names.insert(id, name.to_string());
            }
        }
    }
    names
}

/// Looks up a game name on the public store API. `None` for delisted or unknown apps.
pub async fn fetch_store_name(client: &reqwest::Client, app_id: u32) -> Option<String> {
    let url = format!("https://store.steampowered.com/api/appdetails?appids={app_id}&filters=basic");
    let body: serde_json::Value = client.get(url).send().await.ok()?.json().await.ok()?;
    body.get(app_id.to_string())?
        .get("data")?
        .get("name")?
        .as_str()
        .map(str::to_string)
}
