//! Reads Ubisoft Connect achievements from the launcher's local files:
//!
//! - `%LOCALAPPDATA%\Ubisoft Game Launcher\spool\<user>\<product id>.spool`: one protobuf record
//!   per unlock, `{1: achievement id, 2: unix time}`, appended the moment it unlocks.
//! - `<launcher>\cache\achievements\<product id>_<spec>`: a ZIP (with no extension) holding
//!   `<locale>_loc.txt` (`id<TAB>name<TAB>description` lines) and `<id>.png` icons.
//! - `...\cache\configuration\configurations`: every product's YAML configuration, which names the
//!   game and its store art, matched to an archive by the `achievements:` spec.
//!
//! Ubisoft has no rarity data, and a game whose archive is not on disk is skipped.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::achievement::{crown_platinum, game_key, Achievement, Source};

/// Where the launcher's store art lives. `background_image` values are file names on this host.
pub const ASSET_HOST: &str = "https://static3.cdn.ubi.com/orbit/uplay_launcher_3_0/assets/";

#[derive(Debug, Clone)]
pub struct UbisoftPaths {
    /// `spool`, with one folder per signed-in user.
    pub spool: PathBuf,
    /// Launcher folders that may hold `cache\achievements` and `cache\configuration`.
    launcher_dirs: Vec<PathBuf>,
}

/// One game's achievement texts and icons, from its archive.
#[derive(Debug, Default)]
struct Schema {
    /// Name and description by achievement id.
    texts: HashMap<u64, (String, String)>,
    /// Icon entry name inside the archive, by achievement id.
    icons: HashMap<u64, String>,
}

/// What a product's configuration says about its game.
#[derive(Debug, Default, PartialEq)]
struct Config {
    /// The `achievements:` spec, normalised by [`normalize_spec`].
    spec: String,
    title: String,
    background: String,
}

impl UbisoftPaths {
    pub fn locate() -> Result<Self, String> {
        let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or("Ubisoft Connect not found")?;
        let spool = local.join("Ubisoft Game Launcher").join("spool");
        if !spool.is_dir() {
            return Err("Ubisoft Connect not found".into());
        }
        let mut launcher_dirs: Vec<PathBuf> = registry_launcher_dirs();
        launcher_dirs.push(local.join("Ubisoft Game Launcher"));
        for var in ["PROGRAMDATA", "ProgramFiles(x86)", "ProgramFiles"] {
            if let Some(base) = std::env::var_os(var) {
                launcher_dirs.push(PathBuf::from(base).join("Ubisoft").join("Ubisoft Game Launcher"));
            }
        }
        let mut seen = HashSet::new();
        launcher_dirs.retain(|dir| dir.is_dir() && seen.insert(dir.to_string_lossy().to_lowercase()));
        Ok(Self { spool, launcher_dirs })
    }

