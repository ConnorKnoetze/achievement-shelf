pub mod achievement;
pub mod ea;
pub mod gog;
pub mod retro;
pub mod steam;
pub mod ubisoft;
pub mod xbox;

mod accounts;
mod browser;
mod icons;
mod processes;
mod rarity;
mod settings;
mod share;
mod store;
mod watch;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{
    AppHandle, Emitter, Manager, Monitor, PhysicalPosition, PhysicalSize, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

use achievement::{game_key, Achievement, Source};
use ea::EaPaths;
use gog::GogPaths;
use rarity::RarityCache;
use retro::Retro;
use settings::{Settings, Theme, TrophyKey, MAX_LEVELS, MAX_PER_ROW, MAX_SCALE, MIN_PER_ROW, MIN_SCALE};
use steam::SteamPaths;
use ubisoft::UbisoftPaths;
use xbox::Xbox;

// Shelf layout in CSS pixels; must match the sizes in src/shelf.css.
/// Width of a shelf without its achievements: padding and the rail's ends.
const SHELF_BASE_WIDTH: f64 = 64.0;
/// Width each achievement adds to a shelf: a slab and the gap beside it.
const SLOT_WIDTH: f64 = 76.0;
const ROW_HEIGHT: f64 = 128.0;
const PICKER_LABEL: &str = "picker";
const SETTINGS_LABEL: &str = "settings";
const UPDATE_LABEL: &str = "updater";
/// Sent to the update window with an [`UpdateStatus`].
const UPDATE_EVENT: &str = "update-status";
/// How long the shelf waits on the update check before showing anyway.
const UPDATE_CHECK_TIMEOUT: Duration = Duration::from_secs(10);
/// The shortest time the update window shows a check that found nothing.
const UPDATE_CHECK_MIN_SHOWN: Duration = Duration::from_millis(1200);
/// Sent with a fresh [`SettingsView`] whenever a setting changes, from the tray or the settings window.
const SETTINGS_EVENT: &str = "settings-changed";
/// Width of a shelf holding `per_row` achievements in each row, in CSS pixels.
fn shelf_width(per_row: usize) -> f64 {
    SHELF_BASE_WIDTH + SLOT_WIDTH * per_row as f64
}

/// Sent with the new level counts and row length as soon as they change, so the shelf can lay them out
/// before the full redraw that follows reaches it.
const LAYOUT_EVENT: &str = "shelf-layout";

#[derive(Debug, Clone, Copy, Deserialize)]
struct HitRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

struct AppState {
    client: reqwest::Client,
    cache_dir: PathBuf,
    /// Data kept for good, such as what has been read from the EA app's rotating log.
    data_dir: PathBuf,
    settings_path: PathBuf,
    settings: Mutex<Settings>,
    /// Game names known so far, keyed by [`game_key`]: from Steam appmanifests, the GOG
    /// database, the Steam store API, or the disk cache.
    names: Mutex<HashMap<String, String>>,
    /// Steam app IDs the store had no name for, with when it was last asked, so they are not
    /// asked about again on every load.
    name_misses: Mutex<HashMap<u32, u64>>,
    /// Steam rarities fetched from the Web API.
    rarity: RarityCache,
    /// Shelf area in the window, in physical pixels. Outside it, clicks pass through.
    hit_rect: Arc<Mutex<Option<HitRect>>>,
    /// Achievement images ready to drag out, by the id the shelf gave them (`prepare_share`).
    shares: Mutex<HashMap<String, PreparedShare>>,
    /// Wakes the thread that syncs RetroAchievements.
    retro: retro::Waker,
    /// Wakes the thread that syncs Xbox.
    xbox: xbox::Waker,
}

impl AppState {
    fn names_path(&self) -> PathBuf {
        self.cache_dir.join("names.json")
    }

