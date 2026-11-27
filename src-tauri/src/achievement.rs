//! Launcher-neutral achievement data shared by the launcher readers.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// The launcher an achievement comes from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    #[default]
    Steam,
    Gog,
    Ubisoft,
    Ea,
    Retro,
    Xbox,
    Epic,
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Source::Steam => "steam",
            Source::Gog => "gog",
            Source::Ubisoft => "ubisoft",
            Source::Ea => "ea",
            Source::Retro => "retro",
            Source::Xbox => "xbox",
            Source::Epic => "epic",
        })
    }
}

/// `"<source>:<game id>"`, unique across launchers. Used for the game-name cache.
pub fn game_key(source: Source, game_id: &str) -> String {
    format!("{source}:{game_id}")
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Achievement {
    pub source: Source,
    /// Steam app ID, GOG release key such as `gog_1423049311`, Ubisoft product ID, EA master
    /// title ID, RetroAchievements game ID, Xbox title ID, or Epic namespace.
    pub game_id: String,
    pub api_name: String,
    pub title: String,
    pub description: String,
    pub hidden: bool,
    /// Steam: icon file name on the community CDN. GOG, EA, RetroAchievements, Xbox and Epic: full image
    /// URL. Ubisoft: the icon's file name inside the game's achievement archive.
    pub icon: String,
    pub unlocked_at: u32,
    /// Percentage of players who unlocked it, when the launcher knows.
    pub rarity: Option<f32>,
    /// The achievement that completed its game: the last one unlocked in a game whose
    /// every achievement is unlocked. At most one per game.
    pub platinum: bool,
}

impl Achievement {
    pub fn game_key(&self) -> String {
        game_key(self.source, &self.game_id)
    }
}

/// Which of two achievements from the same game counts as unlocked last: the later unlock
/// time, then the greater API name, so the same one is chosen however the list is ordered,
/// even when unlock times are missing (0) or tied.
fn unlocked_later(a: &Achievement, b: &Achievement) -> Ordering {
    a.unlocked_at.cmp(&b.unlocked_at).then_with(|| a.api_name.cmp(&b.api_name))
}

/// Marks the platinum of every game `is_complete` accepts. `achievements` must hold every
/// unlocked achievement of those games, since the last of them is the one marked.
pub fn crown_platinum(achievements: &mut [Achievement], is_complete: impl Fn(&Achievement) -> bool) {
    let mut last: HashMap<String, usize> = HashMap::new();
    for (i, achievement) in achievements.iter().enumerate() {
        if !is_complete(achievement) {
            continue;
        }
        last.entry(achievement.game_key())
            .and_modify(|best| {
                if unlocked_later(achievement, &achievements[*best]).is_gt() {
                    *best = i;
                }
            })
            .or_insert(i);
    }
    for achievement in achievements.iter_mut() {
        achievement.platinum = false;
    }
    for i in last.into_values() {
        achievements[i].platinum = true;
    }
}

/// Parses a UTC `YYYY-MM-DD HH:MM:SS` or ISO 8601 `YYYY-MM-DDTHH:MM:SS[.fff][Z]` time into
/// unix seconds.
pub fn parse_utc_time(text: &str) -> Option<u32> {
    let (date, time) = text.trim().split_once([' ', 'T'])?;
    let mut d = date.splitn(3, '-').map(|p| p.parse::<i64>().ok());
    let (year, month, day) = (d.next()??, d.next()??, d.next()??);
    let mut t = time.splitn(3, ':').map(|p| p.get(..2).unwrap_or(p).parse::<i64>().ok());
    let (hour, minute, second) = (t.next()??, t.next()??, t.next()??);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let secs = days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second;
    u32::try_from(secs).ok()
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unlocked(game_id: &str, api_name: &str, unlocked_at: u32) -> Achievement {
        Achievement {
            source: Source::Steam,
            game_id: game_id.into(),
            api_name: api_name.into(),
            title: String::new(),
            description: String::new(),
            hidden: false,
            icon: String::new(),
            unlocked_at,
            rarity: None,
            platinum: false,
        }
    }

    fn crowned(list: &[Achievement]) -> Vec<&str> {
        list.iter().filter(|a| a.platinum).map(|a| a.api_name.as_str()).collect()
    }

    #[test]
    fn crowns_the_last_unlock_of_complete_games_only() {
        let mut list = vec![
            unlocked("1", "A", 100),
            unlocked("1", "B", 300),
            unlocked("1", "C", 200),
            unlocked("2", "D", 900),
        ];
        crown_platinum(&mut list, |a| a.game_id == "1");
        assert_eq!(crowned(&list), ["B"]);
    }

    #[test]
    fn ties_and_missing_times_pick_the_same_achievement_in_any_order() {
        let mut list = vec![unlocked("1", "A", 0), unlocked("1", "C", 0), unlocked("1", "B", 0)];
        crown_platinum(&mut list, |_| true);
        assert_eq!(crowned(&list), ["C"]);
        list.reverse();
        crown_platinum(&mut list, |_| true);
        assert_eq!(crowned(&list), ["C"]);
    }

    #[test]
    fn same_game_id_on_different_launchers_are_separate_games() {
        let mut list = vec![unlocked("1", "A", 100), unlocked("1", "B", 50)];
        list[1].source = Source::Gog;
        crown_platinum(&mut list, |_| true);
        assert_eq!(crowned(&list), ["A", "B"]);
    }

    #[test]
    fn parses_utc_times() {
        assert_eq!(parse_utc_time("1970-01-01 00:00:00"), Some(0));
        assert_eq!(parse_utc_time("2000-03-01 12:30:15"), Some(951_913_815));
        assert_eq!(parse_utc_time("2024-02-29 23:59:59"), Some(1_709_251_199));
        assert_eq!(parse_utc_time("2022-08-07T00:27:27.000"), Some(1_659_832_047));
        assert_eq!(parse_utc_time("2024-05-12T18:22:31Z"), Some(1_715_538_151));
        assert_eq!(parse_utc_time("not a date"), None);
        assert_eq!(parse_utc_time("2022-13-01 00:00:00"), None);
    }
}
