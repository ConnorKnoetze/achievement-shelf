//! Watches the launchers' data folders and tells the front end when achievements change.

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use notify::{RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter};

pub const UPDATED_EVENT: &str = "achievements-updated";

/// One folder to watch.
pub struct Target {
    pub dir: PathBuf,
    /// Whether a file in `dir` with this name can hold achievement data.
    pub relevant: fn(&str) -> bool,
    /// Called after a burst of relevant writes; returns whether achievements really changed.
    pub changed: Box<dyn FnMut() -> bool + Send>,
}

/// Flags each target that `event` touched.
fn mark(targets: &[Target], touched: &mut [bool], event: &notify::Result<notify::Event>) {
    let Ok(event) = event else { return };
    for path in &event.paths {
        let (Some(dir), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str())) else {
            continue;
        };
        for (target, touched) in targets.iter().zip(touched.iter_mut()) {
            if dir == target.dir && (target.relevant)(name) {
                *touched = true;
            }
        }
    }
}

pub fn spawn(app: AppHandle, mut targets: Vec<Target>) {
    if targets.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        let (tx, rx) = mpsc::channel();
        let mut watcher = match notify::recommended_watcher(tx) {
            Ok(w) => w,
            Err(e) => return eprintln!("achievement-shelf: file watcher unavailable: {e}"),
        };
        for target in &targets {
            if let Err(e) = watcher.watch(&target.dir, RecursiveMode::NonRecursive) {
                eprintln!("achievement-shelf: cannot watch {}: {e}", target.dir.display());
            }
        }
        let mut touched = vec![false; targets.len()];
        while let Ok(first) = rx.recv() {
            mark(&targets, &mut touched, &first);
            // Launchers write several files in a burst; wait until they go quiet.
            while let Ok(next) = rx.recv_timeout(Duration::from_millis(800)) {
                mark(&targets, &mut touched, &next);
            }
            let mut changed = false;
            for (target, touched) in targets.iter_mut().zip(touched.iter_mut()) {
                // Every touched target is checked, so each keeps its own state up to date.
                if std::mem::take(touched) {
                    changed |= (target.changed)();
                }
            }
            if changed {
                let _ = app.emit(UPDATED_EVENT, ());
            }
        }
    });
}