    fn name_misses_path(&self) -> PathBuf {
        self.cache_dir.join("name-misses.json")
    }
}

/// A Steam game the store had no name for (delisted, or the request failed) is asked about
/// again after an hour.
const NAME_RETRY_AFTER_SECS: u64 = 3600;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ShelfItem {
    #[serde(flatten)]
    achievement: Achievement,
    game_name: String,
    icon_url: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Shelf {
    /// Rows of recent achievements (may be none).
    rows: usize,
    /// Rows of the trophy shelf (at least one, possibly empty).
    trophy_rows: usize,
    per_row: usize,
    /// The trophies on each trophy level, top first. A level may be empty.
    trophies: Vec<Vec<ShelfItem>>,
    /// Whether each trophy level has no room for another trophy. Trophies no longer found on
    /// disk are not shown but still take room.
    trophy_full: Vec<bool>,
    items: Vec<ShelfItem>,
    /// Banner `data:` URLs keyed by [`game_key`], once per game; games without one are left out.
    banners: HashMap<String, String>,
}

/// An unlocked achievement as listed in the trophy picker. Icons are fetched separately.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PickerItem {
    #[serde(flatten)]
    achievement: Achievement,
    game_name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PickerData {
    /// Launchers found on this PC, so the picker can tell "not installed" from "nothing unlocked".
    launchers: Vec<Source>,
    /// Trophies on each rail of the shelf.
    per_row: usize,
    /// Trophy levels on the shelf, and whether the picker can add or remove one.
    trophy_rows: usize,
    can_add_trophy_row: bool,
    can_remove_trophy_row: bool,
    /// Every saved trophy level, top first, including any past `trophy_rows`.
    trophies: Vec<Vec<TrophyKey>>,
    items: Vec<PickerItem>,
    /// Banner URLs (templates, for GOG) from the launchers' files, keyed by [`game_key`],
    /// passed back to `get_banner`.
    covers: HashMap<String, String>,
}

struct Unlocked {
    /// Every launcher's unlocked achievements, newest first.
    achievements: Vec<Achievement>,
    /// Game names the launchers know locally, keyed by [`game_key`].
    names: HashMap<String, String>,
    /// Banner URLs (templates, for GOG) the launchers' files name, keyed by [`game_key`].
    covers: HashMap<String, String>,
    launchers: Vec<Source>,
}

/// Reads every launcher that is installed. Fails only when none is.
async fn load_unlocked(data_dir: PathBuf) -> Result<Unlocked, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut unlocked = Unlocked {
            achievements: Vec::new(),
            names: HashMap::new(),
            covers: HashMap::new(),
            launchers: Vec::new(),
        };
        if let Ok(paths) = SteamPaths::locate() {
            unlocked.launchers.push(Source::Steam);
            unlocked.achievements.extend(steam::stats::load_all(&paths));
            let names = steam::names::installed_names(&paths);
            unlocked.names.extend(names.into_iter().map(|(id, name)| (game_key(Source::Steam, &id.to_string()), name)));
        }
        if let Ok(gog) = GogPaths::locate() {
            unlocked.launchers.push(Source::Gog);
            match gog.load_all() {
                Ok(list) => unlocked.achievements.extend(list),
                Err(e) => eprintln!("achievement-shelf: cannot read GOG achievements: {e}"),
            }
            match gog.names() {
                Ok(names) => unlocked.names.extend(names),
                Err(e) => eprintln!("achievement-shelf: cannot read GOG game names: {e}"),
            }
            match gog.banners() {
                Ok(banners) => unlocked.covers.extend(banners),
                Err(e) => eprintln!("achievement-shelf: cannot read GOG game banners: {e}"),
            }
        }
        if let Ok(ubisoft) = UbisoftPaths::locate() {
            unlocked.launchers.push(Source::Ubisoft);
            unlocked.achievements.extend(ubisoft.load_all());
            unlocked.names.extend(ubisoft.names());
            unlocked.covers.extend(ubisoft.banners());
        }
        if let Ok(ea) = EaPaths::locate(&data_dir) {
            unlocked.launchers.push(Source::Ea);
            unlocked.achievements.extend(ea.load_all());
            unlocked.names.extend(ea.names());
        }
        if let Some(retro) = Retro::locate(&data_dir) {
            unlocked.launchers.push(Source::Retro);
            unlocked.achievements.extend(retro.load_all());
            unlocked.names.extend(retro.names());
            unlocked.covers.extend(retro.banners());
        }
        if let Some(xbox) = Xbox::locate(&data_dir) {
            unlocked.launchers.push(Source::Xbox);
            unlocked.achievements.extend(xbox.load_all());
            unlocked.names.extend(xbox.names());
            unlocked.covers.extend(xbox.banners());
        }
        if unlocked.launchers.is_empty() {
            return Err("No supported launcher found".to_string());
        }
        unlocked.achievements.sort_by(|a, b| b.unlocked_at.cmp(&a.unlocked_at));
        Ok(unlocked)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Makes sure every game in `achievements` has a name, asking the Steam store only for
/// Steam games we have never seen (several at once), and saves what it learns to the disk
/// cache. A game the store had no name for is not asked about again until
/// [`NAME_RETRY_AFTER_SECS`] pass. GOG names all come from its local database.
async fn resolve_names(state: &AppState, local: HashMap<String, String>, achievements: &[&Achievement]) {
    state.names.lock().unwrap().extend(local);
    let mut app_ids: Vec<u32> = achievements
        .iter()
        .filter(|a| a.source == Source::Steam)
        .filter_map(|a| a.game_id.parse().ok())
        .collect();
    app_ids.sort_unstable();
    app_ids.dedup();

    let checked_at = rarity::now();
    let unknown: Vec<u32> = {
        let names = state.names.lock().unwrap();
        let misses = state.name_misses.lock().unwrap();
        app_ids
            .into_iter()
            .filter(|id| !names.contains_key(&game_key(Source::Steam, &id.to_string())))
            .filter(|id| !misses.get(id).is_some_and(|&at| checked_at.saturating_sub(at) < NAME_RETRY_AFTER_SECS))
            .collect()
    };
    if unknown.is_empty() {
        return;
    }

    for chunk in unknown.chunks(rarity::CONCURRENT_REQUESTS) {
        let tasks: Vec<_> = chunk
            .iter()
            .map(|&app_id| {
                let client = state.client.clone();
                tauri::async_runtime::spawn(async move { (app_id, steam::names::fetch_store_name(&client, app_id).await) })
            })
            .collect();
        for task in tasks {
            let Ok((app_id, name)) = task.await else { continue };
            match name {
                Some(name) => {
                    state.names.lock().unwrap().insert(game_key(Source::Steam, &app_id.to_string()), name);
                    state.name_misses.lock().unwrap().remove(&app_id);
                }
                None => {
                    state.name_misses.lock().unwrap().insert(app_id, checked_at);
                }
            }
        }
    }

    let names = serde_json::to_string(&*state.names.lock().unwrap()).unwrap_or_default();
    let misses = serde_json::to_string(&*state.name_misses.lock().unwrap()).unwrap_or_default();
    let _ = std::fs::create_dir_all(&state.cache_dir);
    let _ = std::fs::write(state.names_path(), names);
    let _ = std::fs::write(state.name_misses_path(), misses);
}

fn game_name(state: &AppState, achievement: &Achievement) -> String {
    let known = state.names.lock().unwrap().get(&achievement.game_key()).cloned();
    known.unwrap_or_else(|| match achievement.source {
        Source::Steam => format!("App {}", achievement.game_id),
        Source::Gog => format!("GOG game {}", achievement.game_id),
        Source::Ubisoft => format!("Ubisoft game {}", achievement.game_id),
        Source::Ea => format!("EA game {}", achievement.game_id),
        Source::Retro => format!("RetroAchievements game {}", achievement.game_id),
        Source::Xbox => format!("Xbox game {}", achievement.game_id),
    })
}

async fn shelf_items(state: &AppState, achievements: Vec<Achievement>) -> Vec<ShelfItem> {
    let mut items = Vec::with_capacity(achievements.len());
    for achievement in achievements {
        let icon_url = icons::icon_data_url(
            &state.client,
            &state.cache_dir,
            achievement.source,
            &achievement.game_id,
            &achievement.icon,
        )
        .await;
        let game_name = game_name(state, &achievement);
        items.push(ShelfItem { achievement, game_name, icon_url });
    }
    items
}

/// Banners of every game in `achievements`, fetched once per game.
async fn banners(
    state: &AppState,
    covers: &HashMap<String, String>,
    achievements: &[&Achievement],
) -> HashMap<String, String> {
    let mut banners = HashMap::new();
    for achievement in achievements {
        let key = achievement.game_key();
        if banners.contains_key(&key) {
            continue;
        }
        let cover = covers.get(&key).map(String::as_str);
        if let Some(url) =
            icons::banner_data_url(&state.client, &state.cache_dir, achievement.source, &achievement.game_id, cover)
                .await
        {
            banners.insert(key, url);
        }
    }
    banners
}

#[tauri::command]
async fn get_shelf(state: State<'_, AppState>) -> Result<Shelf, String> {
    let (rows, trophy_rows, per_row, recent_capacity, trophy_levels, trophy_full) = {
        let settings = state.settings.lock().unwrap();
        (
            settings.rows,
            settings.trophy_rows,
            settings.per_row,
            settings.recent_capacity(),
            settings.shown_trophy_levels(),
            settings.full_trophy_levels(),
        )
    };

    let Unlocked { achievements: mut recent, names, covers, .. } = load_unlocked(state.data_dir.clone()).await?;
    // Each level keeps the trophies and order the user chose. Ones no longer found on disk are
    // skipped, and ones past what a level holds are not shown.
    let found: Vec<Vec<Achievement>> = trophy_levels
        .iter()
        .map(|level| {
            level.iter().filter_map(|key| recent.iter().find(|a| key.matches(a)).cloned()).take(per_row).collect()
        })
        .collect();
    let level_sizes: Vec<usize> = found.iter().map(Vec::len).collect();
    let mut trophies: Vec<Achievement> = found.into_iter().flatten().collect();
    recent.truncate(recent_capacity);

    let shown: Vec<&Achievement> = recent.iter().chain(&trophies).collect();
    resolve_names(&state, names, &shown).await;
    let banners = banners(&state, &covers, &shown).await;
    state.rarity.fill(&state.client, &mut recent).await;
    state.rarity.fill(&state.client, &mut trophies).await;
    let mut trophies = shelf_items(&state, trophies).await.into_iter();
    let trophies = level_sizes.into_iter().map(|size| trophies.by_ref().take(size).collect()).collect();
    Ok(Shelf {
        rows,
        trophy_rows,
        per_row,
        trophies,
        trophy_full,
        items: shelf_items(&state, recent).await,
        banners,
    })
}

/// Every unlocked achievement, newest first, for the trophy picker.
#[tauri::command]
async fn get_unlocked(state: State<'_, AppState>) -> Result<PickerData, String> {
    let Unlocked { mut achievements, names, launchers, covers } = load_unlocked(state.data_dir.clone()).await?;
    resolve_names(&state, names, &achievements.iter().collect::<Vec<_>>()).await;
    state.rarity.fill(&state.client, &mut achievements).await;
    let items = achievements
        .into_iter()
        .map(|achievement| PickerItem { game_name: game_name(&state, &achievement), achievement })
        .collect();
    let settings = state.settings.lock().unwrap().clone();
    Ok(PickerData {
        launchers,
        per_row: settings.per_row,
        trophy_rows: settings.trophy_rows,
        can_add_trophy_row: settings.can_add_row(),
        can_remove_trophy_row: settings.can_remove_trophy_row(),
        trophies: settings.trophy_levels,
        items,
        covers,
    })
}

/// A game's banner as a `data:` URL for the picker's catalog, or `None` when it has none.
/// `cover` is the URL (template, for GOG) from `PickerData::covers`; `icons` checks its host.
#[tauri::command]
async fn get_banner(
    state: State<'_, AppState>,
    source: Source,
    game_id: String,
    cover: Option<String>,
) -> Result<Option<String>, String> {
    Ok(icons::banner_data_url(&state.client, &state.cache_dir, source, &game_id, cover.as_deref()).await)
}

#[tauri::command]
async fn get_icon(
    state: State<'_, AppState>,
    source: Source,
    game_id: String,
    icon: String,
) -> Result<Option<String>, String> {
    Ok(icons::icon_data_url(&state.client, &state.cache_dir, source, &game_id, &icon).await)
}

/// Adds or removes a trophy and returns every trophy level. A trophy is added to the first
/// level with room; fails when every level is full.
#[tauri::command]
fn set_trophy(
    app: AppHandle,
    state: State<'_, AppState>,
    key: TrophyKey,
    trophy: bool,
) -> Result<Vec<Vec<TrophyKey>>, String> {
    let trophies = {
        let mut settings = state.settings.lock().unwrap();
        if trophy {
            settings.add_trophy(key)?;
        } else {
            settings.remove_trophy(&key);
        }
        settings.save(&state.settings_path);
        settings.trophy_levels.clone()
    };
    let _ = app.emit(watch::UPDATED_EVENT, ());
    Ok(trophies)
}

/// Rearranges the trophies on the levels shown: `levels` lists, top first, the trophies the
/// shelf or picker shows on each level (see [`settings::Settings::move_trophies`]).
#[tauri::command]
fn move_trophies(app: AppHandle, state: State<'_, AppState>, levels: Vec<Vec<TrophyKey>>) -> Result<(), String> {
    {
        let mut settings = state.settings.lock().unwrap();
        settings.move_trophies(levels)?;
        settings.save(&state.settings_path);
    }
    let _ = app.emit(watch::UPDATED_EVENT, ());
    Ok(())
}

#[tauri::command]
fn clear_trophies(app: AppHandle, state: State<'_, AppState>) {
    {
        let mut settings = state.settings.lock().unwrap();
        settings.clear_trophies();
        settings.save(&state.settings_path);
    }
    let _ = app.emit(watch::UPDATED_EVENT, ());
}

#[tauri::command]
fn set_hit_rect(state: State<'_, AppState>, rect: HitRect) {
    *state.hit_rect.lock().unwrap() = Some(rect);
}

/// Shared images are kept this long, so a chat app still uploading one can read it.
const SHARE_KEEP: Duration = Duration::from_secs(24 * 60 * 60);

/// A file name for a shared image of the achievement `title`, safe on every platform.
fn share_file_name(title: &str) -> String {
    let name: String = title
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { ' ' })
        .collect();
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let name: String = name.chars().take(60).collect();
    if name.is_empty() { "Achievement.png".into() } else { format!("{name}.png") }
}

/// An achievement's images, ready to drag out (`prepare_share`).
struct PreparedShare {
    /// The PNG, saved at `path`; kept to save it again if it has been cleared out since.
    png: Vec<u8>,
    path: PathBuf,
    /// What follows the cursor: straight RGBA rows, top first.
    preview: Vec<u8>,
    preview_width: u32,
    preview_height: u32,
    /// Whether it has been dragged out, so something may still be reading the file.
    shared: bool,
}

/// Where shared images are saved, each in a folder of its own.
fn share_dir() -> PathBuf {
    std::env::temp_dir().join("achievement-shelf-share")
}

/// Clears out images shared long enough ago that nothing is still reading them.
fn clean_old_shares() {
    let Ok(entries) = std::fs::read_dir(share_dir()) else { return };
    for entry in entries.flatten() {
        let old = entry.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
        if old.is_some_and(|age| age > SHARE_KEEP) {
            let path = entry.path();
            let _ = if path.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) };
        }
    }
}

