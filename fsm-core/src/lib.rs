#![allow(clippy::missing_errors_doc)]
#![allow(clippy::cargo_common_metadata)]
#![allow(clippy::wildcard_dependencies)]
#![allow(clippy::multiple_crate_versions)]

pub mod error;

pub mod config;

pub mod logging;

pub mod printer;

pub mod error_core;

pub mod cache {
    pub mod cache_manager;
}

pub mod controller {
    pub mod actions;
    pub use actions::Action;

    pub mod event_loop;
    pub use event_loop::{EventLoop, TaskResult};
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
        FileOperationProgress, LoadingState, Notification, NotificationLevel, RedrawFlag,
        SearchType, UIMode, UIOverlay, UIState,
    };

    pub mod metadata_manager;

    pub mod loading_strategy;
    pub use loading_strategy::SmoothedKStrategy;

    pub mod object_registry;
}

pub mod view {
    pub mod icons;

    pub mod theme;

    pub mod traits;
    pub mod ui;
    pub use ui::View;

    pub mod components {
        pub mod command_completion;
        pub use command_completion::CommandCompletion;

        pub mod command_palette;
        pub use command_palette::CommandPalette;

        pub mod content_search_overlay;
        pub use content_search_overlay::ContentSearchOverlay;

        pub mod file_operations_overlay;
        pub use file_operations_overlay::FileOperationsOverlay;

        pub mod filename_search_overlay;
        pub use filename_search_overlay::FileNameSearchOverlay;

        pub mod help_overlay;
        pub use help_overlay::HelpOverlay;

        pub mod input_prompt_overlay;
        pub use input_prompt_overlay::InputPromptOverlay;

        pub mod loading_overlay;
        pub use loading_overlay::LoadingOverlay;

        pub mod notification_overlay;
        pub use notification_overlay::NotificationOverlay;
        pub mod object_table;
        pub use object_table::ObjectTable;

        pub mod prompt_bar;
        pub use prompt_bar::PromptBar;

        pub mod search_overlay;
        pub use search_overlay::SearchOverlay;

        pub mod search_results_overlay;
        pub use search_results_overlay::SearchResultsOverlay;

        pub mod status_bar;
        pub use status_bar::StatusBar;
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

pub mod plugins;

pub mod util;

pub use error::AppError;

pub use model::{app_state::AppState, fs_state::FSState, ui_state::UIState};
