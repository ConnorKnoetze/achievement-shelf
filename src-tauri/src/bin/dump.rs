//! Debugging tool: prints unlocked achievements read from the local launcher files.
//! Usage: `cargo run --bin dump [appid]` (no app ID = the 20 most recent Steam unlocks
//! across all games), or `cargo run --bin dump -- --gog`, `--ubisoft` or `--ea` for every GOG
//! Galaxy, Ubisoft Connect or EA app unlock. `--ea` reads only the current log: its history is
//! kept in a temporary folder, not the app's.

use std::collections::HashMap;

use achievement_shelf_lib::achievement::Achievement;
use achievement_shelf_lib::ea::EaPaths;
use achievement_shelf_lib::gog::GogPaths;
use achievement_shelf_lib::steam::{stats, SteamPaths};
use achievement_shelf_lib::ubisoft::UbisoftPaths;

/// Global unlock percentage from local files only (the app fills Steam gaps from the Web API).
fn rarity(a: &Achievement) -> String {
    a.rarity.map_or_else(|| "-".to_string(), |r| format!("{r:.1}%"))
}

/// Flags the achievement that completed its game.
fn platinum(a: &Achievement) -> &'static str {
    if a.platinum { "  [PLATINUM]" } else { "" }
}

/// Prints a launcher's unlocks with their game names.
fn print_all(list: &[Achievement], names: &HashMap<String, String>) {
    for a in list {
        let game = names.get(&a.game_key()).map_or(a.game_id.as_str(), String::as_str);
        println!("{game:>24}  {:>10}  {:>6}  {}  —  {}{}", a.unlocked_at, rarity(a), a.title, a.description, platinum(a));
    }
}

fn main() -> Result<(), String> {
    let arg = std::env::args().nth(1);
    let achievements = if arg.as_deref() == Some("--ubisoft") {
        let ubisoft = UbisoftPaths::locate()?;
        println!("Ubisoft Connect: {}", ubisoft.spool.display());
        let list = ubisoft.load_all();
        print_all(&list, &ubisoft.names());
        list
    } else if arg.as_deref() == Some("--ea") {
        let history = std::env::temp_dir().join(format!("achievement-shelf-dump-{}", std::process::id()));
        let ea = EaPaths::locate(&history)?;
        println!("EA app: {}", ea.logs.display());
        let list = ea.load_all();
        print_all(&list, &ea.names());
        let _ = std::fs::remove_dir_all(&history);
        list
    } else if arg.as_deref() == Some("--gog") {
        let gog = GogPaths::locate()?;
        println!("GOG Galaxy: {}", gog.db.display());
        let list = gog.load_all()?;
        print_all(&list, &gog.names()?);
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
            None => {
                let all = stats::load_all(&paths);
                let games: Vec<&str> = all.iter().filter(|a| a.platinum).map(|a| a.game_id.as_str()).collect();
                println!("{} completed game(s): {}", games.len(), games.join(", "));
                all.into_iter().take(20).collect()
            }
        };
        for a in &list {
            println!("{:>8}  {:>10}  {:>6}  {}  —  {}{}", a.game_id, a.unlocked_at, rarity(a), a.title, a.description, platinum(a));
        }
        list
    };
    println!("{} unlocked achievement(s)", achievements.len());
    Ok(())
}