/// Lets go of the images prepared as `ids`, deleting their files unless they were dragged out:
/// those are left for `clean_old_shares`, as an app they were dropped into may still read them.
#[tauri::command]
fn forget_shares(state: State<'_, AppState>, ids: Vec<String>) {
    let mut shares = state.shares.lock().unwrap();
    for id in ids {
        let Some(share) = shares.remove(&id) else { continue };
        if !share.shared {
            if let Some(folder) = share.path.parent() {
                let _ = std::fs::remove_dir_all(folder);
            }
        }
    }
}

/// The shelf keeps no more prepared images than this; it names the ones it lets go.
const MAX_SHARES: usize = 256;

/// Readies an achievement to be dragged out, ahead of the drag, so the drag itself only names
/// it: decodes `image` (a PNG of its slab and certificate, base64) and saves it to a temporary
/// file, and keeps `preview` (RGBA pixels, base64, `preview_width` wide) to show under the
/// cursor.
#[tauri::command]
async fn prepare_share(
    state: State<'_, AppState>,
    id: String,
    title: String,
    image: String,
    preview: String,
    preview_width: u32,
) -> Result<(), String> {
    use base64::Engine;
    if id.is_empty() || id.len() > 32 || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("the share id must be a short run of letters and digits".into());
    }
    let decode = |data: &str| base64::engine::general_purpose::STANDARD.decode(data).map_err(|e| e.to_string());
    let png = decode(&image)?;
    let rgba = decode(&preview)?;
    let row = preview_width as usize * 4;
    if row == 0 || rgba.len() % row != 0 {
        return Err("the drag preview is not whole rows of pixels".into());
    }
    let preview_height = (rgba.len() / row) as u32;

    // Also run at startup; here too, for a shelf left running for days.
    clean_old_shares();
    // A folder of its own, so achievements with the same title don't share a file. The process
    // id keeps ids reused by a later run from writing over a file still being read.
    let path = share_dir().join(format!("{}-{id}", std::process::id())).join(share_file_name(&title));
    write_share(&path, &png)?;

    let mut shares = state.shares.lock().unwrap();
    if shares.len() >= MAX_SHARES && !shares.contains_key(&id) {
        return Err("too many achievement images are kept".into());
    }
    shares.insert(id, PreparedShare { png, path, preview: rgba, preview_width, preview_height, shared: false });
    Ok(())
}

fn write_share(path: &Path, png: &[u8]) -> Result<(), String> {
    std::fs::create_dir_all(path.parent().expect("share files sit in a folder")).map_err(|e| e.to_string())?;
    std::fs::write(path, png).map_err(|e| e.to_string())
}