    /// The per-user spool folders, which the watcher watches.
    pub fn spool_dirs(&self) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(&self.spool) else { return Vec::new() };
        entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect()
    }

    /// Each product's spool file. With several users, the most recently written one.
    fn spool_files(&self) -> HashMap<u64, PathBuf> {
        let mut newest: HashMap<u64, (SystemTime, PathBuf)> = HashMap::new();
        for dir in self.spool_dirs() {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for path in entries.flatten().map(|e| e.path()) {
                let is_spool = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("spool"));
                let Some(product) = path.file_stem().and_then(|s| s.to_str()).and_then(|s| s.parse().ok()) else {
                    continue;
                };
                if !is_spool {
                    continue;
                }
                let modified = modified(&path);
                if newest.get(&product).is_none_or(|(at, _)| modified > *at) {
                    newest.insert(product, (modified, path));
                }
            }
        }
        newest.into_iter().map(|(product, (_, path))| (product, path)).collect()
    }

    /// The product's achievement archive: the newest `<product>_*` file in any launcher folder.
    fn archive(&self, product: u64) -> Option<PathBuf> {
        let prefix = format!("{product}_");
        self.launcher_dirs
            .iter()
            .filter_map(|dir| std::fs::read_dir(dir.join("cache").join("achievements")).ok())
            .flat_map(|entries| entries.flatten().map(|e| e.path()))
            .filter(|p| p.is_file() && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(&prefix)))
            .max_by_key(|p| modified(p))
    }

    fn configs(&self) -> Vec<Config> {
        self.launcher_dirs
            .iter()
            .map(|dir| dir.join("cache").join("configuration").join("configurations"))
            .find_map(|path| std::fs::read(path).ok())
            .map(|bytes| parse_configurations(&String::from_utf8_lossy(&bytes)))
            .unwrap_or_default()
    }

    /// The configuration of the product whose archive is `archive`.
    fn config_for<'a>(configs: &'a [Config], product: u64, archive: &Path) -> Option<&'a Config> {
        let name = archive.file_name()?.to_str()?;
        let spec = normalize_spec(name.strip_prefix(&format!("{product}_"))?);
        configs.iter().find(|c| c.spec == spec && !c.title.is_empty())
    }

    /// All unlocked achievements, newest first, with each completed game's platinum marked.
    pub fn load_all(&self) -> Vec<Achievement> {
        let mut all = Vec::new();
        let mut complete = HashSet::new();
        for (product, spool) in self.spool_files() {
            let unlocks = match std::fs::read(&spool) {
                Ok(bytes) => read_spool(&bytes),
                Err(e) => {
                    eprintln!("achievement-shelf: cannot read {}: {e}", spool.display());
                    continue;
                }
            };
            if unlocks.is_empty() {
                continue;
            }
            let Some(archive) = self.archive(product) else { continue };
            let schema = match read_schema(&archive) {
                Ok(schema) => schema,
                Err(e) => {
                    eprintln!("achievement-shelf: cannot read Ubisoft achievements of {product}: {e}");
                    continue;
                }
            };
            let game_id = product.to_string();
            for (&id, &unlocked_at) in &unlocks {
                let Some((title, description)) = schema.texts.get(&id) else { continue };
                all.push(Achievement {
                    source: Source::Ubisoft,
                    game_id: game_id.clone(),
                    api_name: id.to_string(),
                    title: title.clone(),
                    description: description.clone(),
                    hidden: false,
                    icon: schema.icons.get(&id).cloned().unwrap_or_default(),
                    unlocked_at,
                    rarity: None,
                    platinum: false,
                });
            }
            if schema.texts.keys().all(|id| unlocks.contains_key(id)) {
                complete.insert(game_id);
            }
        }
        crown_platinum(&mut all, |a| complete.contains(&a.game_id));
        all.sort_by(|a, b| b.unlocked_at.cmp(&a.unlocked_at));
        all
    }

    /// Names of games with unlocks, keyed by [`game_key`]: from the launcher's configurations,
    /// else the name of the game's install folder.
    pub fn names(&self) -> HashMap<String, String> {
        let configs = self.configs();
        let mut names = HashMap::new();
        for product in self.spool_files().into_keys() {
            let from_config = self
                .archive(product)
                .and_then(|archive| Self::config_for(&configs, product, &archive).map(|c| c.title.clone()));
            if let Some(name) = from_config.or_else(|| registry_install_name(product)) {
                names.insert(game_key(Source::Ubisoft, &product.to_string()), name);
            }
        }
        names
    }

    /// Store background image URLs of games with unlocks, keyed by [`game_key`].
    pub fn banners(&self) -> HashMap<String, String> {
        let configs = self.configs();
        let mut banners = HashMap::new();
        for product in self.spool_files().into_keys() {
            let Some(archive) = self.archive(product) else { continue };
            let Some(config) = Self::config_for(&configs, product, &archive) else { continue };
            if !config.background.is_empty() {
                banners.insert(game_key(Source::Ubisoft, &product.to_string()), format!("{ASSET_HOST}{}", config.background));
            }
        }
        banners
    }

    /// The PNG of the icon `entry` in the product's archive.
    pub fn icon(&self, product: u64, entry: &str) -> Option<Vec<u8>> {
        let archive = self.archive(product)?;
        let mut zip = zip::ZipArchive::new(File::open(archive).ok()?).ok()?;
        let mut file = zip.by_name(entry).ok()?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).ok()?;
        Some(bytes)
    }
}

fn modified(path: &Path) -> SystemTime {
    std::fs::metadata(path).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH)
}

