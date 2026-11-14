//! Watches Steam's stats folder and tells the front end when achievements change.

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use notify::{RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter};

pub const UPDATED_EVENT: &str = "achievements-updated";

fn touches_user_stats(event: &notify::Event) -> bool {
    event.paths.iter().any(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("UserGameStats_"))
    })
}

pub fn spawn(app: AppHandle, stats_dir: PathBuf) {
    std::thread::spawn(move || {
        let (tx, rx) = mpsc::channel();
        let mut watcher = match notify::recommended_watcher(tx) {
            Ok(w) => w,
            Err(e) => return eprintln!("steam-shelf: file watcher unavailable: {e}"),
        };
        if let Err(e) = watcher.watch(&stats_dir, RecursiveMode::NonRecursive) {
            return eprintln!("steam-shelf: cannot watch {}: {e}", stats_dir.display());
        }
        while let Ok(first) = rx.recv() {
            let mut changed = matches!(&first, Ok(event) if touches_user_stats(event));
            // Steam writes several files in a burst; wait until it goes quiet.
            while let Ok(next) = rx.recv_timeout(Duration::from_millis(800)) {
                changed |= matches!(&next, Ok(event) if touches_user_stats(event));
            }
            if changed {
                let _ = app.emit(UPDATED_EVENT, ());
            }
        }
    });
}
