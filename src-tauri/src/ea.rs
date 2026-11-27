//! Reads EA app (EA Desktop) achievements from its verbose log,
//! `%LOCALAPPDATA%\Electronic Arts\EA Desktop\Logs\EADesktopVerbose.log` (and the rotated `.bak`).
//!
//! The log records the achievement responses the app receives: an `<AchievementSet Name="…">`
//! listing a game's achievements whenever the app loads them, and a `<Response><Achievement …/>`
//! when one is granted. Game names come from the app's launch lines. The log only covers recent
//! sessions and is rotated, so everything read is kept in a history file and merged with what
//! the log holds now. EA reports no rarity.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::achievement::{crown_platinum, game_key, parse_utc_time, Achievement, Source};
use crate::store;

/// The log file names, current and rotated, start with this.
pub const LOG_PREFIX: &str = "EADesktopVerbose";
const LOG_FILES: [&str; 2] = ["EADesktopVerbose.bak", "EADesktopVerbose.log"];
pub const ICON_HOST: &str = "https://achievements.gameservices.ea.com/achievements/icons/";
const HISTORY_FILE: &str = "ea-history.json";

#[derive(Debug, Clone)]
pub struct EaPaths {
    pub logs: PathBuf,
    /// Where what has been read from the log is kept.
    history: PathBuf,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
struct History {
    /// By game ID (EA's master title ID).
    games: BTreeMap<String, Game>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
struct Game {
    name: String,
    /// Whether the app has listed the game's whole achievement set, so completion is known.
    listed: bool,
    /// By achievement ID.
    achievements: BTreeMap<String, Entry>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
struct Entry {
    title: String,
    description: String,
    /// `<set>-<id>`, which names the icon on [`ICON_HOST`].
    image_id: String,
    unlocked: bool,
    /// Unix seconds; 0 when the log gave no time.
    unlocked_at: u32,
}

impl EaPaths {
    /// Finds the EA app's log folder. `data_dir` is where the history is kept. The folder alone
    /// is not enough: Steam copies of EA games create it for their launch helper's logs.
    pub fn locate(data_dir: &Path) -> Result<Self, String> {
        let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or("EA app not found")?;
        let logs = local.join("Electronic Arts").join("EA Desktop").join("Logs");
        let history = data_dir.join(HISTORY_FILE);
        if !logs.is_dir() || !(LOG_FILES.iter().any(|f| logs.join(f).is_file()) || history.is_file()) {
            return Err("EA app not found".into());
        }
        Ok(Self { logs, history })
    }

    /// Merges the current log into the history, saves it if anything is new, and returns it.
    fn refresh(&self) -> History {
        let mut history: History = store::load(&self.history);
        let before = history.clone();
        for file in LOG_FILES {
            if let Ok(bytes) = std::fs::read(self.logs.join(file)) {
                merge_log(&mut history, &String::from_utf8_lossy(&bytes).replace('\0', ""));
            }
        }
        if history != before {
            store::save(&self.history, &history);
        }
        history
    }

    /// All unlocked achievements, newest first, with each completed game's platinum marked.
    pub fn load_all(&self) -> Vec<Achievement> {
        achievements(&self.refresh())
    }

    /// Names of games with unlocks, keyed by [`game_key`].
    pub fn names(&self) -> HashMap<String, String> {
        self.refresh()
            .games
            .into_iter()
            .filter(|(_, game)| !game.name.is_empty() && game.achievements.values().any(|a| a.unlocked))
            .map(|(id, game)| (game_key(Source::Ea, &id), game.name))
            .collect()
    }

    /// Changes whenever an achievement is unlocked. The app writes to its log all the time, so
    /// the watcher compares this instead of reacting to every write.
    pub fn fingerprint(&self) -> (usize, u32) {
        let history = self.refresh();
        let unlocked = history.games.values().flat_map(|g| g.achievements.values()).filter(|a| a.unlocked);
        unlocked.fold((0, 0), |(count, latest), a| (count + 1, latest.max(a.unlocked_at)))
    }

