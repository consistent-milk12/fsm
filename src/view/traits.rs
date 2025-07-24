//! src/view/traits.rs
//! ============================================================================
//! # UI Component Traits
//!
//! Common traits for consistent behavior across UI components, particularly
//! overlays and modals.

use crate::model::app_state::AppState;
use ratatui::{Frame, layout::Rect};

/// Generic overlay trait for consistent modal/overlay behavior
pub trait Overlay {
    /// The name/type of this overlay for debugging
    fn name(&self) -> &'static str;

    /// Render the overlay to the given area
    fn render(&self, frame: &mut Frame<'_>, app: &AppState, area: Rect);

    /// Whether this overlay should consume all input (modal behavior)
    fn is_modal(&self) -> bool {
        true
    }

    /// Whether this overlay blocks background rendering
    fn blocks_background(&self) -> bool {
        true
    }

    /// Priority for overlay stacking (higher = on top)
    fn z_index(&self) -> u8 {
        100
    }

    /// Handle keyboard input for this overlay (return true if handled)
    fn handle_input(&mut self, _key: crossterm::event::KeyEvent) -> bool {
        false
    }

    /// Called when overlay becomes active
    fn on_activate(&mut self, _app: &mut AppState) {}

    /// Called when overlay becomes inactive
    fn on_deactivate(&mut self, _app: &mut AppState) {}
}

/// Trait for components that can be focused/unfocused
pub trait Focusable {
    /// Whether this component currently has focus
    fn is_focused(&self) -> bool;

    /// Set focus state
    fn set_focused(&mut self, focused: bool);

    /// Handle focus-specific input (when focused)
    fn handle_focused_input(&mut self, key: crossterm::event::KeyEvent) -> bool;
}

/// Trait for components with scrollable content
pub trait Scrollable {
    /// Get current scroll position
    fn scroll_position(&self) -> usize;

    /// Scroll up by n items
    fn scroll_up(&mut self, n: usize);

    /// Scroll down by n items  
    fn scroll_down(&mut self, n: usize);

    /// Scroll to specific position
    fn scroll_to(&mut self, position: usize);

    /// Get total scrollable content size
    fn content_size(&self) -> usize;

    /// Get visible area size
    fn visible_size(&self) -> usize;
}
