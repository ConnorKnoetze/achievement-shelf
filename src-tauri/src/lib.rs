pub mod achievement;
pub mod gog;
pub mod steam;

mod icons;
mod rarity;
mod settings;
mod watch;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

use achievement::{game_key, Achievement, Source};
use gog::GogPaths;
use rarity::RarityCache;
use settings::{Settings, TrophyKey, MAX_LEVELS, PER_ROW};
use steam::SteamPaths;

// Window layout in logical pixels; must match the sizes in src/shelf.css.
const SHELF_WIDTH: f64 = 520.0;
const ROW_HEIGHT: f64 = 112.0;
/// Empty space above the shelf where hover cards (game banner included) are drawn.
const TOOLTIP_SPACE: f64 = 280.0;
const PICKER_LABEL: &str = "picker";
const SETTINGS_LABEL: &str = "settings";
/// Sent with a fresh [`SettingsView`] whenever a setting changes, from the tray or the settings window.
const SETTINGS_EVENT: &str = "settings-changed";

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
    settings_path: PathBuf,
    settings: Mutex<Settings>,
    /// Game names known so far, keyed by [`game_key`]: from Steam appmanifests, the GOG
    /// database, the Steam store API, or the disk cache.
    names: Mutex<HashMap<String, String>>,
    /// Steam rarities fetched from the Web API.
    rarity: RarityCache,
    /// Shelf area in the window, in logical pixels. Outside it, clicks pass through.
    hit_rect: Arc<Mutex<Option<HitRect>>>,
}

impl AppState {
    fn names_path(&self) -> PathBuf {
        self.cache_dir.join("names.json")
    }
}

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
    trophies: Vec<ShelfItem>,
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
    max_trophies: usize,
    trophies: Vec<TrophyKey>,
    items: Vec<PickerItem>,
}

struct Unlocked {
    /// Every launcher's unlocked achievements, newest first.
    achievements: Vec<Achievement>,
    /// Game names the launchers know locally, keyed by [`game_key`].
    names: HashMap<String, String>,
    /// GOG store banner URL templates, keyed by [`game_key`].
    gog_banners: HashMap<String, String>,
    launchers: Vec<Source>,
}

/// Reads every launcher that is installed. Fails only when none is.
async fn load_unlocked() -> Result<Unlocked, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let mut unlocked = Unlocked {
            achievements: Vec::new(),
            names: HashMap::new(),
            gog_banners: HashMap::new(),
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
                Ok(banners) => unlocked.gog_banners = banners,
                Err(e) => eprintln!("achievement-shelf: cannot read GOG game banners: {e}"),
            }
        }
        if unlocked.launchers.is_empty() {
            return Err("No Steam or GOG Galaxy installation found".to_string());
        }
        unlocked.achievements.sort_by(|a, b| b.unlocked_at.cmp(&a.unlocked_at));
        Ok(unlocked)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Makes sure every game in `achievements` has a name, asking the Steam store only for
/// Steam games we have never seen, and saves new names to the disk cache. GOG names all
/// come from its local database.
async fn resolve_names(state: &AppState, local: HashMap<String, String>, achievements: &[&Achievement]) {
    state.names.lock().unwrap().extend(local);
    let mut fetched_any = false;
    let mut app_ids: Vec<u32> = achievements
        .iter()
        .filter(|a| a.source == Source::Steam)
        .filter_map(|a| a.game_id.parse().ok())
        .collect();
    app_ids.sort_unstable();
    app_ids.dedup();
    for app_id in app_ids {
        let key = game_key(Source::Steam, &app_id.to_string());
        if state.names.lock().unwrap().contains_key(&key) {
            continue;
        }
        if let Some(name) = steam::names::fetch_store_name(&state.client, app_id).await {
            state.names.lock().unwrap().insert(key, name);
            fetched_any = true;
        }
    }
    if fetched_any {
        let json = serde_json::to_string(&*state.names.lock().unwrap()).unwrap_or_default();
        let _ = std::fs::create_dir_all(&state.cache_dir);
        let _ = std::fs::write(state.names_path(), json);
    }
}

