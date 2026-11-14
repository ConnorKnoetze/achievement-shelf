pub mod steam;

mod icons;
mod settings;
mod watch;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

use settings::{Settings, TrophyKey, MAX_TROPHIES, PER_ROW};
use steam::{Achievement, SteamPaths};

// Window layout in logical pixels; must match the sizes in src/shelf.css.
const SHELF_WIDTH: f64 = 520.0;
const ROW_HEIGHT: f64 = 112.0;
/// Empty space above the shelf where hover cards are drawn.
const TOOLTIP_SPACE: f64 = 150.0;
const PICKER_LABEL: &str = "picker";

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
    /// Game names known so far (from appmanifests, the store API, or the disk cache).
    names: Mutex<HashMap<u32, String>>,
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
    rows: usize,
    per_row: usize,
    /// Whether the trophy row is shown (it may still be empty).
    trophy_shelf: bool,
    trophies: Vec<ShelfItem>,
    items: Vec<ShelfItem>,
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
    max_trophies: usize,
    trophies: Vec<TrophyKey>,
    items: Vec<PickerItem>,
}

fn is_key(achievement: &Achievement, key: &TrophyKey) -> bool {
    achievement.app_id == key.app_id && achievement.api_name == key.api_name
}

/// All unlocked achievements (newest first) plus the names of installed games.
async fn load_unlocked() -> Result<(Vec<Achievement>, HashMap<u32, String>), String> {
    tauri::async_runtime::spawn_blocking(|| {
        let paths = SteamPaths::locate()?;
        Ok((steam::stats::load_all(&paths), steam::names::installed_names(&paths)))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Makes sure every game in `achievements` has a name, asking the store only for games
/// we have never seen, and saves new names to the disk cache.
async fn resolve_names(state: &AppState, installed: HashMap<u32, String>, achievements: &[&Achievement]) {
    state.names.lock().unwrap().extend(installed);
    let mut fetched_any = false;
    let mut app_ids: Vec<u32> = achievements.iter().map(|a| a.app_id).collect();
    app_ids.sort_unstable();
    app_ids.dedup();
    for app_id in app_ids {
        if state.names.lock().unwrap().contains_key(&app_id) {
            continue;
        }
        if let Some(name) = steam::names::fetch_store_name(&state.client, app_id).await {
            state.names.lock().unwrap().insert(app_id, name);
            fetched_any = true;
        }
    }
    if fetched_any {
        let json = serde_json::to_string(&*state.names.lock().unwrap()).unwrap_or_default();
        let _ = std::fs::create_dir_all(&state.cache_dir);
        let _ = std::fs::write(state.names_path(), json);
    }
}

fn game_name(state: &AppState, app_id: u32) -> String {
    state.names.lock().unwrap().get(&app_id).cloned().unwrap_or_else(|| format!("App {app_id}"))
}

async fn shelf_items(state: &AppState, achievements: Vec<Achievement>) -> Vec<ShelfItem> {
    let mut items = Vec::with_capacity(achievements.len());
    for achievement in achievements {
        let icon_url =
            icons::icon_data_url(&state.client, &state.cache_dir, achievement.app_id, &achievement.icon).await;
        let game_name = game_name(state, achievement.app_id);
        items.push(ShelfItem { achievement, game_name, icon_url });
    }
    items
}

#[tauri::command]
async fn get_shelf(state: State<'_, AppState>) -> Result<Shelf, String> {
    let (rows, trophy_shelf, trophy_keys) = {
        let settings = state.settings.lock().unwrap();
        (settings.rows, settings.trophy_shelf, settings.trophies.clone())
    };

    let (mut recent, installed) = load_unlocked().await?;
    // Trophies keep the order the user chose. Ones no longer found on disk are skipped.
    let trophies: Vec<Achievement> = if trophy_shelf {
        trophy_keys.iter().filter_map(|key| recent.iter().find(|a| is_key(a, key)).cloned()).collect()
    } else {
        Vec::new()
    };
    recent.truncate(rows * PER_ROW);

    resolve_names(&state, installed, &recent.iter().chain(&trophies).collect::<Vec<_>>()).await;
    Ok(Shelf {
        rows,
        per_row: PER_ROW,
        trophy_shelf,
        trophies: shelf_items(&state, trophies).await,
        items: shelf_items(&state, recent).await,
    })
}

/// Every unlocked achievement, newest first, for the trophy picker.
#[tauri::command]
async fn get_unlocked(state: State<'_, AppState>) -> Result<PickerData, String> {
    let (all, installed) = load_unlocked().await?;
    resolve_names(&state, installed, &all.iter().collect::<Vec<_>>()).await;
    let items = all
        .into_iter()
        .map(|achievement| PickerItem { game_name: game_name(&state, achievement.app_id), achievement })
        .collect();
    let trophies = state.settings.lock().unwrap().trophies.clone();
    Ok(PickerData { max_trophies: MAX_TROPHIES, trophies, items })
}

#[tauri::command]
async fn get_icon(state: State<'_, AppState>, app_id: u32, icon: String) -> Result<Option<String>, String> {
    Ok(icons::icon_data_url(&state.client, &state.cache_dir, app_id, &icon).await)
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
            if settings.trophies.len() >= MAX_TROPHIES {
                return Err(format!("The trophy shelf holds {MAX_TROPHIES} achievements. Remove one first."));
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
    let height = ((TOOLTIP_SPACE + rows as f64 * ROW_HEIGHT) * scale).round() as u32;
    let area = monitor.work_area();
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

/// Opens the trophy picker, or brings it forward if it is already open.
fn open_picker(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(PICKER_LABEL) {
        window.unminimize()?;
        return window.set_focus();
    }
    WebviewWindowBuilder::new(app, PICKER_LABEL, WebviewUrl::App("picker.html".into()))
        .title("Choose trophies")
        .inner_size(860.0, 640.0)
        .min_inner_size(520.0, 360.0)
        .center()
        .build()?;
    Ok(())
}

struct TrayItems {
    two_rows: CheckMenuItem<tauri::Wry>,
    trophy_shelf: CheckMenuItem<tauri::Wry>,
    on_top: CheckMenuItem<tauri::Wry>,
    autostart: CheckMenuItem<tauri::Wry>,
}

fn on_menu_event(app: &AppHandle, event: MenuEvent, tray: &TrayItems) {
    let state = app.state::<AppState>();
    let window = app.get_webview_window("main");
    match event.id().as_ref() {
        "refresh" => {
            let _ = app.emit(watch::UPDATED_EVENT, ());
        }
        id @ ("two_rows" | "trophy_shelf") => {
            let mut settings = state.settings.lock().unwrap();
            if id == "two_rows" {
                settings.rows = if tray.two_rows.is_checked().unwrap_or(false) { 2 } else { 1 };
            } else {
                settings.trophy_shelf = tray.trophy_shelf.is_checked().unwrap_or(false);
            }
            settings.save(&state.settings_path);
            if let Some(window) = &window {
                let _ = place_window(window, settings.total_rows());
            }
            let _ = app.emit(watch::UPDATED_EVENT, ());
        }
        "choose_trophies" => {
            if let Err(e) = open_picker(app) {
                eprintln!("steam-shelf: could not open the trophy picker: {e}");
            }
        }
        "on_top" => {
            let value = tray.on_top.is_checked().unwrap_or(false);
            let mut settings = state.settings.lock().unwrap();
            settings.on_top = value;
            settings.save(&state.settings_path);
            if let Some(window) = &window {
                let _ = apply_layer(window, value);
            }
        }
        "autostart" => {
            let manager = app.autolaunch();
            let result = if tray.autostart.is_checked().unwrap_or(false) { manager.enable() } else { manager.disable() };
            if let Err(e) = result {
                eprintln!("steam-shelf: could not change autostart: {e}");
                let _ = tray.autostart.set_checked(manager.is_enabled().unwrap_or(false));
            }
        }
        "quit" => app.exit(0),
        _ => {}
    }
}

fn build_tray(app: &AppHandle, settings: &Settings) -> tauri::Result<()> {
    let refresh = MenuItem::with_id(app, "refresh", "Refresh", true, None::<&str>)?;
    let two_rows = CheckMenuItem::with_id(app, "two_rows", "Two rows", true, settings.rows == 2, None::<&str>)?;
    let trophy_shelf =
        CheckMenuItem::with_id(app, "trophy_shelf", "Trophy shelf", true, settings.trophy_shelf, None::<&str>)?;
    let choose_trophies = MenuItem::with_id(app, "choose_trophies", "Choose trophies...", true, None::<&str>)?;
    let on_top = CheckMenuItem::with_id(app, "on_top", "Keep above other windows", true, settings.on_top, None::<&str>)?;
    let autostart_enabled = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart = CheckMenuItem::with_id(app, "autostart", "Start with Windows", true, autostart_enabled, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &refresh,
            &PredefinedMenuItem::separator(app)?,
            &two_rows,
            &trophy_shelf,
            &choose_trophies,
            &on_top,
            &autostart,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    let mut tray = TrayIconBuilder::with_id("main").tooltip("Steam Shelf").menu(&menu);
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    let items = TrayItems { two_rows, trophy_shelf, on_top, autostart };
    tray.on_menu_event(move |app, event| on_menu_event(app, event, &items))
        .build(app)?;
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .setup(|app| {
            let cache_dir = app.path().app_cache_dir()?;
            let settings_path = app.path().app_config_dir()?.join("settings.json");
            let settings = Settings::load(&settings_path);
            let names: HashMap<u32, String> = std::fs::read_to_string(cache_dir.join("names.json"))
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default();
            let hit_rect = Arc::new(Mutex::new(None));

            // Managed before the window is shown so early front-end calls find the state.
            app.manage(AppState {
                client: reqwest::Client::builder()
                    .user_agent("steam-shelf/0.1")
                    .timeout(Duration::from_secs(10))
                    .build()?,
                cache_dir,
                settings_path,
                settings: Mutex::new(settings.clone()),
                names: Mutex::new(names),
                hit_rect: hit_rect.clone(),
            });
            let window = app.get_webview_window("main").expect("main window is defined in tauri.conf.json");
            place_window(&window, settings.total_rows())?;
            apply_layer(&window, settings.on_top)?;
            window.set_ignore_cursor_events(true)?;
            window.show()?;
            spawn_hit_test(window, hit_rect.clone());

            build_tray(app.handle(), &settings)?;
            match SteamPaths::locate() {
                Ok(paths) => watch::spawn(app.handle().clone(), paths.stats_dir()),
                Err(e) => eprintln!("steam-shelf: {e}"),
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_shelf,
            get_unlocked,
            get_icon,
            set_trophy,
            clear_trophies,
            set_hit_rect
        ])
        .run(tauri::generate_context!())
        .expect("error while running Steam Shelf");
}