    /// Follows the end of the current log, so the watcher re-reads the whole log only when
    /// something worth reading was written.
    pub fn tail(&self) -> LogTail {
        let path = self.logs.join(LOG_FILES[1]);
        let read = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        LogTail { path, read }
    }
}

pub struct LogTail {
    path: PathBuf,
    /// How much of the log has been looked at.
    read: u64,
}

impl LogTail {
    /// Whether what was written since the last call may hold achievements or game names. A log
    /// that shrank was rotated, which always counts.
    pub fn has_news(&mut self) -> bool {
        use std::io::{Read, Seek, SeekFrom};
        let Ok(mut file) = std::fs::File::open(&self.path) else { return true };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        if len < self.read {
            self.read = len;
            return true;
        }
        let mut added = Vec::new();
        if file.seek(SeekFrom::Start(self.read)).is_err() || file.read_to_end(&mut added).is_err() {
            return true;
        }
        self.read += added.len() as u64;
        // A marker split across two writes is caught by the next one's overlap.
        let text = String::from_utf8_lossy(&added).replace('\0', "");
        let news = ["<Achievement", "titleName["].iter().any(|m| text.contains(m));
        self.read -= (added.len() as u64).min(MARKER_OVERLAP);
        news
    }
}

/// Bytes re-read at the start of each look at the log, longer than any marker `has_news` checks.
const MARKER_OVERLAP: u64 = 16;

fn achievements(history: &History) -> Vec<Achievement> {
    let mut all = Vec::new();
    let mut complete = HashSet::new();
    for (game_id, game) in &history.games {
        for (id, entry) in &game.achievements {
            if !entry.unlocked || entry.title.is_empty() {
                continue;
            }
            all.push(Achievement {
                source: Source::Ea,
                game_id: game_id.clone(),
                api_name: id.clone(),
                title: entry.title.clone(),
                description: entry.description.clone(),
                hidden: false,
                icon: if entry.image_id.is_empty() { String::new() } else { format!("{ICON_HOST}{}-208.png", entry.image_id) },
                unlocked_at: entry.unlocked_at,
                rarity: None,
                platinum: false,
            });
        }
        if game.listed && !game.achievements.is_empty() && game.achievements.values().all(|a| a.unlocked) {
            complete.insert(game_id.clone());
        }
    }
    crown_platinum(&mut all, |a| complete.contains(&a.game_id));
    all.sort_by(|a, b| b.unlocked_at.cmp(&a.unlocked_at));
    all
}

/// The game ID in an achievement set name such as `51789_196354_50844`: the middle number.
fn game_id_of_set(set: &str) -> Option<&str> {
    let id = set.split('_').filter(|p| !p.is_empty()).nth(1)?;
    id.bytes().all(|b| b.is_ascii_digit()).then_some(id)
}

/// Splits an image ID `achieve:<set>-<id>` (the prefix is optional) into set and achievement ID.
fn split_image_id(image_id: &str) -> Option<(&str, &str)> {
    let raw = image_id.trim();
    let raw = raw.strip_prefix("achieve:").unwrap_or(raw);
    let (set, id) = raw.rsplit_once('-')?;
    (!set.is_empty() && !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())).then_some((set, id))
}

fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';').filter(|&end| end <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => entity
                .strip_prefix("#x")
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|dec| dec.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The `name="value"` attributes of a tag's inside.
fn attributes(tag: &str) -> HashMap<&str, String> {
    let mut attrs = HashMap::new();
    let mut rest = tag;
    while let Some(eq) = rest.find("=\"") {
        let name = rest[..eq].rsplit(|c: char| c.is_whitespace()).next().unwrap_or_default();
        let value_start = eq + 2;
        let Some(len) = rest[value_start..].find('"') else { break };
        if !name.is_empty() {
            attrs.insert(name, decode_entities(&rest[value_start..value_start + len]));
        }
        rest = &rest[value_start + len + 1..];
    }
    attrs
}

/// The inside of the tag that starts at `start`, up to its closing `>` outside quotes.
fn tag_at(text: &str, start: usize) -> Option<&str> {
    let mut quoted = false;
    for (i, c) in text[start..].char_indices() {
        match c {
            '"' => quoted = !quoted,
            '>' if !quoted => return Some(&text[start..start + i]),
            _ => {}
        }
    }
    None
}

/// The value in `key[value]` on a log line.
fn bracketed<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let start = line.find(&format!("{key}["))? + key.len() + 1;
    let len = line[start..].find(']')?;
    Some(line[start..start + len].trim()).filter(|v| !v.is_empty())
}

/// Adds what one log holds to `history`: titles and texts are updated, unlocks only added.
fn merge_log(history: &mut History, text: &str) {
    for line in text.lines() {
        if let (Some(name), Some(id)) = (bracketed(line, "titleName"), bracketed(line, "masterTitleId")) {
            if id.bytes().all(|b| b.is_ascii_digit()) {
                history.games.entry(id.to_string()).or_default().name = name.to_string();
            }
        }
    }

    // The set an `<Achievement>` is listed in; grants outside a set name it in their image ID.
    let mut current_set: Option<String> = None;
    let mut pos = 0;
    while let Some(found) = text[pos..].find('<') {
        let start = pos + found;
        pos = start + 1;
        let rest = &text[start..];
        if rest.starts_with("</AchievementSet>") {
            current_set = None;
        } else if rest.starts_with("<AchievementSet ") || rest.starts_with("<AchievementSet>") {
            let Some(tag) = tag_at(text, start) else { break };
            let attrs = attributes(tag);
            current_set = attrs.get("Name").cloned();
            if let Some(game_id) = current_set.as_deref().and_then(game_id_of_set) {
                let game = history.games.entry(game_id.to_string()).or_default();
                game.listed = true;
                if let Some(name) = attrs.get("GameName").filter(|n| !n.is_empty()) {
                    game.name = name.clone();
                }
            }
        } else if rest.starts_with("<Achievement ") {
            let Some(tag) = tag_at(text, start) else { break };
            merge_achievement(history, current_set.as_deref(), &attributes(tag));
        }
    }
}

fn merge_achievement(history: &mut History, set: Option<&str>, attrs: &HashMap<&str, String>) {
    let image_id = attrs.get("ImageId").map(|i| i.trim().trim_start_matches("achieve:").to_string()).unwrap_or_default();
    let from_image = split_image_id(&image_id);
    let Some(id) = attrs.get("Id").map(|i| i.trim()).filter(|i| !i.is_empty()).or(from_image.map(|(_, id)| id)) else {
        return;
    };
    let Some(game_id) = set.or(from_image.map(|(set, _)| set)).and_then(game_id_of_set) else { return };
    let number = |key: &str| attrs.get(key).and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(0.0);
    let (total, progress) = (number("Total"), number("Progress"));
    let earned = number("Count") > 0.0 || (total > 0.0 && progress >= total);
    let unlocked_at = attrs.get("GrantDate").and_then(|d| parse_utc_time(d)).unwrap_or(0);

    let entry = history.games.entry(game_id.to_string()).or_default().achievements.entry(id.to_string()).or_default();
    let set_text = |field: &mut String, key: &str| {
        if let Some(value) = attrs.get(key).map(|v| v.trim()).filter(|v| !v.is_empty()) {
            *field = value.to_string();
        }
    };
    set_text(&mut entry.title, "Name");
    set_text(&mut entry.description, "Description");
    if entry.description.is_empty() {
        set_text(&mut entry.description, "HowTo");
    }
    if !image_id.is_empty() {
        entry.image_id = image_id;
    }
    if earned {
        if !entry.unlocked || (unlocked_at > 0 && (entry.unlocked_at == 0 || unlocked_at < entry.unlocked_at)) {
            entry.unlocked_at = unlocked_at;
        }
        entry.unlocked = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = r#"2024-05-12 18:00:01.123 [info] Launched game details: titleName[Star Wars Jedi: Survivor™] offerId[Origin.OFR.50.0005386] masterTitleId[196354]
2024-05-12 18:00:02.000 [debug] Response: <?xml version="1.0"?><AchievementSet Name="51789_196354_50844" Platform="PC"><Achievement Id="1" Name="Rock &amp; Roll" Description="Finish the prologue" ImageId="achieve:51789_196354_50844-1" Count="1" GrantDate="2024-05-12T18:22:31Z"/><Achievement Id="2" Name="Explorer" HowTo="Find every chest" ImageId="51789_196354_50844-2" Count="0" Progress="3" Total="10" GrantDate="0000-00-00T00:00:00"/></AchievementSet>
2024-05-12 19:10:00.000 [debug] Grant response: <Response><Achievement Id="2" Name="Explorer" ImageId="achieve:51789_196354_50844-2" Progress="10" Total="10" GrantDate="2024-05-12T19:09:58Z"/></Response>
2024-05-12 19:11:00.000 [debug] <Response><Achievement Name="Unknown set" ImageId="nope" Count="1"/></Response>
"#;

    #[test]
    fn reads_sets_grants_and_names_from_the_log() {
        let mut history = History::default();
        merge_log(&mut history, LOG);
        let list = achievements(&history);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].api_name, "2");
        assert_eq!(list[0].description, "Find every chest");
        assert_eq!(list[0].unlocked_at, 1_715_540_998);
        assert!(list[0].platinum);
        assert_eq!(list[1].title, "Rock & Roll");
        assert_eq!(list[1].icon, "https://achievements.gameservices.ea.com/achievements/icons/51789_196354_50844-1-208.png");
        assert!(list.iter().all(|a| a.game_id == "196354" && a.source == Source::Ea));
        assert_eq!(history.games["196354"].name, "Star Wars Jedi: Survivor™");
    }

