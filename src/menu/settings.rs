//! Menu settings remembered between runs: the last server joined and the
//! hosting port. A small RON file in the user's config directory:
//! - Linux: `$XDG_CONFIG_HOME/harrow/menu.ron` (default `~/.config/harrow/`)
//! - Windows: `%APPDATA%\harrow\menu.ron`
//! - macOS: `~/Library/Application Support/harrow/menu.ron`

use std::path::PathBuf;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::session::DEFAULT_PORT;

#[derive(Resource, Serialize, Deserialize, Debug, Clone)]
#[serde(default)]
pub struct MenuSettings {
    pub last_address: String,
    pub host_port: u16,
}

impl Default for MenuSettings {
    fn default() -> Self {
        Self {
            last_address: "127.0.0.1".into(),
            host_port: DEFAULT_PORT,
        }
    }
}

impl MenuSettings {
    fn path() -> Option<PathBuf> {
        let base = if cfg!(windows) {
            std::env::var_os("APPDATA").map(PathBuf::from)
        } else if cfg!(target_os = "macos") {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
        } else {
            std::env::var_os("XDG_CONFIG_HOME")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        };
        base.map(|b| b.join("harrow").join("menu.ron"))
    }

    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => ron::from_str(&text).unwrap_or_else(|err| {
                warn!("menu settings {} unreadable ({err}); using defaults", path.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else {
            return;
        };
        let result = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|_| {
                let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
                    .map_err(std::io::Error::other)?;
                std::fs::write(&path, text)
            });
        if let Err(err) = result {
            warn!("can't save menu settings to {}: {err}", path.display());
        }
    }
}
