use crate::error::{ClipError, ClipResult};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardConfig {
    /// Maximum number of items in clipboard
    pub max_items: usize,

    /// Auto-expire items after duration (None = never expire)
    pub item_expiry: Option<Duration>,

    /// Enable clipboard persistence across sessions
    pub persist_clipboard: bool,

    /// Clipboard file location for persistence
    pub clipboard_file: Option<PathBuf>,

    /// Show visual indicators in main UI
    pub show_clipboard_indicators: bool,

    /// Confirm destructive operations
    pub confirm_destructive_ops: bool,
}

impl Default for ClipboardConfig {
    fn default() -> Self {
        Self {
            max_items: 50,
            item_expiry: Some(Duration::from_secs(3600)), // 1 hour
            persist_clipboard: true,
            clipboard_file: None, // Will use default location
            show_clipboard_indicators: true,
            confirm_destructive_ops: true,
        }
    }
}

impl ClipboardConfig {
    pub fn load_from_file<P: AsRef<std::path::Path>>(path: P) -> ClipResult<Self> {
        let content = std::fs::read_to_string(path.as_ref()).map_err(|e| {
            ClipError::ConfigError(format!(
                "Failed to read
  config file: {}",
                e
            ))
        })?;

        let config: Self = toml::from_str(&content).map_err(|e| {
            ClipError::ConfigError(format!(
                "Failed to parse
  config: {}",
                e
            ))
        })?;

        Ok(config)
    }

    pub fn save_to_file<P: AsRef<std::path::Path>>(&self, path: P) -> ClipResult<()> {
        let content = toml::to_string_pretty(self).map_err(|e| {
            ClipError::ConfigError(format!(
                "Failed to
  serialize config: {}",
                e
            ))
        })?;

        std::fs::write(path.as_ref(), content).map_err(|e| {
            ClipError::ConfigError(format!(
                "Failed to write
  config file: {}",
                e
            ))
        })?;

        Ok(())
    }
}
