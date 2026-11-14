//! Launcher-neutral achievement data shared by the Steam and GOG readers.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The launcher an achievement comes from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    #[default]
    Steam,
    Gog,
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Source::Steam => "steam",
            Source::Gog => "gog",
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
    /// Steam app ID, or GOG release key such as `gog_1423049311`.
    pub game_id: String,
    pub api_name: String,
    pub title: String,
    pub description: String,
    pub hidden: bool,
    /// Steam: icon file name on the community CDN. GOG: full image URL.
    pub icon: String,
    pub unlocked_at: u32,
    /// Percentage of players who unlocked it, when the launcher knows.
    pub rarity: Option<f32>,
}

impl Achievement {
    pub fn game_key(&self) -> String {
        game_key(self.source, &self.game_id)
    }
}