    #[test]
    fn a_rotated_log_keeps_earlier_unlocks() {
        let mut history = History::default();
        merge_log(&mut history, LOG);
        let later = r#"<AchievementSet Name="51789_196354_50844"><Achievement Id="1" Name="Rock and Roll" Count="0"/></AchievementSet>"#;
        merge_log(&mut history, later);
        let first = &history.games["196354"].achievements["1"];
        assert!(first.unlocked);
        assert_eq!(first.title, "Rock and Roll");
        assert_eq!(first.unlocked_at, 1_715_538_151);
    }

    #[test]
    fn no_platinum_without_the_whole_set() {
        let mut history = History::default();
        merge_log(&mut history, r#"<Response><Achievement Id="4" Name="Solo" ImageId="1_2_3-4" Count="1"/></Response>"#);
        let list = achievements(&history);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].game_id, "2");
        assert!(!list[0].platinum);
    }

    #[test]
    fn the_tail_reports_only_writes_worth_reading() {
        let dir = std::env::temp_dir().join(format!("achievement-shelf-ea-tail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join(LOG_FILES[1]);
        let append = |text: &str| {
            use std::io::Write;
            std::fs::OpenOptions::new().create(true).append(true).open(&log).unwrap().write_all(text.as_bytes()).unwrap();
        };
        append("startup\n");
        let mut tail = EaPaths { logs: dir.clone(), history: dir.join(HISTORY_FILE) }.tail();
        assert!(!tail.has_news());
        append("[debug] heartbeat\n");
        assert!(!tail.has_news());
        append("<Achiev");
        assert!(!tail.has_news());
        append("ement Id=\"1\"/>\n");
        assert!(tail.has_news(), "a marker split across writes");
        std::fs::write(&log, "rotated\n").unwrap();
        assert!(tail.has_news(), "a rotated log");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn decodes_entities() {
        assert_eq!(decode_entities("a &amp; b &lt;c&gt; &#233;&#x2122; & d"), "a & b <c> é™ & d");
    }
}