/// Drags the achievement prepared as `id` out of the shelf as its image file, so it can be
/// dropped into a chat app such as Discord. Its preview is shown under the cursor, held by the
/// pixel at `hotspot`. Must be called while the mouse button is still down; the drag ends when
/// it is released, or hands the slab back to the shelf when the cursor comes back over `shelf`
/// (in the page's CSS px; `SHARE_RETURNED_EVENT`).
#[tauri::command]
async fn share_drag(
    window: WebviewWindow,
    state: State<'_, AppState>,
    id: String,
    hotspot: (i32, i32),
    swing: Option<share::Swing>,
    pixel_ratio: f64,
    shelf: HitRect,
) -> Result<(), String> {
    let finite = swing.is_none_or(|s| [s.angle, s.rate, s.speed].iter().all(|v| v.is_finite()));
    if !finite || !pixel_ratio.is_finite() || pixel_ratio <= 0.0 {
        return Err("the drag's swing is not a set of finite numbers".into());
    }
    let (path, preview) = {
        let mut shares = state.shares.lock().unwrap();
        let share = shares.get_mut(&id).ok_or("that achievement image is not prepared")?;
        share.shared = true;
        // Cleared out by a long-running shelf: saved again.
        if !share.path.exists() {
            write_share(&share.path, &share.png)?;
        }
        let preview = share::DragImage {
            rgba: share.preview.clone(),
            width: share.preview_width,
            height: share.preview_height,
            hotspot,
            swing,
            pixel_ratio,
        };
        (share.path.clone(), preview)
    };

    // The window stays put while dragging, so its place is read here rather than from inside
    // the drag loop, which holds up the event loop.
    let origin = window.inner_position().map_err(|e| e.to_string())?;
    let over_shelf: share::OverShelf = Box::new(move |x, y| {
        let (x, y) = ((x - origin.x) as f64 / pixel_ratio, (y - origin.y) as f64 / pixel_ratio);
        x >= shelf.x && x <= shelf.x + shelf.width && y >= shelf.y && y <= shelf.y + shelf.height
    });

    // The OS drag loop blocks until the drop, and must run on the UI thread.
    let target = window.clone();
    window
        .run_on_main_thread(move || match share::drag_file(&path, preview, over_shelf) {
            Ok(share::DragEnd::Returned { x, y, swing }) => {
                let at = ((x - origin.x) as f64 / pixel_ratio, (y - origin.y) as f64 / pixel_ratio);
                let _ = target.emit_to(target.label(), SHARE_RETURNED_EVENT, ShareReturned { x: at.0, y: at.1, swing });
            }
            ended => {
                if let Err(e) = ended {
                    eprintln!("achievement-shelf: could not drag out the achievement: {e}");
                }
                let _ = target.emit_to(target.label(), SHARE_ENDED_EVENT, ());
            }
        })
        .map_err(|e| e.to_string())
}

/// Sent to the shelf when a slab dragged out is brought back over it with the button down.
const SHARE_RETURNED_EVENT: &str = "share-drag-returned";
/// Sent to the shelf when a drag out ends anywhere else: dropped, cancelled or failed.
const SHARE_ENDED_EVENT: &str = "share-drag-ended";

/// Where the cursor came back, in the shelf page's CSS px, and how the slab was swinging.
#[derive(Clone, Serialize)]
struct ShareReturned {
    x: f64,
    y: f64,
    swing: share::Swing,
}

/// The chosen display, or the main one if none was chosen or it is not connected.
fn shelf_monitor(window: &WebviewWindow, display: Option<&str>) -> tauri::Result<Option<Monitor>> {
    if let Some(name) = display {
        if let Some(m) = window.available_monitors()?.into_iter().find(|m| m.name().is_some_and(|n| n == name)) {
            return Ok(Some(m));
        }
    }
    Ok(match window.primary_monitor()? {
        Some(m) => Some(m),
        None => window.current_monitor()?,
    })
}

/// Page zoom at which a shelf of [`MAX_LEVELS`] levels fills the height of `monitor`'s work
/// area, with its top level at the top of the screen. This is the shelf's 100% size.
fn fit_zoom(monitor: &Monitor) -> f64 {
    let height = monitor.work_area().size.height as f64 / monitor.scale_factor();
    height / (MAX_LEVELS as f64 * ROW_HEIGHT)
}

/// Largest scale at which the settings' shelf, with its current levels and row length, still
/// fits `monitor`'s work area, up to [`MAX_SCALE`]. Fewer levels or shorter rows leave room to
/// draw the shelf larger than 100%.
fn max_scale(monitor: &Monitor, settings: &Settings) -> f64 {
    let area = monitor.work_area().size;
    let width = area.width as f64 / monitor.scale_factor();
    let height = area.height as f64 / monitor.scale_factor();
    let fit = (height / (settings.total_rows() as f64 * ROW_HEIGHT)).min(width / shelf_width(settings.per_row));
    (fit / fit_zoom(monitor)).clamp(MIN_SCALE, MAX_SCALE)
}

/// Share of the work area's width a shelf of the default row length takes before a size is chosen.
const DEFAULT_WIDTH_SHARE: f64 = 1.0 / 3.0;
/// Share of the work area's height the default shelf's levels take before a size is chosen.
const DEFAULT_HEIGHT_SHARE: f64 = 0.3;

/// Scale for a shelf whose size has not been chosen: the default shelf (one recent row and one
/// trophy row of the default length) takes a fixed share of `monitor`'s work area, so it reads
/// alike on every screen. It is worked out from the default layout, not the current one, so
/// adding rows or achievements grows the shelf rather than shrinking each achievement.
fn default_scale(monitor: &Monitor) -> f64 {
    let defaults = Settings::default();
    let area = monitor.work_area().size;
    let width = area.width as f64 / monitor.scale_factor() * DEFAULT_WIDTH_SHARE;
    let height = area.height as f64 / monitor.scale_factor() * DEFAULT_HEIGHT_SHARE;
    let zoom = (width / shelf_width(defaults.per_row)).min(height / (defaults.total_rows() as f64 * ROW_HEIGHT));
    zoom / fit_zoom(monitor)
}

/// The scale the shelf is drawn at on `monitor`: the chosen scale, or one sized for the display
/// if none was chosen, made smaller if the shelf would not fit at it.
fn shown_scale(monitor: &Monitor, settings: &Settings) -> f64 {
    let scale = settings.scale.unwrap_or_else(|| default_scale(monitor));
    scale.clamp(MIN_SCALE, max_scale(monitor, settings))
}

/// Zooms the page so the shelf is drawn at its shown scale on `monitor`.
fn apply_zoom(window: &WebviewWindow, monitor: &Monitor, settings: &Settings) -> tauri::Result<()> {
    window.set_zoom(shown_scale(monitor, settings) * fit_zoom(monitor))
}

/// Zooms the shelf for its scale on the chosen display, after moving the window there.
fn apply_scale(window: &WebviewWindow, settings: &Settings) -> tauri::Result<()> {
    match shelf_monitor(window, settings.display.as_deref())? {
        Some(monitor) => apply_zoom(window, &monitor, settings),
        None => Ok(()),
    }
}

/// Puts the window in the bottom-right corner of the chosen display, above the taskbar, and
/// zooms the page for the shelf scale.
///
/// The window fills the work area whatever the settings, since the shelf is never drawn larger
/// than fits it (see `max_scale`), and it is click-through outside the shelf. So changing the
/// scale or the levels only zooms or redraws the page. Resizing and moving the window instead
/// shows each step as a separate frame, and the shelf jumps around on every change.
fn place_window(window: &WebviewWindow, settings: &Settings) -> tauri::Result<()> {
    let Some(monitor) = shelf_monitor(window, settings.display.as_deref())? else {
        return Ok(());
    };
    let area = monitor.work_area();
    let width = area.size.width;
    let height = area.size.height;
    // Moves onto the display before sizing: crossing to a display with another scale makes
    // Windows rescale the window, which would undo a size set first. On the same display that
    // move would only make the shelf jump, so it is skipped.
    let same_display = window.current_monitor()?.is_some_and(|m| m.name() == monitor.name());
    if !same_display {
        window.set_position(area.position)?;
    }
    let size = PhysicalSize::new(width, height);
    if window.inner_size()? != size {
        window.set_size(size)?;
    }
    let position = PhysicalPosition::new(
        area.position.x + area.size.width as i32 - width as i32,
        area.position.y + area.size.height as i32 - height as i32,
    );
    if window.outer_position()? != position {
        window.set_position(position)?;
    }
    apply_zoom(window, &monitor, settings)
}

