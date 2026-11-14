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
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State, WebviewWindow};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

use settings::{Settings, PER_ROW};
use steam::{Achievement, SteamPaths};

// Window layout in logical pixels; must match the sizes in src/shelf.css.
const SHELF_WIDTH: f64 = 520.0;
const ROW_HEIGHT: f64 = 112.0;
/// Empty space above the shelf where hover cards are drawn.
const TOOLTIP_SPACE: f64 = 150.0;

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
    items: Vec<ShelfItem>,
}

#[tauri::command]
async fn get_shelf(state: State<'_, AppState>) -> Result<Shelf, String> {
    let rows = state.settings.lock().unwrap().rows;
    let limit = rows * PER_ROW;

    let (achievements, installed) = tauri::async_runtime::spawn_blocking(move || {
        let paths = SteamPaths::locate()?;
        let mut all = steam::stats::load_all(&paths);
        all.truncate(limit);
        Ok::<_, String>((all, steam::names::installed_names(&paths)))
    })
    .await
    .map_err(|e| e.to_string())??;

    // Resolve game names, asking the store only for games we have never seen.
    state.names.lock().unwrap().extend(installed);
    let mut fetched_any = false;
    let mut app_ids: Vec<u32> = achievements.iter().map(|a| a.app_id).collect();
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

    let mut items = Vec::with_capacity(achievements.len());
    for achievement in achievements {
        let icon_url =
            icons::icon_data_url(&state.client, &state.cache_dir, achievement.app_id, &achievement.icon).await;
        let game_name = state
            .names
            .lock()
            .unwrap()
            .get(&achievement.app_id)
            .cloned()
            .unwrap_or_else(|| format!("App {}", achievement.app_id));
        items.push(ShelfItem { achievement, game_name, icon_url });
    }
    Ok(Shelf { rows, per_row: PER_ROW, items })
}

#[tauri::command]
fn set_hit_rect(state: State<'_, AppState>, rect: HitRect) {
    *state.hit_rect.lock().unwrap() = Some(rect);
}

/// Sizes the window for the row count and puts it in the bottom-right corner, above the taskbar.
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

fn on_menu_event(app: &AppHandle, event: MenuEvent, two_rows: &CheckMenuItem<tauri::Wry>, on_top: &CheckMenuItem<tauri::Wry>, autostart: &CheckMenuItem<tauri::Wry>) {
    let state = app.state::<AppState>();
    let window = app.get_webview_window("main");
    match event.id().as_ref() {
        "refresh" => {
            let _ = app.emit(watch::UPDATED_EVENT, ());
        }
        "two_rows" => {
            let rows = if two_rows.is_checked().unwrap_or(false) { 2 } else { 1 };
            let mut settings = state.settings.lock().unwrap();
            settings.rows = rows;
            settings.save(&state.settings_path);
            if let Some(window) = &window {
                let _ = place_window(window, rows);
            }
            let _ = app.emit(watch::UPDATED_EVENT, ());
        }
        "on_top" => {
            let value = on_top.is_checked().unwrap_or(false);
            let mut settings = state.settings.lock().unwrap();
            settings.on_top = value;
            settings.save(&state.settings_path);
            if let Some(window) = &window {
                let _ = apply_layer(window, value);
            }
        }
        "autostart" => {
            let manager = app.autolaunch();
            let result = if autostart.is_checked().unwrap_or(false) { manager.enable() } else { manager.disable() };
            if let Err(e) = result {
                eprintln!("steam-shelf: could not change autostart: {e}");
                let _ = autostart.set_checked(manager.is_enabled().unwrap_or(false));
            }
        }
        "quit" => app.exit(0),
        _ => {}
    }
}

fn build_tray(app: &AppHandle, settings: &Settings) -> tauri::Result<()> {
    let refresh = MenuItem::with_id(app, "refresh", "Refresh", true, None::<&str>)?;
    let two_rows = CheckMenuItem::with_id(app, "two_rows", "Two rows", true, settings.rows == 2, None::<&str>)?;
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
    tray.on_menu_event(move |app, event| on_menu_event(app, event, &two_rows, &on_top, &autostart))
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
            place_window(&window, settings.rows)?;
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
        .invoke_handler(tauri::generate_handler![get_shelf, set_hit_rect])
        .run(tauri::generate_context!())
        .expect("error while running Steam Shelf");
}