fn game_name(state: &AppState, achievement: &Achievement) -> String {
    let known = state.names.lock().unwrap().get(&achievement.game_key()).cloned();
    known.unwrap_or_else(|| match achievement.source {
        Source::Steam => format!("App {}", achievement.game_id),
        Source::Gog => format!("GOG game {}", achievement.game_id),
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
    gog_banners: &HashMap<String, String>,
    achievements: &[&Achievement],
) -> HashMap<String, String> {
    let mut banners = HashMap::new();
    for achievement in achievements {
        let key = achievement.game_key();
        if banners.contains_key(&key) {
            continue;
        }
        let gog_cover = gog_banners.get(&key).map(String::as_str);
        if let Some(url) =
            icons::banner_data_url(&state.client, &state.cache_dir, achievement.source, &achievement.game_id, gog_cover)
                .await
        {
            banners.insert(key, url);
        }
    }
    banners
}

#[tauri::command]
async fn get_shelf(state: State<'_, AppState>) -> Result<Shelf, String> {
    let (rows, trophy_rows, trophy_capacity, trophy_keys) = {
        let settings = state.settings.lock().unwrap();
        (settings.rows, settings.trophy_rows, settings.trophy_capacity(), settings.trophies.clone())
    };

    let Unlocked { achievements: mut recent, names, gog_banners, .. } = load_unlocked().await?;
    // Trophies keep the order the user chose. Ones no longer found on disk are skipped,
    // and ones past what the trophy rows hold are not shown.
    let mut trophies: Vec<Achievement> = trophy_keys
        .iter()
        .filter_map(|key| recent.iter().find(|a| key.matches(a)).cloned())
        .take(trophy_capacity)
        .collect();
    recent.truncate(rows * PER_ROW);

    let shown: Vec<&Achievement> = recent.iter().chain(&trophies).collect();
    resolve_names(&state, names, &shown).await;
    let banners = banners(&state, &gog_banners, &shown).await;
    state.rarity.fill(&state.client, &mut recent).await;
    state.rarity.fill(&state.client, &mut trophies).await;
    Ok(Shelf {
        rows,
        trophy_rows,
        per_row: PER_ROW,
        trophies: shelf_items(&state, trophies).await,
        items: shelf_items(&state, recent).await,
        banners,
    })
}

/// Every unlocked achievement, newest first, for the trophy picker.
#[tauri::command]
async fn get_unlocked(state: State<'_, AppState>) -> Result<PickerData, String> {
    let Unlocked { mut achievements, names, launchers, .. } = load_unlocked().await?;
    resolve_names(&state, names, &achievements.iter().collect::<Vec<_>>()).await;
    state.rarity.fill(&state.client, &mut achievements).await;
    let items = achievements
        .into_iter()
        .map(|achievement| PickerItem { game_name: game_name(&state, &achievement), achievement })
        .collect();
    let (max_trophies, trophies) = {
        let settings = state.settings.lock().unwrap();
        (settings.trophy_capacity(), settings.trophies.clone())
    };
    Ok(PickerData { launchers, max_trophies, trophies, items })
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

/// Adds or removes a trophy and returns the new list. Fails when the shelf is already full.
#[tauri::command]
fn set_trophy(
    app: AppHandle,
    state: State<'_, AppState>,
    key: TrophyKey,
    trophy: bool,
) -> Result<Vec<TrophyKey>, String> {
    let trophies = {
        let mut settings = state.settings.lock().unwrap();
        let present = settings.trophies.contains(&key);
        if trophy && !present {
            let capacity = settings.trophy_capacity();
            if settings.trophies.len() >= capacity {
                return Err(format!(
                    "The trophy shelf holds {capacity} achievements. Remove one, or add a trophy level from the tray menu."
                ));
            }
            settings.trophies.push(key);
        } else if !trophy && present {
            settings.trophies.retain(|k| *k != key);
        }
        settings.save(&state.settings_path);
        settings.trophies.clone()
    };
    let _ = app.emit(watch::UPDATED_EVENT, ());
    Ok(trophies)
}

#[tauri::command]
fn clear_trophies(app: AppHandle, state: State<'_, AppState>) {
    {
        let mut settings = state.settings.lock().unwrap();
        settings.trophies.clear();
        settings.save(&state.settings_path);
    }
    let _ = app.emit(watch::UPDATED_EVENT, ());
}

#[tauri::command]
fn set_hit_rect(state: State<'_, AppState>, rect: HitRect) {
    *state.hit_rect.lock().unwrap() = Some(rect);
}

/// Sizes the window for the row count (trophy row included) and puts it in the
/// bottom-right corner, above the taskbar.
fn place_window(window: &WebviewWindow, rows: usize) -> tauri::Result<()> {
    let monitor = match window.primary_monitor()? {
        Some(m) => m,
        None => match window.current_monitor()? {
            Some(m) => m,
            None => return Ok(()),
        },
    };
    let scale = monitor.scale_factor();
    let width = (SHELF_WIDTH * scale).round() as u32;
    let area = monitor.work_area();
    // Ten levels can be taller than the screen; the shelf is drawn from the bottom up, so the
    // top of it (the tooltip space first) is cut off rather than the window leaving the screen.
    let height = (((TOOLTIP_SPACE + rows as f64 * ROW_HEIGHT) * scale).round() as u32).min(area.size.height);
    window.set_size(PhysicalSize::new(width, height))?;
    window.set_position(PhysicalPosition::new(
        area.position.x + area.size.width as i32 - width as i32,
        area.position.y + area.size.height as i32 - height as i32,
    ))
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
            let (Ok(cursor), Ok(origin), Ok(scale)) =
                (window.cursor_position(), window.inner_position(), window.scale_factor())
            else {
                continue;
            };
            let x = (cursor.x - origin.x as f64) / scale;
            let y = (cursor.y - origin.y as f64) / scale;
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
    open_window(app, SETTINGS_LABEL, "settings.html", "Achievement Shelf settings", (420.0, 450.0), (380.0, 420.0))
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
    can_add_row: bool,
    can_remove_recent_row: bool,
    can_remove_trophy_row: bool,
    on_top: bool,
    autostart: bool,
}

fn settings_view(app: &AppHandle) -> SettingsView {
    let settings = app.state::<AppState>().settings.lock().unwrap().clone();
    SettingsView {
        rows: settings.rows,
        trophy_rows: settings.trophy_rows,
        max_levels: MAX_LEVELS,
        can_add_row: settings.can_add_row(),
        can_remove_recent_row: settings.can_remove_recent_row(),
        can_remove_trophy_row: settings.can_remove_trophy_row(),
        on_top: settings.on_top,
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
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

/// Adds or removes a recent or trophy row, then resizes the shelf to fit.
fn change_rows(app: &AppHandle, trophy: bool, add: bool) {
    let state = app.state::<AppState>();
    {
        let mut settings = state.settings.lock().unwrap();
        if !settings.change_rows(trophy, if add { 1 } else { -1 }) {
            return;
        }
        settings.save(&state.settings_path);
        if let Some(window) = app.get_webview_window("main") {
            let _ = place_window(&window, settings.total_rows());
        }
    }
    // Redraws the shelf and updates the picker's trophy capacity.
    let _ = app.emit(watch::UPDATED_EVENT, ());
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
fn set_keep_on_top(app: AppHandle, value: bool) -> SettingsView {
    set_on_top(&app, value);
    settings_view(&app)
}

#[tauri::command]
fn set_start_with_windows(app: AppHandle, value: bool) -> Result<SettingsView, String> {
    set_autostart(&app, value)?;
    Ok(settings_view(&app))
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        "refresh" => {
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

    let mut tray = TrayIconBuilder::with_id("main").tooltip("Achievement Shelf").menu(&menu);
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    app.manage(TrayItems { on_top, autostart });
    tray.on_menu_event(on_menu_event).build(app)?;
    Ok(())
}

/// Folders to watch for new unlocks, one per installed launcher.
fn watch_targets() -> Vec<watch::Target> {
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

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .setup(|app| {
            let cache_dir = app.path().app_cache_dir()?;
            let settings_path = app.path().app_config_dir()?.join("settings.json");
            migrate_old_settings(&settings_path);
            let settings = Settings::load(&settings_path);
            let names: HashMap<String, String> = std::fs::read_to_string(cache_dir.join("names.json"))
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default();
            let rarity = RarityCache::load(cache_dir.join("rarity.json"));
            let hit_rect = Arc::new(Mutex::new(None));

            // Managed before the window is shown so early front-end calls find the state.
            app.manage(AppState {
                client: reqwest::Client::builder()
                    .user_agent("achievement-shelf/0.1")
                    .timeout(Duration::from_secs(10))
                    .build()?,
                cache_dir,
                settings_path,
                settings: Mutex::new(settings.clone()),
                names: Mutex::new(names),
                rarity,
                hit_rect: hit_rect.clone(),
            });
            let window = app.get_webview_window("main").expect("main window is defined in tauri.conf.json");
            place_window(&window, settings.total_rows())?;
            apply_layer(&window, settings.on_top)?;
            window.set_ignore_cursor_events(true)?;
            window.show()?;
            spawn_hit_test(window, hit_rect.clone());

            migrate_old_autostart(app.handle());
            build_tray(app.handle(), &settings)?;
            watch::spawn(app.handle().clone(), watch_targets());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_shelf,
            get_unlocked,
            get_icon,
            set_trophy,
            clear_trophies,
            set_hit_rect,
            get_settings,
            change_shelf_rows,
            set_keep_on_top,
            set_start_with_windows
        ])
        .run(tauri::generate_context!())
        .expect("error while running Achievement Shelf");
}