fn apply_layer(window: &WebviewWindow, on_top: bool) -> tauri::Result<()> {
    if on_top {
        window.set_always_on_bottom(false)?;
        window.set_always_on_top(true)
    } else {
        window.set_always_on_top(false)?;
        window.set_always_on_bottom(true)
    }
}

/// The window is transparent around the shelf. While the cursor is outside the shelf,
/// clicks go to whatever is behind it; over the shelf, the window takes the mouse.
/// The webview gets no mouse events while ignoring them, so this polls from here.
fn spawn_hit_test(window: WebviewWindow, hit_rect: Arc<Mutex<Option<HitRect>>>) {
    std::thread::spawn(move || {
        let mut last: Option<bool> = None;
        loop {
            std::thread::sleep(Duration::from_millis(50));
            let (Ok(cursor), Ok(origin)) = (window.cursor_position(), window.inner_position()) else {
                continue;
            };
            let x = cursor.x - origin.x as f64;
            let y = cursor.y - origin.y as f64;
            let inside = hit_rect.lock().unwrap().is_some_and(|r| {
                x >= r.x && x <= r.x + r.width && y >= r.y && y <= r.y + r.height
            });
            if last != Some(inside) && window.set_ignore_cursor_events(!inside).is_ok() {
                last = Some(inside);
            }
        }
    });
}

/// Opens a secondary window, or brings it forward if it is already open.
fn open_window(app: &AppHandle, label: &str, page: &str, title: &str, size: (f64, f64), min: (f64, f64)) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(label) {
        window.unminimize()?;
        return window.set_focus();
    }
    WebviewWindowBuilder::new(app, label, WebviewUrl::App(page.into()))
        .title(title)
        .inner_size(size.0, size.1)
        .min_inner_size(min.0, min.1)
        .center()
        .build()?;
    Ok(())
}

fn open_picker(app: &AppHandle) -> tauri::Result<()> {
    open_window(app, PICKER_LABEL, "picker.html", "Choose trophies", (860.0, 640.0), (520.0, 360.0))
}

fn open_settings(app: &AppHandle) -> tauri::Result<()> {
    open_window(app, SETTINGS_LABEL, "settings.html", "Achievement Shelf settings", (420.0, 860.0), (380.0, 560.0))
}

/// Tray check items that mirror settings the settings window can also change.
struct TrayItems {
    on_top: CheckMenuItem<tauri::Wry>,
    autostart: CheckMenuItem<tauri::Wry>,
}

/// What the settings window shows, with the row limits already worked out.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsView {
    rows: usize,
    trophy_rows: usize,
    max_levels: usize,
    per_row: usize,
    min_per_row: usize,
    max_per_row: usize,
    /// Name of the chosen display; empty for the main display.
    display: String,
    displays: Vec<DisplayOption>,
    scale: f64,
    min_scale: f64,
    max_scale: f64,
    /// Whether the shelf is sized for its display rather than at a chosen scale.
    scale_is_default: bool,
    can_add_row: bool,
    can_remove_recent_row: bool,
    can_remove_trophy_row: bool,
    on_top: bool,
    autostart: bool,
    theme: Theme,
}

/// A display the shelf can be put on, for the settings window's list.
#[derive(Clone, Serialize)]
struct DisplayOption {
    name: String,
    label: String,
}

/// The connected displays, left to right, plus the chosen one if it is not connected.
fn display_options(app: &AppHandle, chosen: Option<&str>) -> Vec<DisplayOption> {
    let mut monitors = app.available_monitors().unwrap_or_default();
    monitors.sort_by_key(|m| (m.position().x, m.position().y));
    let primary = app.primary_monitor().ok().flatten().and_then(|m| m.name().cloned());
    let mut options: Vec<DisplayOption> = monitors
        .iter()
        .enumerate()
        .filter_map(|(i, m)| {
            let name = m.name()?.clone();
            let size = m.size();
            let main = if primary.as_ref() == Some(&name) { " (main)" } else { "" };
            Some(DisplayOption { label: format!("Display {} \u{b7} {} \u{d7} {}{main}", i + 1, size.width, size.height), name })
        })
        .collect();
    if let Some(chosen) = chosen.filter(|c| !options.iter().any(|o| o.name == *c)) {
        options.push(DisplayOption { name: chosen.to_owned(), label: format!("{chosen} (not connected)") });
    }
    options
}

fn settings_view(app: &AppHandle) -> SettingsView {
    let settings = app.state::<AppState>().settings.lock().unwrap().clone();
    // The slider runs up to the largest scale the shelf fits its display at, and shows the
    // scale it is drawn at, which a longer shelf may hold below the chosen one.
    let monitor = app
        .get_webview_window("main")
        .and_then(|window| shelf_monitor(&window, settings.display.as_deref()).ok().flatten());
    let (scale, max_scale) = match &monitor {
        Some(m) => (shown_scale(m, &settings), max_scale(m, &settings)),
        None => (settings.scale.unwrap_or(1.0), MAX_SCALE),
    };
    SettingsView {
        display: settings.display.clone().unwrap_or_default(),
        displays: display_options(app, settings.display.as_deref()),
        rows: settings.rows,
        trophy_rows: settings.trophy_rows,
        max_levels: MAX_LEVELS,
        per_row: settings.per_row,
        min_per_row: MIN_PER_ROW,
        max_per_row: MAX_PER_ROW,
        scale,
        min_scale: MIN_SCALE,
        max_scale,
        scale_is_default: settings.scale.is_none(),
        can_add_row: settings.can_add_row(),
        can_remove_recent_row: settings.can_remove_recent_row(),
        can_remove_trophy_row: settings.can_remove_trophy_row(),
        on_top: settings.on_top,
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        theme: settings.theme,
    }
}

/// Brings the tray checks and an open settings window in line with the current settings.
fn settings_changed(app: &AppHandle) {
    let view = settings_view(app);
    if let Some(tray) = app.try_state::<TrayItems>() {
        let _ = tray.on_top.set_checked(view.on_top);
        let _ = tray.autostart.set_checked(view.autostart);
    }
    let _ = app.emit(SETTINGS_EVENT, view);
}

/// Adds or removes a recent or trophy row; the shelf redraws with it.
fn change_rows(app: &AppHandle, trophy: bool, add: bool) {
    let state = app.state::<AppState>();
    {
        let mut settings = state.settings.lock().unwrap();
        if !settings.change_rows(trophy, if add { 1 } else { -1 }) {
            return;
        }
        settings.save(&state.settings_path);
        emit_layout(app, &settings);
        rezoom(app, &settings);
    }
    // Redraws the shelf and updates the picker's trophy capacity.
    let _ = app.emit(watch::UPDATED_EVENT, ());
    settings_changed(app);
}

/// Adds or removes an achievement from every row; the shelf redraws with it.
fn change_per_row(app: &AppHandle, add: bool) {
    let state = app.state::<AppState>();
    {
        let mut settings = state.settings.lock().unwrap();
        if !settings.change_per_row(if add { 1 } else { -1 }) {
            return;
        }
        settings.save(&state.settings_path);
        emit_layout(app, &settings);
        rezoom(app, &settings);
    }
    // Redraws the shelf and updates the picker's trophy capacity and rails.
    let _ = app.emit(watch::UPDATED_EVENT, ());
    settings_changed(app);
}

fn emit_layout(app: &AppHandle, settings: &Settings) {
    let layout = serde_json::json!({
        "rows": settings.rows,
        "trophyRows": settings.trophy_rows,
        "perRow": settings.per_row,
    });
    let _ = app.emit(LAYOUT_EVENT, layout);
}

