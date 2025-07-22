//! src/config/config.rs
//! ============================================================================
//! # Config: Application Configuration Loader and Saver (directories only)
//!
//! Manages all user-editable configuration settings for the file manager.
//! Loads and saves settings as TOML from the proper cross-platform config path
//! using the [`directories`](https://docs.rs/directories) crate.
//!
//! ## Features
//! - XDG-compliant config discovery and writing (Linux, macOS, Windows)
//! - Robust defaulting if no config file exists
//! - Async load/save for smooth integration with Tokio
//! - Extensible: add fields as needed for UI, cache, plugins, etc.
//!
//! ## Example
//! ```rust,ignore
//! let config = Config::load().await?;
//! config.save().await?;
//! ```

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

/// App theme (color scheme) selector.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Default,
    Light,
    Dark,
    Solarized,
    Custom(String),
}
impl Default for Theme {
    fn default() -> Self {
        Theme::Default
    }
}

/// Keyboard shortcut preset (can be user-extended).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Keymap {
    Vim,
    Emacs,
    Standard,
    Custom(String),
}
impl Default for Keymap {
    fn default() -> Self {
        Keymap::Standard
    }
}

/// Main configuration struct for the application.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub theme: Theme,
    pub keymap: Keymap,
    pub cache_entries: u64,
    #[serde(with = "humantime_serde")]
    pub cache_ttl: Duration,
    pub show_hidden: bool,
    pub editor_cmd: String,
    // Add more config fields here as needed
}

impl Default for Config {
    fn default() -> Self {
        Config {
            theme: Theme::Default,
            keymap: Keymap::Standard,
            cache_entries: 5000,
            cache_ttl: Duration::from_secs(600),
            show_hidden: false,
            editor_cmd: "nvim".to_string(),
        }
    }
}

impl Config {
    /// Loads config from TOML file at the XDG-compliant app config dir, or returns defaults.
    ///
    /// The config is expected at `$XDG_CONFIG_HOME/FileManager/config.toml`
    /// (Linux), or equivalent on Windows/macOS.
    pub async fn load() -> anyhow::Result<Self> {
        let path: PathBuf = Self::config_path()?;
        if path.exists() {
            let text: String = tokio::fs::read_to_string(&path).await?;
            let cfg: Config = toml::from_str(&text)?;
            Ok(cfg)
        } else {
            Ok(Config::default())
        }
    }

    /// Saves config to TOML file at the XDG-compliant app config dir.
    pub async fn save(&self) -> anyhow::Result<()> {
        let path: PathBuf = Self::config_path()?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let toml_str: String = toml::to_string_pretty(self)?;
        tokio::fs::write(&path, toml_str).await?;
        Ok(())
    }

    /// Returns the canonical config file path using `directories::ProjectDirs`.
    pub fn config_path() -> anyhow::Result<PathBuf> {
        // Replace these identifiers to your app's actual organization/name if needed:
        let proj_dirs: ProjectDirs = ProjectDirs::from("org", "example", "FileManager")
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory."))?;
        Ok(proj_dirs.config_dir().join("config.toml"))
    }

    /// Returns the config directory (without filename), if you need to display or manipulate it.
    pub fn config_dir() -> anyhow::Result<PathBuf> {
        let proj_dirs = ProjectDirs::from("org", "example", "FileManager")
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory."))?;
        Ok(proj_dirs.config_dir().to_path_buf())
    }
}
