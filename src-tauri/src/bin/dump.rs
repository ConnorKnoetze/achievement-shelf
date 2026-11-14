//! Debugging tool: prints unlocked achievements read from the local Steam files.
//! Usage: `cargo run --bin dump [appid]` (no app ID = the 20 most recent across all games).

use steam_shelf_lib::steam::{stats, SteamPaths};

fn main() -> Result<(), String> {
    let paths = SteamPaths::locate()?;
    println!("Steam: {}  account: {}", paths.root.display(), paths.account_id);

    let achievements = match std::env::args().nth(1) {
        Some(arg) => {
            let app_id: u32 = arg.parse().map_err(|_| format!("not an app ID: {arg}"))?;
            let mut list = stats::load_game(&paths, app_id)?;
            list.sort_by(|a, b| b.unlocked_at.cmp(&a.unlocked_at));
            list
        }
        None => stats::load_all(&paths).into_iter().take(20).collect(),
    };
    for a in &achievements {
        println!("{:>8}  {:>10}  {}  —  {}", a.app_id, a.unlocked_at, a.title, a.description);
    }
    println!("{} unlocked achievement(s)", achievements.len());
    Ok(())
}
