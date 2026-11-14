//! Parses `UserGameStatsSchema_<appid>.bin`: the achievement definitions for one game.

use super::kv::Kv;

#[derive(Debug, Clone, PartialEq)]
pub struct AchievementDef {
    pub stat_id: u32,
    pub bit: u32,
    pub api_name: String,
    pub title: String,
    pub description: String,
    pub hidden: bool,
    /// Icon file name on the Steam CDN, e.g. `d02e...e89.jpg`.
    pub icon: String,
}

/// Picks the English text from a localized value, which is either a plain string
/// or a section of `language -> text` (plus a `token` key).
fn localized(node: Option<&Kv>) -> String {
    match node {
        Some(Kv::Str(s)) => s.clone(),
        Some(section @ Kv::Section(entries)) => section
            .get("english")
            .and_then(Kv::as_str)
            .or_else(|| {
                entries
                    .iter()
                    .find(|(k, _)| !k.eq_ignore_ascii_case("token"))
                    .and_then(|(_, v)| v.as_str())
            })
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

pub fn parse_schema(root: &Kv) -> Vec<AchievementDef> {
    let mut defs = Vec::new();
    // Root holds a single `<appid>` section.
    let Some((_, game)) = root.entries().first() else {
        return defs;
    };
    let Some(stats) = game.get("stats") else {
        return defs;
    };
    for (stat_key, stat) in stats.entries() {
        let (Ok(stat_id), Some(bits)) = (stat_key.parse::<u32>(), stat.get("bits")) else {
            continue;
        };
        for (bit_key, def) in bits.entries() {
            let Ok(bit) = bit_key.parse::<u32>() else { continue };
            if bit >= 32 {
                continue;
            }
            let display = def.get("display");
            defs.push(AchievementDef {
                stat_id,
                bit,
                api_name: def.get("name").and_then(Kv::as_str).unwrap_or_default().to_string(),
                title: localized(display.and_then(|d| d.get("name"))),
                description: localized(display.and_then(|d| d.get("desc"))),
                hidden: display
                    .and_then(|d| d.get("hidden"))
                    .and_then(Kv::as_int)
                    .is_some_and(|h| h != 0),
                icon: display
                    .and_then(|d| d.get("icon"))
                    .and_then(Kv::as_str)
                    .unwrap_or_default()
                    .to_string(),
            });
        }
    }
    defs
}