/// Moves the shelf to the named display; an empty name means the main display.
fn set_display(app: &AppHandle, name: String) {
    let state = app.state::<AppState>();
    {
        let mut settings = state.settings.lock().unwrap();
        settings.display = Some(name).filter(|n| !n.is_empty());
        settings.save(&state.settings_path);
        if let Some(window) = app.get_webview_window("main") {
            if let Err(e) = place_window(&window, &settings) {
                eprintln!("achievement-shelf: could not move the shelf: {e}");
            }
        }
    }
    settings_changed(app);
}

/// Zooms the shelf for its scale, which the shelf's levels and row length limit.
fn rezoom(app: &AppHandle, settings: &Settings) {
    if let Some(window) = app.get_webview_window("main") {
        if let Err(e) = apply_scale(&window, settings) {
            eprintln!("achievement-shelf: could not resize the shelf: {e}");
        }
    }
}

/// Resizes the shelf to `scale` times its normal size, or with `None` back to the size for its display.
fn set_scale(app: &AppHandle, scale: Option<f64>) {
    let state = app.state::<AppState>();
    {
        let mut settings = state.settings.lock().unwrap();
        match scale {
            Some(scale) => settings.set_scale(scale),
            None => settings.reset_scale(),
        }
        settings.save(&state.settings_path);
        rezoom(app, &settings);
    }
    settings_changed(app);
}

fn set_on_top(app: &AppHandle, value: bool) {
    let state = app.state::<AppState>();
    {
        let mut settings = state.settings.lock().unwrap();
        settings.on_top = value;
        settings.save(&state.settings_path);
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = apply_layer(&window, value);
    }
    settings_changed(app);
}

/// Themes every window, open and future. Windows' title bars follow it, and so does each
/// page's `prefers-color-scheme`, which the window styles switch on. With no theme set, both
/// follow the Windows app theme as it changes.
fn apply_theme(app: &AppHandle, theme: Theme) {
    app.set_theme(match theme {
        Theme::Dark => Some(tauri::Theme::Dark),
        Theme::Light => Some(tauri::Theme::Light),
        Theme::System => None,
    });
}

fn set_theme(app: &AppHandle, theme: Theme) {
    let state = app.state::<AppState>();
    {
        let mut settings = state.settings.lock().unwrap();
        settings.theme = theme;
        settings.save(&state.settings_path);
    }
    apply_theme(app, theme);
    settings_changed(app);
}

/// Puts every setting back to its default and redraws the shelf with them. Trophies stay, and
/// so does Start with Windows, which lives in Windows rather than in the settings.
fn reset_all(app: &AppHandle) {
    let state = app.state::<AppState>();
    let theme = {
        let mut settings = state.settings.lock().unwrap();
        settings.reset();
        settings.save(&state.settings_path);
        emit_layout(app, &settings);
        if let Some(window) = app.get_webview_window("main") {
            if let Err(e) = place_window(&window, &settings) {
                eprintln!("achievement-shelf: could not move the shelf: {e}");
            }
            let _ = apply_layer(&window, settings.on_top);
        }
        settings.theme
    };
    apply_theme(app, theme);
    // Redraws the shelf and updates the picker's trophy capacity and rails.
    let _ = app.emit(watch::UPDATED_EVENT, ());
    settings_changed(app);
}

fn set_autostart(app: &AppHandle, value: bool) -> Result<(), String> {
    let manager = app.autolaunch();
    let result = if value { manager.enable() } else { manager.disable() };
    // Shows the real state either way, so a failed change does not leave a wrong check.
    settings_changed(app);
    result.map_err(|e| format!("Could not change Start with Windows: {e}"))
}

#[tauri::command]
fn get_settings(app: AppHandle) -> SettingsView {
    settings_view(&app)
}

#[tauri::command]
fn change_shelf_rows(app: AppHandle, trophy: bool, add: bool) -> SettingsView {
    change_rows(&app, trophy, add);
    settings_view(&app)
}

#[tauri::command]
fn change_shelf_per_row(app: AppHandle, add: bool) -> SettingsView {
    change_per_row(&app, add);
    settings_view(&app)
}

#[tauri::command]
fn set_shelf_display(app: AppHandle, name: String) -> SettingsView {
    set_display(&app, name);
    settings_view(&app)
}

#[tauri::command]
fn set_shelf_scale(app: AppHandle, scale: f64) -> SettingsView {
    set_scale(&app, Some(scale));
    settings_view(&app)
}

#[tauri::command]
fn reset_shelf_scale(app: AppHandle) -> SettingsView {
    set_scale(&app, None);
    settings_view(&app)
}

#[tauri::command]
fn set_keep_on_top(app: AppHandle, value: bool) -> SettingsView {
    set_on_top(&app, value);
    settings_view(&app)
}

#[tauri::command]
fn set_app_theme(app: AppHandle, theme: Theme) -> SettingsView {
    set_theme(&app, theme);
    settings_view(&app)
}

#[tauri::command]
fn reset_settings(app: AppHandle) -> SettingsView {
    reset_all(&app);
    settings_view(&app)
}

/// The signed-in RetroAchievements username, if any.
#[tauri::command]
async fn retro_account() -> Option<String> {
    tauri::async_runtime::spawn_blocking(retro::username).await.ok().flatten()
}

/// Signs in to RetroAchievements and starts syncing; returns the username.
#[tauri::command]
async fn retro_connect(state: State<'_, AppState>, username: String, api_key: String) -> Result<String, String> {
    let username = retro::connect(&state.client, &state.data_dir, &username, &api_key).await?;
    state.retro.sync_now();
    Ok(username)
}

/// Opens the RetroAchievements page that shows the user's Web API key.
#[tauri::command]
fn retro_open_key_page() -> Result<(), String> {
    retro::open_key_page()
}

#[tauri::command]
fn retro_disconnect(app: AppHandle, state: State<'_, AppState>) {
    retro::disconnect(&state.data_dir);
    let _ = app.emit(watch::UPDATED_EVENT, ());
}

#[derive(Serialize)]
struct XboxAccountView {
    /// Whether this build can sign in to Xbox at all.
    available: bool,
    gamertag: Option<String>,
}

#[tauri::command]
async fn xbox_account() -> XboxAccountView {
    let gamertag = tauri::async_runtime::spawn_blocking(xbox::gamertag).await.ok().flatten();
    XboxAccountView { available: xbox::available(), gamertag }
}

/// Signs in to Xbox in the user's browser and starts syncing; returns the gamertag.
#[tauri::command]
async fn xbox_connect(state: State<'_, AppState>) -> Result<String, String> {
    let gamertag = xbox::connect(&state.client, &state.data_dir).await?;
    state.xbox.sync_now();
    Ok(gamertag)
}

/// Stops a sign-in that is waiting for the browser.
#[tauri::command]
fn xbox_cancel() {
    xbox::cancel_sign_in();
}

#[tauri::command]
fn xbox_disconnect(app: AppHandle, state: State<'_, AppState>) {
    xbox::disconnect(&state.data_dir);
    let _ = app.emit(watch::UPDATED_EVENT, ());
}

#[tauri::command]
fn set_start_with_windows(app: AppHandle, value: bool) -> Result<SettingsView, String> {
    set_autostart(&app, value)?;
    Ok(settings_view(&app))
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        "refresh" => {
            icons::forget_misses();
            app.state::<AppState>().retro.sync_now();
            app.state::<AppState>().xbox.sync_now();
            let _ = app.emit(watch::UPDATED_EVENT, ());
        }
        "choose_trophies" => {
            if let Err(e) = open_picker(app) {
                eprintln!("achievement-shelf: could not open the trophy picker: {e}");
            }
        }
        "settings" => {
            if let Err(e) = open_settings(app) {
                eprintln!("achievement-shelf: could not open settings: {e}");
            }
        }
        // A check item has already toggled itself when the event arrives.
        id @ ("on_top" | "autostart") => {
            let Some(tray) = app.try_state::<TrayItems>() else { return };
            if id == "on_top" {
                set_on_top(app, tray.on_top.is_checked().unwrap_or(false));
            } else if let Err(e) = set_autostart(app, tray.autostart.is_checked().unwrap_or(false)) {
                eprintln!("achievement-shelf: {e}");
            }
        }
        "preview_update" => preview_update_window(app.clone()),
        "quit" => app.exit(0),
        _ => {}
    }
}