#[cfg(windows)]
const LAUNCHER_KEYS: [&str; 2] = [r"SOFTWARE\WOW6432Node\Ubisoft\Launcher", r"SOFTWARE\Ubisoft\Launcher"];

#[cfg(windows)]
fn registry_value(subkey: &str, name: &str) -> Option<String> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER].into_iter().find_map(|hive| {
        let key = winreg::RegKey::predef(hive).open_subkey(subkey).ok()?;
        key.get_value::<String, _>(name).ok().filter(|v| !v.trim().is_empty())
    })
}

/// The launcher's install folders the registry names.
#[cfg(windows)]
fn registry_launcher_dirs() -> Vec<PathBuf> {
    LAUNCHER_KEYS.iter().filter_map(|key| registry_value(key, "InstallDir")).map(PathBuf::from).collect()
}

#[cfg(not(windows))]
fn registry_launcher_dirs() -> Vec<PathBuf> {
    Vec::new()
}

/// The name of the folder the product is installed in, e.g. `Far Cry 5`.
#[cfg(windows)]
fn registry_install_name(product: u64) -> Option<String> {
    LAUNCHER_KEYS.iter().find_map(|key| {
        let dir = registry_value(&format!(r"{key}\Installs\{product}"), "InstallDir")?;
        let name = Path::new(dir.trim_end_matches(['/', '\\'])).file_name()?.to_str()?.to_string();
        Some(name)
    })
}

#[cfg(not(windows))]
fn registry_install_name(_product: u64) -> Option<String> {
    None
}

/// Reads a protobuf varint at `pos`, moving past it.
fn varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *buf.get(*pos)?;
        *pos += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

/// Moves `pos` past a field of `wire` type; `None` for a type that cannot be skipped.
fn skip(buf: &[u8], pos: &mut usize, wire: u64) -> Option<()> {
    match wire {
        0 => {
            varint(buf, pos)?;
        }
        1 => *pos += 8,
        2 => {
            let len = usize::try_from(varint(buf, pos)?).ok()?;
            *pos = pos.checked_add(len)?;
        }
        5 => *pos += 4,
        _ => return None,
    }
    (*pos <= buf.len()).then_some(())
}

/// The first varint `field` in `buf`, looking into nested messages a few levels deep.
fn first_varint(buf: &[u8], field: u64, depth: u32) -> Option<u64> {
    let mut pos = 0;
    while pos < buf.len() {
        let tag = varint(buf, &mut pos)?;
        let (number, wire) = (tag >> 3, tag & 7);
        match wire {
            0 => {
                let value = varint(buf, &mut pos)?;
                if number == field {
                    return Some(value);
                }
            }
            2 => {
                let len = usize::try_from(varint(buf, &mut pos)?).ok()?;
                let end = pos.checked_add(len).filter(|&end| end <= buf.len())?;
                if depth < 4 {
                    if let Some(value) = first_varint(&buf[pos..end], field, depth + 1) {
                        return Some(value);
                    }
                }
                pos = end;
            }
            _ => skip(buf, &mut pos, wire)?,
        }
    }
    None
}

/// The unlocks in a spool file: unix time by achievement id, the earliest when one repeats.
/// A damaged tail is ignored; the records before it are kept.
fn read_spool(buf: &[u8]) -> HashMap<u64, u32> {
    let mut unlocks: HashMap<u64, u32> = HashMap::new();
    let mut pos = 0;
    while pos < buf.len() {
        let Some(tag) = varint(buf, &mut pos) else { break };
        let (number, wire) = (tag >> 3, tag & 7);
        if number == 1 && wire == 2 {
            let Some(len) = varint(buf, &mut pos).and_then(|l| usize::try_from(l).ok()) else { break };
            let Some(end) = pos.checked_add(len).filter(|&end| end <= buf.len()) else { break };
            let record = &buf[pos..end];
            pos = end;
            let (Some(id), Some(time)) = (first_varint(record, 1, 0), first_varint(record, 2, 0)) else { continue };
            // Some launcher versions write milliseconds.
            let time = if time >= 10_000_000_000 { time / 1000 } else { time };
            let Ok(time) = u32::try_from(time) else { continue };
            if id > 0 && time > 0 {
                unlocks.entry(id).and_modify(|t| *t = (*t).min(time)).or_insert(time);
            }
        } else if skip(buf, &mut pos, wire).is_none() {
            break;
        }
    }
    unlocks
}

