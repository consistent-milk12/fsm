//! lib.rs — Main Library Entry for TUI File Manager
//! -----------------------------------------------
//! Explicitly exposes core, model, view, fs, and task modules for easy access.
//! Follows latest idiomatic Rust module management.
//! Only re-export what you want public in the library crate root.

/// --- Error handling (unified error type for app) ---
pub mod error;

/// --- Cache (object info cache manager, async) ---
pub mod cache {
    pub mod cache_manager;
}

/// --- Configuration: app settings, keymap, themes, etc. ---
pub mod config {
    pub mod config;
    pub mod keymap;
}

/// --- Controller/event loop (main async event handling) ---
pub mod controller {
    pub mod actions;
    pub mod event_loop;
}

/// --- State/data models (MVC model) ---
pub mod model {
    pub mod app_state;
    pub mod command_palette;
    pub mod fs_state;
    pub mod ui_state;
}

/// --- UI rendering: all view logic and components ---
pub mod view {
    pub mod theme;
    pub mod icons;
    pub mod ui; // main UI orchestrator
    pub mod components {
        pub mod command_palette;
        pub mod help_overlay;
        pub use help_overlay::HelpOverlay;
        pub mod loading_overlay;
        pub mod object_table;
        pub mod prompt_bar;
        pub mod search_overlay;
        pub mod status_bar;
    }
    pub use components::*;
    pub use ui::*;
}

/// --- Filesystem abstraction ---
pub mod fs {
    pub mod dir_scanner;
    pub mod object_info;
}

/// --- Background/async tasks ---
pub mod tasks {
    pub mod search_task;
    pub mod size_task;
}

pub mod logging;
pub use logging::Logger;

/// --- Plugins and utilities (optional, if you want to expose) ---
// pub mod plugins;
// pub mod util;

/// --- Crate-level re-exports for the most important types (optional) ---
pub use error::AppError;
pub use model::{app_state::AppState, fs_state::FSState, ui_state::UIState};