fn build_tray(app: &AppHandle, settings: &Settings) -> tauri::Result<()> {
    let refresh = MenuItem::with_id(app, "refresh", "Refresh", true, None::<&str>)?;
    let choose_trophies = MenuItem::with_id(app, "choose_trophies", "Choose trophies...", true, None::<&str>)?;
    let open_settings = MenuItem::with_id(app, "settings", "Settings...", true, None::<&str>)?;
    let on_top = CheckMenuItem::with_id(app, "on_top", "Keep above other windows", true, settings.on_top, None::<&str>)?;
    let autostart_enabled = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart = CheckMenuItem::with_id(app, "autostart", "Start with Windows", true, autostart_enabled, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &refresh,
            &PredefinedMenuItem::separator(app)?,
            &choose_trophies,
            &open_settings,
            &on_top,
            &autostart,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    // Dev builds never check for updates, so this is the way to see the update window.
    if cfg!(debug_assertions) {
        let preview = MenuItem::with_id(app, "preview_update", "Preview update window", true, None::<&str>)?;
        menu.insert_items(&[&preview, &PredefinedMenuItem::separator(app)?], 7)?;
    }

    let mut tray = TrayIconBuilder::with_id("main").tooltip("Achievement Shelf").menu(&menu);
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    app.manage(TrayItems { on_top, autostart });
    tray.on_menu_event(on_menu_event).build(app)?;
    Ok(())
}

/// Folders to watch for new unlocks, one or more per installed launcher.
fn watch_targets(data_dir: &Path) -> Vec<watch::Target> {
    let mut targets = Vec::new();
    match SteamPaths::locate() {
        Ok(paths) => targets.push(watch::Target {
            dir: paths.stats_dir(),
            relevant: |name| name.starts_with("UserGameStats_"),
            changed: Box::new(|| true),
        }),
        Err(e) => eprintln!("achievement-shelf: {e}"),
    }
    match GogPaths::locate() {
        Ok(gog) => {
            let mut last = gog.fingerprint();
            targets.push(watch::Target {
                dir: gog.storage_dir(),
                // Galaxy writes to the `-wal` file first, then folds it into the database.
                relevant: |name| name.starts_with(gog::DB_FILE_NAME),
                changed: Box::new(move || {
                    let now = gog.fingerprint();
                    now != std::mem::replace(&mut last, now.clone())
                }),
            });
        }
        Err(e) => eprintln!("achievement-shelf: {e}"),
    }
    if let Ok(ubisoft) = UbisoftPaths::locate() {
        // The launcher appends a record to `<product>.spool` the moment an achievement unlocks.
        for dir in ubisoft.spool_dirs() {
            targets.push(watch::Target {
                dir,
                relevant: |name| name.to_ascii_lowercase().ends_with(".spool"),
                changed: Box::new(|| true),
            });
        }
    }
    if let Ok(ea) = EaPaths::locate(data_dir) {
        let mut last = ea.fingerprint();
        let mut tail = ea.tail();
        targets.push(watch::Target {
            dir: ea.logs.clone(),
            relevant: |name| name.starts_with(ea::LOG_PREFIX),
            changed: Box::new(move || {
                if !tail.has_news() {
                    return false;
                }
                let now = ea.fingerprint();
                now != std::mem::replace(&mut last, now)
            }),
        });
    }
    targets
}

/// The app was called Steam Shelf before, with this identifier for its config folder.
const OLD_IDENTIFIER: &str = "dev.steamshelf.app";
const OLD_PRODUCT_NAME: &str = "Steam Shelf";

/// Copies settings from the old config folder on the first run after the rename.
fn migrate_old_settings(settings_path: &Path) {
    if settings_path.exists() {
        return;
    }
    let Some(old) =
        settings_path.parent().and_then(Path::parent).map(|dir| dir.join(OLD_IDENTIFIER).join("settings.json"))
    else {
        return;
    };
    if !old.is_file() {
        return;
    }
    if let Some(parent) = settings_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::copy(&old, settings_path) {
        eprintln!("achievement-shelf: could not copy settings from {}: {e}", old.display());
    }
}

/// Autostart is registered under the product name. Moves an entry left by the old name.
#[cfg(windows)]
fn migrate_old_autostart(app: &AppHandle) {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE};
    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
    let hkcu = winreg::RegKey::predef(HKEY_CURRENT_USER);
    let Ok(run) = hkcu.open_subkey_with_flags(RUN, KEY_QUERY_VALUE | KEY_SET_VALUE) else {
        return;
    };
    if run.get_raw_value(OLD_PRODUCT_NAME).is_err() {
        return;
    }
    let _ = run.delete_value(OLD_PRODUCT_NAME);
    if let Ok(approved) = hkcu.open_subkey_with_flags(APPROVED, KEY_SET_VALUE) {
        let _ = approved.delete_value(OLD_PRODUCT_NAME);
    }
    if let Err(e) = app.autolaunch().enable() {
        eprintln!("achievement-shelf: could not move autostart to the new name: {e}");
    }
}

#[cfg(not(windows))]
fn migrate_old_autostart(_app: &AppHandle) {}

/// Where the update check is up to, sent to the update window as [`UPDATE_EVENT`]. The window
/// starts out showing the check itself.
#[derive(Clone, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
enum UpdateStatus {
    Downloading { version: String, downloaded: u64, total: Option<u64> },
    Installing,
    Failed,
}

/// Opens the small window that shows the update check, and any update downloading and
/// installing, centred on the shelf.
fn open_update_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let window = WebviewWindowBuilder::new(app, UPDATE_LABEL, WebviewUrl::App("updater.html".into()))
        .title("Achievement Shelf")
        .inner_size(360.0, 150.0)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .visible(false)
        .build()?;

    let settings = app.state::<AppState>().settings.lock().unwrap().clone();
    let shelf = app.get_webview_window("main");
    if let Some(monitor) = shelf_monitor(shelf.as_ref().unwrap_or(&window), settings.display.as_deref())? {
        let area = monitor.work_area();
        // Moves onto the display first: crossing to one with another scale resizes the window.
        window.set_position(area.position)?;
        let size = window.outer_size()?;
        // The shelves sit in the work area's bottom-right corner, at the page's zoom (see
        // `place_window` and `apply_zoom`). Worked out here rather than asked of the shelf
        // page, which may not have drawn yet while its window is hidden.
        let px = monitor.scale_factor() * shown_scale(&monitor, &settings) * fit_zoom(&monitor);
        let shelf_width = shelf_width(settings.per_row) * px;
        let shelf_height = settings.total_rows() as f64 * ROW_HEIGHT * px;
        let (right, bottom) = (area.position.x + area.size.width as i32, area.position.y + area.size.height as i32);
        let x = (right as f64 - (shelf_width + size.width as f64) / 2.0).round() as i32;
        let y = (bottom as f64 - (shelf_height + size.height as f64) / 2.0).round() as i32;
        // Kept on screen, clear of the taskbar, even over a short shelf.
        window.set_position(PhysicalPosition::new(
            x.clamp(area.position.x, (right - size.width as i32).max(area.position.x)),
            y.clamp(area.position.y, (bottom - size.height as i32).max(area.position.y)),
        ))?;
    }
    window.show()?;
    Ok(window)
}