/// `id<TAB>name<TAB>description` lines of a `<locale>_loc.txt`.
fn parse_localization(text: &str) -> HashMap<u64, (String, String)> {
    let mut texts = HashMap::new();
    for line in text.trim_start_matches('\u{feff}').lines() {
        let mut parts = line.trim().splitn(3, '\t');
        let (Some(id), Some(name), Some(description)) = (parts.next(), parts.next(), parts.next()) else { continue };
        if let Ok(id) = id.trim().parse() {
            texts.insert(id, (name.trim().to_string(), description.trim().to_string()));
        }
    }
    texts
}

/// Preference for a localization file's locale: English first.
fn locale_rank(locale: &str) -> u8 {
    match locale {
        "en-us" => 0,
        "en-gb" | "en" => 1,
        l if l.starts_with("en") => 2,
        _ => 3,
    }
}

fn read_schema(archive: &Path) -> Result<Schema, String> {
    let file = File::open(archive).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let mut schema = Schema::default();
    let mut best: Option<(u8, String)> = None;
    for name in zip.file_names().flatten().map(|n| n.into_owned()).collect::<Vec<_>>() {
        let lower = name.to_ascii_lowercase();
        if let Some(locale) = lower.strip_suffix("_loc.txt") {
            let rank = locale_rank(locale);
            if best.as_ref().is_none_or(|(r, _)| rank < *r) {
                best = Some((rank, name));
            }
        } else if let Some(id) = lower.strip_suffix(".png").and_then(|stem| stem.parse().ok()) {
            schema.icons.insert(id, name);
        }
    }
    let (_, loc) = best.ok_or("no localization in the achievement archive")?;
    let mut bytes = Vec::new();
    zip.by_name(&loc).map_err(|e| e.to_string())?.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    schema.texts = parse_localization(&String::from_utf8_lossy(&bytes));
    Ok(schema)
}

/// An archive spec as configurations and archive file names both reduce to: the file name,
/// lower case, without `.zip` or a leading `<number>_`.
fn normalize_spec(spec: &str) -> String {
    let base = spec.trim().trim_matches(['"', '\'']).rsplit(['/', '\\']).next().unwrap_or_default();
    let base = base.to_lowercase();
    let base = base.strip_suffix(".zip").unwrap_or(&base);
    match base.split_once('_') {
        Some((number, rest)) if !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()) => rest.to_string(),
        _ => base.to_string(),
    }
}

/// Whether a configured name is an unresolved localization key (`l1`, `NAME`,
/// `RELATED_GAMENAME_116`) rather than a title.
fn is_placeholder(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let slot = lower.strip_prefix('l').is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    name.is_empty() || slot || !name.chars().any(|c| c.is_whitespace() || c.is_lowercase())
}

/// The value of the first `key: value` line in `block`, unquoted.
fn yaml_value<'a>(block: &'a str, key: &str) -> Option<&'a str> {
    block.lines().find_map(|line| {
        let value = line.trim_start().strip_prefix(key)?.strip_prefix(':')?;
        let value = value.trim().trim_matches(['"', '\'']).trim();
        (!value.is_empty()).then_some(value)
    })
}

