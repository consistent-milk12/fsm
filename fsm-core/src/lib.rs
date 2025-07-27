pub mod error;

pub mod config;

pub mod cache {
    pub mod cache_manager;
}

pub mod controller {

    pub mod actions;
    pub use actions::Action;

    pub mod event_loop;
    pub use event_loop::{EventLoop, TaskResult};

    pub mod event_processor;

    pub mod state_coordinator;

    pub mod action_batcher;

    pub mod handler_registry;
    pub mod handlers {
        pub mod clipboard_handler;
        pub mod file_ops_handler;
        pub mod keyboard_handler;
        pub mod navigation_handler;
        pub mod search_handler;
    }

    pub mod metrics;

    pub mod eactions;
    pub mod ekey_processor;
    pub mod esimd_matcher;
}

pub mod model {
    pub mod app_state;
    pub use app_state::{AppHistoryEvent, AppState, PluginInfo};

    pub mod command_palette;
    pub use command_palette::{Command, CommandAction, CommandPaletteState};
    pub mod fs_state;
    pub use fs_state::{EntryFilter, EntrySort, FSState, PaneState};

    pub mod ui_state;
    pub use ui_state::{
        FileOperationProgress, LoadingState, Notification, NotificationLevel, RedrawFlag, UIMode,
        UIOverlay, UIState,
    };
}

pub mod view {
    pub mod icons;

    pub mod theme;

    pub mod traits;

    pub mod ui;

    pub mod components {
        pub mod clipboard_overlay;
        pub use clipboard_overlay::OptimizedClipboardOverlay;
        pub mod file_operations_overlay;
        pub use file_operations_overlay::OptimizedFileOperationsOverlay;
        pub mod help_overlay;
        pub use help_overlay::OptimizedHelpOverlay;
        pub mod input_prompt_overlay;
        pub use input_prompt_overlay::OptimizedPromptOverlay;
        pub mod loading_overlay;
        pub use loading_overlay::OptimizedLoadingOverlay;
        pub mod notification_overlay;
        pub use notification_overlay::OptimizedNotificationOverlay;
        pub mod object_table;
        pub use object_table::OptimizedFileTable;
        pub mod search_overlay;
        pub use search_overlay::OptimizedSearchOverlay;
        pub mod search_results_overlay;
        pub use search_results_overlay::OptimizedSearchResultsOverlay;
        pub mod status_bar;
        pub use status_bar::OptimizedStatusBar;
        pub mod error_overlay;
        pub use error_overlay::ErrorOverlay;
    }

    pub use components::*;
}

pub use view::*;

pub mod fs {
    pub mod dir_scanner;
    pub mod object_info;
}

pub mod tasks {

    pub mod filename_search_task;

    pub mod metadata_task;

    pub mod search_task;

    pub mod size_task;

    pub mod file_ops_task;
}

pub mod logging;
pub use logging::Logger;

pub mod plugins;

pub mod util;

pub use error::AppError;

pub use model::{app_state::AppState, fs_state::FSState, ui_state::UIState};
