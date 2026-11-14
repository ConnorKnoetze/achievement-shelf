//! Debugging tool: prints unlocked achievements read from the local launcher files.
//! Usage: `cargo run --bin dump [appid]` (no app ID = the 20 most recent Steam unlocks
//! across all games), or `cargo run --bin dump -- --gog` for every GOG Galaxy unlock.

use achievement_shelf_lib::gog::GogPaths;
use achievement_shelf_lib::steam::{stats, SteamPaths};

fn main() -> Result<(), String> {
    let arg = std::env::args().nth(1);
    let achievements = if arg.as_deref() == Some("--gog") {
        let gog = GogPaths::locate()?;
        println!("GOG Galaxy: {}", gog.db.display());
        let names = gog.names()?;
        let list = gog.load_all()?;
        for a in &list {
            let game = names.get(&a.game_key()).map_or(a.game_id.as_str(), String::as_str);
            println!("{game:>24}  {:>10}  {}  —  {}", a.unlocked_at, a.title, a.description);
        }
        list
    } else {
        let paths = SteamPaths::locate()?;
        println!("Steam: {}  account: {}", paths.root.display(), paths.account_id);
        let list = match arg {
            Some(arg) => {
                let app_id: u32 = arg.parse().map_err(|_| format!("not an app ID: {arg}"))?;
                let mut list = stats::load_game(&paths, app_id)?;
                list.sort_by(|a, b| b.unlocked_at.cmp(&a.unlocked_at));
                list
            }
            None => stats::load_all(&paths).into_iter().take(20).collect(),
        };
        for a in &list {
            println!("{:>8}  {:>10}  {}  —  {}", a.game_id, a.unlocked_at, a.title, a.description);
        }
        list
    };
    println!("{} unlocked achievement(s)", achievements.len());
    Ok(())
}
