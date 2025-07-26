<!-- src/config.rs -->

# Config: Application Configuration

This module manages all user-editable configuration settings for the file manager. It handles loading and saving settings in TOML format to a standard, cross-platform configuration directory.

## Features

- **XDG-Compliant**: Uses the `directories` crate to find the correct configuration path on Linux, macOS, and Windows.
- **Robust Defaulting**: If no configuration file is found, it creates one with default settings.
- **Asynchronous**: `load` and `save` operations are `async`, integrating smoothly with Tokio.
- **Extensible**: The `Config` struct can be easily extended with new fields as the application grows.

## `Theme` (Enum)

Defines the available color schemes for the application.

- `Default`
- `Light`
- `Dark`
- `Solarized`
- `Custom(String)`: Allows specifying a path to a custom theme file.

## `Keymap` (Enum)

Defines the keyboard shortcut presets.

- `Vim`
- `Emacs`
- `Standard` (Default)
- `Custom(String)`: Allows specifying a path to a custom keymap file.

## `CacheConfig` (Struct)

Contains all configuration settings related to the object metadata cache.

- `max_capacity`: The maximum number of entries the cache can hold.
- `ttl`: Time-to-live for cache entries.
- `tti`: Time-to-idle (entries are evicted if not accessed for this duration).
- `max_memory_mb`: An approximate memory usage limit for the cache in megabytes.
- `enable_stats`: A boolean to enable or disable cache statistics collection.
- `num_shards`: The number of concurrent shards for the cache, improving performance under heavy load.

## `Config` (Struct)

The main configuration struct for the application.

- `theme`: The selected `Theme`.
- `keymap`: The selected `Keymap`.
- `cache`: The `CacheConfig` settings.
- `show_hidden`: A boolean to determine if hidden files should be shown.
- `editor_cmd`: The command to use for opening files in an external editor.

### Key Methods

- `load() -> anyhow::Result<Self>`: Asynchronously loads the configuration from the TOML file. If the file doesn't exist, it creates a default configuration and saves it.
- `save(&self) -> anyhow::Result<()>`: Asynchronously saves the current configuration to the TOML file.
- `config_path() -> anyhow::Result<PathBuf>`: Returns the full, canonical path to the `config.toml` file.
- `config_dir() -> anyhow::Result<PathBuf>`: Returns the path to the configuration directory.