/// Waits without holding up the async runtime.
async fn pause(duration: Duration) {
    let _ = tauri::async_runtime::spawn_blocking(move || std::thread::sleep(duration)).await;
}

/// Dev builds only: opens the update window where a real check would, and plays it through
/// the check, a made-up download, the installing step, and the failure message. Nothing is
/// downloaded.
fn preview_update_window(app: AppHandle) {
    if app.get_webview_window(UPDATE_LABEL).is_some() {
        return;
    }
    let window = match open_update_window(&app) {
        Ok(window) => window,
        Err(e) => return eprintln!("achievement-shelf: could not open the update window: {e}"),
    };
    std::thread::spawn(move || {
        let send = |status: UpdateStatus| {
            let _ = app.emit_to(UPDATE_LABEL, UPDATE_EVENT, status);
        };
        let pause = |ms| std::thread::sleep(Duration::from_millis(ms));
        pause(2000);
        let total = 8_800_000u64;
        for step in 0..=50 {
            let downloaded = total * step / 50;
            send(UpdateStatus::Downloading { version: "0.0.0-preview".into(), downloaded, total: Some(total) });
            pause(80);
        }
        send(UpdateStatus::Installing);
        pause(2500);
        send(UpdateStatus::Failed);
        pause(5000);
        let _ = window.close();
    });
}

/// Checks GitHub Releases for a newer signed build and installs it, with the update window
/// showing the check and the update's progress. The installer runs silently, closes the app,
/// and starts it again. With no update, or if anything fails, the shelf appears instead.
fn spawn_update_check(app: AppHandle) {
    use tauri_plugin_updater::UpdaterExt;
    tauri::async_runtime::spawn(async move {
        let opened = Instant::now();
        let window = open_update_window(&app)
            .map_err(|e| eprintln!("achievement-shelf: could not open the update window: {e}"))
            .ok();
        // Closes the update window and shows the shelf in its place.
        let finish = |window: Option<WebviewWindow>| {
            if let Some(window) = window {
                let _ = window.close();
            }
            if let Some(shelf) = app.get_webview_window("main") {
                if let Err(e) = shelf.show() {
                    eprintln!("achievement-shelf: could not show the shelf: {e}");
                }
            }
        };

        // The shelf waits for this check, so it gets a short limit.
        let checked = match app.updater_builder().timeout(UPDATE_CHECK_TIMEOUT).build() {
            Ok(updater) => updater.check().await,
            Err(e) => Err(e),
        };
        let mut update = match checked {
            Ok(Some(update)) => update,
            result => {
                if let Err(e) = result {
                    eprintln!("achievement-shelf: update check failed: {e}");
                }
                // A quick check would only flash the window; it stays up long enough to read.
                if let Some(rest) = UPDATE_CHECK_MIN_SHOWN.checked_sub(opened.elapsed()) {
                    pause(rest).await;
                }
                return finish(window);
            }
        };
        // The limit covers the download too, which can take longer on a slow connection.
        update.timeout = None;
        eprintln!("achievement-shelf: installing update {}", update.version);
        let send = |status: UpdateStatus| {
            let _ = app.emit_to(UPDATE_LABEL, UPDATE_EVENT, status);
        };

        // Chunks arrive many times a second; only send whole-percent steps.
        let version = update.version.clone();
        let mut downloaded = 0u64;
        let mut last_step = None;
        let result = update
            .download(
                |chunk, total| {
                    downloaded += chunk as u64;
                    let step = total.map(|t| downloaded * 100 / t.max(1)).unwrap_or(downloaded >> 18);
                    if last_step != Some(step) {
                        last_step = Some(step);
                        send(UpdateStatus::Downloading { version: version.clone(), downloaded, total });
                    }
                },
                || send(UpdateStatus::Installing),
            )
            .await;
        let result = match result {
            Ok(bytes) => {
                // Lets the window show "Installing" before the installer closes the app.
                pause(Duration::from_millis(900)).await;
                update.install(bytes)
            }
            Err(e) => Err(e),
        };
        if let Err(e) = result {
            eprintln!("achievement-shelf: update failed: {e}");
            send(UpdateStatus::Failed);
            pause(Duration::from_secs(5)).await;
            return finish(window);
        }
        // Windows exits inside `install`; elsewhere the new version is in place now.
        app.restart();
    });
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let cache_dir = app.path().app_cache_dir()?;
            let data_dir = app.path().app_data_dir()?;
            let settings_path = app.path().app_config_dir()?.join("settings.json");
            migrate_old_settings(&settings_path);
            let settings = Settings::load(&settings_path);
            let names: HashMap<String, String> = std::fs::read_to_string(cache_dir.join("names.json"))
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default();
            let name_misses: HashMap<u32, u64> = std::fs::read_to_string(cache_dir.join("name-misses.json"))
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default();
            let rarity = RarityCache::load(cache_dir.join("rarity.json"));
            let client = reqwest::Client::builder()
                .user_agent("achievement-shelf/0.1")
                .timeout(Duration::from_secs(10))
                .build()?;
            let handle = app.handle().clone();
            let retro = retro::spawn(client.clone(), data_dir.clone(), move || {
                let _ = handle.emit(watch::UPDATED_EVENT, ());
            });
            let handle = app.handle().clone();
            let xbox = xbox::spawn(client.clone(), data_dir.clone(), move || {
                let _ = handle.emit(watch::UPDATED_EVENT, ());
            });
            let hit_rect = Arc::new(Mutex::new(None));

            // Managed before the window is shown so early front-end calls find the state.
            app.manage(AppState {
                client,
                cache_dir,
                data_dir: data_dir.clone(),
                settings_path,
                settings: Mutex::new(settings.clone()),
                names: Mutex::new(names),
                name_misses: Mutex::new(name_misses),
                rarity,
                hit_rect: hit_rect.clone(),
                shares: Mutex::default(),
                retro,
                xbox,
            });
            apply_theme(app.handle(), settings.theme);
            let window = app.get_webview_window("main").expect("main window is defined in tauri.conf.json");
            place_window(&window, &settings)?;
            apply_layer(&window, settings.on_top)?;
            window.set_ignore_cursor_events(true)?;
            // Release builds show the shelf once the update check is done (`spawn_update_check`),
            // so it doesn't appear only to vanish when an update installs.
            if cfg!(debug_assertions) {
                window.show()?;
            }
            spawn_hit_test(window, hit_rect.clone());
            // Images shared in an earlier run, once nothing can still be reading them.
            std::thread::spawn(clean_old_shares);

            migrate_old_autostart(app.handle());
            build_tray(app.handle(), &settings)?;
            watch::spawn(app.handle().clone(), watch_targets(&data_dir));
            if !cfg!(debug_assertions) {
                spawn_update_check(app.handle().clone());
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_shelf,
            get_unlocked,
            get_icon,
            get_banner,
            set_trophy,
            move_trophies,
            clear_trophies,
            set_hit_rect,
            prepare_share,
            forget_shares,
            share_drag,
            get_settings,
            change_shelf_rows,
            change_shelf_per_row,
            set_shelf_display,
            set_shelf_scale,
            reset_shelf_scale,
            set_keep_on_top,
            set_app_theme,
            reset_settings,
            set_start_with_windows,
            retro_account,
            retro_connect,
            retro_open_key_page,
            retro_disconnect,
            xbox_account,
            xbox_connect,
            xbox_cancel,
            xbox_disconnect
        ])
        .run(tauri::generate_context!())
        .expect("error while running Achievement Shelf");
}
