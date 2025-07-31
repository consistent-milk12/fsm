//! src/view/theme.rs
//! ============================================================================
//! # Catppuccin Mocha Theme Color Palette
//!
//! This module defines the color constants for the Catppuccin Mocha theme.
//! Colors are from the official Catppuccin theme specification:
//! https://github.com/catppuccin/catppuccin

use ratatui::style::{Color, Style};
use tracing::{debug, instrument};

/// Initialize theme colors - logs color palette information
#[instrument(level = "debug")]
pub fn init_theme() {
    debug!("Initializing Catppuccin Mocha theme");
    debug!("Background: {:?}", BACKGROUND);
    debug!("Foreground: {:?}", FOREGROUND);
    debug!("Current line: {:?}", CURRENT_LINE);
}

pub const BACKGROUND: Color = Color::Rgb(30, 30, 46); // Base
pub const CURRENT_LINE: Color = Color::Rgb(69, 71, 90); // Surface1
pub const FOREGROUND: Color = Color::Rgb(81, 224, 81); // #51E051
pub const COMMENT: Color = Color::Rgb(127, 132, 156); // Overlay1
pub const CYAN: Color = Color::Rgb(137, 220, 235); // Sky
pub const GREEN: Color = Color::Rgb(166, 227, 161); // Green
pub const ORANGE: Color = Color::Rgb(250, 179, 135); // Peach
pub const PINK: Color = Color::Rgb(245, 194, 231); // Pink
pub const PURPLE: Color = Color::Rgb(203, 166, 247); // Mauve
pub const RED: Color = Color::Rgb(243, 139, 168); // Red
pub const YELLOW: Color = Color::Rgb(249, 226, 175); // Yellow

pub fn clipboard_overlay_style() -> Style {
    Style::default().bg(BACKGROUND).fg(FOREGROUND)
}

pub fn clipboard_overlay_border_style() -> Style {
    Style::default().fg(PURPLE)
}

pub fn clipboard_empty_message_style() -> Style {
    Style::default().bg(CURRENT_LINE).fg(COMMENT)
}

pub fn clipboard_empty_message_border_style() -> Style {
    Style::default().fg(PURPLE)
}

pub fn clipboard_copy_style() -> Style {
    Style::default().fg(GREEN)
}

pub fn clipboard_move_style() -> Style {
    Style::default().fg(ORANGE)
}

pub fn clipboard_list_style() -> Style {
    Style::default().bg(BACKGROUND).fg(FOREGROUND)
}

pub fn clipboard_list_border_style() -> Style {
    Style::default().fg(PURPLE)
}

pub fn clipboard_highlight_style() -> Style {
    Style::default().bg(CURRENT_LINE).fg(FOREGROUND)
}

pub fn clipboard_details_style() -> Style {
    Style::default().bg(BACKGROUND).fg(FOREGROUND)
}

pub fn clipboard_details_border_style() -> Style {
    Style::default().fg(PURPLE)
}

pub fn clipboard_help_style() -> Style {
    Style::default().bg(BACKGROUND).fg(FOREGROUND)
}

pub fn clipboard_help_border_style() -> Style {
    Style::default().fg(PURPLE)
}