/// Every product configuration that names an achievement archive. The file is binary framing
/// around YAML documents that each start with a `version:` line followed by `root:`.
fn parse_configurations(text: &str) -> Vec<Config> {
    let text = text.replace('\0', "");
    let mut starts: Vec<usize> = Vec::new();
    let mut from = 0;
    while let Some(found) = text[from..].find("version:") {
        let at = from + found;
        let next_line = text[at..].find('\n').map(|n| at + n + 1);
        if next_line.is_some_and(|n| text[n..].starts_with("root:")) {
            starts.push(at);
        }
        from = at + "version:".len();
    }
    let mut configs = Vec::new();
    for (i, &start) in starts.iter().enumerate() {
        let block = &text[start..starts.get(i + 1).copied().unwrap_or(text.len())];
        let Some(spec) = yaml_value(block, "achievements") else { continue };
        let title = ["game_identifier", "display_name"]
            .iter()
            .filter_map(|key| yaml_value(block, key))
            .find(|name| !is_placeholder(name))
            .unwrap_or_default();
        let background = yaml_value(block, "background_image")
            .filter(|f| f.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-'))
            .filter(|f| [".jpg", ".png"].iter().any(|ext| f.to_ascii_lowercase().ends_with(ext)))
            .unwrap_or_default();
        configs.push(Config { spec: normalize_spec(spec), title: title.to_string(), background: background.to_string() });
    }
    configs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn put_varint(out: &mut Vec<u8>, mut value: u64) {
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                out.push(byte);
                return;
            }
            out.push(byte | 0x80);
        }
    }

    fn record(id: u64, time: u64) -> Vec<u8> {
        let mut inner = Vec::new();
        put_varint(&mut inner, 1 << 3);
        put_varint(&mut inner, id);
        put_varint(&mut inner, 2 << 3);
        put_varint(&mut inner, time);
        let mut out = vec![(1 << 3) | 2];
        put_varint(&mut out, inner.len() as u64);
        out.extend(inner);
        out
    }

    #[test]
    fn reads_spool_records_keeping_the_earliest_unlock() {
        let mut spool = record(3, 1_700_000_000);
        spool.extend([(5 << 3), 42]); // an unrelated varint field
        spool.extend(record(7, 1_700_000_500_123)); // milliseconds
        spool.extend(record(3, 1_700_000_900));
        spool.extend([(1 << 3) | 2, 50, 1]); // truncated tail
        let unlocks = read_spool(&spool);
        assert_eq!(unlocks.len(), 2);
        assert_eq!(unlocks[&3], 1_700_000_000);
        assert_eq!(unlocks[&7], 1_700_000_500);
    }

    #[test]
    fn reads_the_english_texts_and_icons_of_an_archive() {
        let path = std::env::temp_dir().join(format!("achievement-shelf-ubisoft-{}", std::process::id()));
        {
            let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("fr-FR_loc.txt", options).unwrap();
            zip.write_all("1\tPremier pas\tFinir le tutoriel\n".as_bytes()).unwrap();
            zip.start_file("en-US_loc.txt", options).unwrap();
            zip.write_all("\u{feff}001\tFirst Steps\tFinish the tutorial\n2\tTabs\tHas a\ttab\nbad line\n".as_bytes()).unwrap();
            zip.start_file("001.png", options).unwrap();
            zip.write_all(b"\x89PNG").unwrap();
            zip.finish().unwrap();
        }
        let schema = read_schema(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(schema.texts.len(), 2);
        assert_eq!(schema.texts[&1], ("First Steps".to_string(), "Finish the tutorial".to_string()));
        assert_eq!(schema.texts[&2].1, "Has a\ttab");
        assert_eq!(schema.icons[&1], "001.png");
    }

    #[test]
    fn normalizes_archive_specs() {
        assert_eq!(normalize_spec("\"achievements/635_FarCry5.zip\""), "farcry5");
        assert_eq!(normalize_spec("FarCry5"), "farcry5");
        assert_eq!(normalize_spec("v2_FarCry5"), "v2_farcry5");
    }

    #[test]
    fn reads_titles_and_art_from_configurations() {
        let text = "\u{1}\u{0}junkversion: 2.0\nroot:\n  name: l1\n  display_name: RELATED_GAMENAME_1\n  game_identifier: 'Far Cry® 5'\n  achievements: 635_FarCry5.zip\n  background_image: ab12_cd.jpg\n\u{2}version: 2.0\nroot:\n  name: Ubisoft Connect\n  display_name: \"Anno 1800\"\n  achievements: anno1800\n  background_image: l3\nversion: 2.0\nroot:\n  name: No achievements\n";
        let configs = parse_configurations(text);
        assert_eq!(
            configs,
            [
                Config { spec: "farcry5".into(), title: "Far Cry® 5".into(), background: "ab12_cd.jpg".into() },
                Config { spec: "anno1800".into(), title: "Anno 1800".into(), background: String::new() },
            ]
        );
    }

    #[test]
    fn spots_localization_keys() {
        assert!(is_placeholder("l1"));
        assert!(is_placeholder("NAME"));
        assert!(is_placeholder("RELATED_GAMENAME_116"));
        assert!(!is_placeholder("Far Cry 5"));
        assert!(!is_placeholder("Anno"));
    }
}
