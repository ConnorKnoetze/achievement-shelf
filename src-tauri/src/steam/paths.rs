//! Finds the Steam install and the active account.

use std::fs;
use std::path::{Path, PathBuf};

use super::vdf;

const STEAMID64_BASE: u64 = 76_561_197_960_265_728;

#[derive(Debug, Clone)]
pub struct SteamPaths {
    pub root: PathBuf,
    /// 32-bit account ID, as used in `UserGameStats_<account>_<appid>.bin`.
    pub account_id: u32,
}

impl SteamPaths {
    pub fn stats_dir(&self) -> PathBuf {
        self.root.join("appcache").join("stats")
    }

    /// The client's library page cache for one game, which includes some achievement rarities.
    pub fn library_cache_path(&self, app_id: u32) -> PathBuf {
        self.root
            .join("userdata")
            .join(self.account_id.to_string())
            .join("config")
            .join("librarycache")
            .join(format!("{app_id}.json"))
    }

    pub fn locate() -> Result<Self, String> {
        let root = steam_root().ok_or("Steam installation not found")?;
        let account_id = account_id(&root).ok_or("No Steam account with achievement stats found")?;
        Ok(Self { root, account_id })
    }
}

#[cfg(windows)]
fn registry_steam_path() -> Option<PathBuf> {
    use winreg::enums::HKEY_CURRENT_USER;
    let key = winreg::RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Valve\Steam").ok()?;
    let path: String = key.get_value("SteamPath").ok()?;
    Some(PathBuf::from(path))
}

#[cfg(not(windows))]
fn registry_steam_path() -> Option<PathBuf> {
    None
}

fn steam_root() -> Option<PathBuf> {
    registry_steam_path()
        .into_iter()
        .chain([PathBuf::from(r"C:\Program Files (x86)\Steam")])
        .find(|p| p.join("appcache").join("stats").is_dir())
}

/// Account IDs that have stats files, from file names like `UserGameStats_<account>_<appid>.bin`.
fn accounts_with_stats(root: &Path) -> Vec<u32> {
    let mut ids: Vec<u32> = fs::read_dir(root.join("appcache").join("stats"))
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            let rest = name.strip_prefix("UserGameStats_")?;
            rest.split('_').next()?.parse().ok()
        })
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// The account marked `MostRecent` in `config/loginusers.vdf`.
fn most_recent_login(root: &Path) -> Option<u32> {
    let text = fs::read_to_string(root.join("config").join("loginusers.vdf")).ok()?;
    let users = vdf::parse(&text);
    let users = users.get("users")?;
    users.entries().iter().find_map(|(steam_id, user)| {
        let recent = user.get("MostRecent").and_then(vdf::Vdf::as_str) == Some("1");
        let id64: u64 = steam_id.parse().ok()?;
        recent.then(|| id64.checked_sub(STEAMID64_BASE)).flatten().map(|id| id as u32)
    })
}

fn account_id(root: &Path) -> Option<u32> {
    let candidates = accounts_with_stats(root);
    match most_recent_login(root) {
        Some(recent) if candidates.contains(&recent) => Some(recent),
        _ => candidates.first().copied(),
    }
}
