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
use tracing::info;

use tokio::fs as TokioFs;

/// App theme (color scheme) selector.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Default,
    
    Light,
    
    Dark,
    
    Solarized,
    
    Custom(String),
}

/// Keyboard shortcut preset (can be user-extended).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Keymap {
    Vim,
    
    Emacs,
    
    #[default]
    Standard,
    
    Custom(String),
}

/// Profiling configuration for performance monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfilingConfig {
    /// Enable profiling data collection
    pub enabled: bool,
    
    /// Sample rate for profiling (0.0 to 1.0, where 1.0 = 100%)
    pub sample_rate: f32,
    
    /// Enable memory tracking (may have performance impact)
    pub memory_tracking: bool,
    
    /// Enable CPU usage tracking (may have performance impact)
    pub cpu_tracking: bool,
    
    /// Minimum duration (ms) to trigger profiling for an operation
    pub min_duration_ms: u64,
}

impl Default for ProfilingConfig {
    fn default() -> Self {
        Self {
            enabled: true, // Disabled by default for performance
            sample_rate: 0.1, // 10% sampling
            memory_tracking: true,
            cpu_tracking: true,
            min_duration_ms: 5, // Only profile operations taking >5ms
        }
    }
}

/// Cache configuration with sensible defaults - embedded in main Config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Maximum number of entries
    pub max_capacity: u64,
    
    /// Time-to-live for entries
    #[serde(with = "humantime_serde")]
    pub ttl: Duration,
    
    /// Time-to-idle (evict if not accessed)
    #[serde(with = "humantime_serde")]
    pub tti: Duration,
    
    /// Maximum memory usage estimate (MB)
    pub max_memory_mb: u64,
    
    /// Enable cache statistics
    pub enable_stats: bool,
    
    /// Number of shards for concurrent access (power of 2)
    pub num_shards: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_capacity: 32_768,
            ttl: Duration::from_secs(1800), // 30 minutes
            tti: Duration::from_secs(600),  // 10 minutes
            max_memory_mb: 256,
            enable_stats: true,
            num_shards: 64, // Good default for concurrent access
        }
    }
}

/// Main configuration struct for the application.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub theme: Theme,
    
    pub keymap: Keymap,
    
    pub cache: CacheConfig, // Centralized cache configuration
    
    #[serde(default)] // Backward compatibility - use default if missing
    pub profiling: ProfilingConfig, // Performance profiling configuration
    
    pub show_hidden: bool,
    
    pub editor_cmd: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: Theme::Default,
            keymap: Keymap::Standard,
            cache: CacheConfig::default(), // Use centralized cache config
            profiling: ProfilingConfig::default(), // Default profiling config (disabled)
            show_hidden: false,
            editor_cmd: "code".to_string(),
        }
    }
}

impl Config {
    /// Loads config from TOML file at the XDG-compliant app config dir, or returns defaults.
    ///
    /// The config is expected at `$XDG_CONFIG_HOME/FileManager/config.toml`
    /// (Linux), or equivalent on Windows/macOS.
    pub async fn load() -> anyhow::Result<Self> {
        let path = Self::config_path()?;
        if path.exists() {
            info!("Loading config from {}", path.display());
            let text = TokioFs::read_to_string(&path).await?;
            let cfg: Self = toml::from_str(&text)?;
        
            Ok(cfg)
        } else {
            info!(
                "No config file found at {}, using default configuration. Creating it now.",
                path.display()
            );
        
            let default_config = Self::default();
            default_config.save().await?;

            Ok(default_config)
        }
    }

    /// Saves config to TOML file at the XDG-compliant app config dir.
    pub async fn save(&self) -> anyhow::Result<()> {
        let path = Self::config_path()?;
        
        info!("Saving config to {}", path.display());
        
        if let Some(parent) = path.parent() {
            TokioFs::create_dir_all(parent).await?;
        }
        
        let toml_str = toml::to_string_pretty(self)?;
        TokioFs::write(&path, toml_str).await?;
        
        Ok(())
    }

    /// Returns the canonical config file path using `directories::ProjectDirs`.
    pub fn config_path() -> anyhow::Result<PathBuf> {
        // Replace these identifiers to your app's actual organization/name if needed:
        let proj_dirs = ProjectDirs::from("org", "example", "FileManager")
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
