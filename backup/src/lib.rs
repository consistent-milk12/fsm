//! Library root: public modules for all file manager functionality

pub mod error;

pub mod controller {
    pub mod actions;
    pub mod event_loop;
}

pub mod model {
    pub mod app_state;
    pub mod fs_state;
    pub mod ui_state;
}

pub mod view {
    pub mod ui;
    pub mod components {
        pub mod file_table;
        pub mod help_overlay;
        pub mod loading_overlay;
        pub mod prompt_bar;
        pub mod search_overlay;
        pub mod status_bar;
    }
}

pub mod fs {
    pub mod dir_scanner;
    pub mod file_info;
}

pub mod tasks {
    pub mod search_task;
    pub mod size_task;
}

pub mod cache {
    pub mod cache_manager;
}

pub mod config {
    pub mod config;
    pub mod keymap;
}

pub mod plugins {
    pub mod plugin_api;
}

pub mod util {
    pub mod humanize;
}
