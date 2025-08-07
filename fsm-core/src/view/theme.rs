//! src/view/theme.rs
//! ============================================================================
//! # Catppuccin Mocha Theme Color Palette
//!
//! This module defines the color constants for the Catppuccin Mocha theme.
//! Colors are from the official Catppuccin theme specification:

use ratatui::style::Color;

// pub const BACKGROUND: Color = Color::Rgb(30, 30, 46); // Base
pub const CURRENT_LINE: Color = Color::Rgb(69, 71, 90); // Surface1
// pub const FOREGROUND: Color = Color::Rgb(81, 224, 81); // #51E051
// pub const COMMENT: Color = Color::Rgb(127, 132, 156); // Overlay1
pub const CYAN: Color = Color::Rgb(137, 220, 235); // Sky
pub const GREEN: Color = Color::Rgb(166, 227, 161); // Green
pub const ORANGE: Color = Color::Rgb(250, 179, 135); // Peach
pub const PINK: Color = Color::Rgb(245, 194, 231); // Pink
pub const PURPLE: Color = Color::Rgb(203, 166, 247); // Mauve
pub const RED: Color = Color::Rgb(243, 139, 168); // Red
pub const YELLOW: Color = Color::Rgb(249, 226, 175); // Yellow

/// Main background for the entire UI
pub const BACKGROUND: Color = Color::Rgb(0x1E, 0x1E, 0x3F); // #1E1E3F

/// Default text and foreground elements
pub const FOREGROUND: Color = Color::Rgb(0xA5, 0x99, 0xE9); // #A599E9

/// Background for hovered elements, active selections
pub const HOVER_BG: Color = Color::Rgb(0x4D, 0x21, 0xFC); // #4D21FC

/// Strong accent, used for warning/caution highlights
pub const CONTRAST: Color = Color::Rgb(0xFA, 0xD0, 0x00); // #FAD000

/// Subtle highlight, border accent
pub const CONTRAST_LITE: Color = Color::Rgb(0xFF, 0xEE, 0x80); // #FFEE80

/// Gentler secondary highlight
pub const CONTRAST_LITE_II: Color = Color::Rgb(0xFA, 0xEF, 0xA5); // #FAEFA5

/// Primary highlight for buttons, links, active states
pub const HIGHLIGHT: Color = Color::Rgb(0xFF, 0x72, 0x00); // #FF7200

/// Used for code comments, secondary/disabled UI text
pub const COMMENT: Color = Color::Rgb(0xB3, 0x62, 0xFF); // #B362FF

/// Syntax highlighting: constants
pub const CONSTANTS: Color = Color::Rgb(0xFF, 0x62, 0x8C); // #FF628C

/// Syntax highlighting: keywords, important labels
pub const KEYWORDS: Color = Color::Rgb(0xFF, 0x9D, 0x00); // #FF9D00

/// Syntax highlighting: miscellaneous variables
pub const OTHER: Color = Color::Rgb(0x9E, 0xFF, 0xFF); // #9EFFFF

/// Syntax highlighting: string literals
pub const STRINGS: Color = Color::Rgb(0xA5, 0xFF, 0x90); // #A5FF90

/// Syntax highlighting: templates
pub const TEMPLATES: Color = Color::Rgb(0x3A, 0xD9, 0x00); // #3AD900

/// Syntax highlighting: definitions, declarations
pub const DEFINITIONS: Color = Color::Rgb(0xFB, 0x94, 0xFF); // #FB94FF

/// Used for invalid/error states (alpha not supported in TUI)
pub const INVALID: Color = Color::Rgb(0xEC, 0x3A, 0x37); // #EC3A37

/// Diff highlighting: lines added (alpha ignored)
pub const DIFF_ADDED: Color = Color::Rgb(0x00, 0xFF, 0x00); // #00FF00

/// Diff highlighting: lines removed (alpha ignored)
pub const DIFF_REMOVED: Color = Color::Rgb(0xFF, 0x00, 0x0D); // #FF000D
