//! src/view/traits.rs
//! ============================================================================
//! # UI Component Traits
//!
//! Common traits for consistent behavior across UI components, particularly
//! overlays and modals.

use crate::model::app_state::AppState;
use ratatui::{Frame, layout::Rect};
use tracing::{debug, trace, instrument};

/// Generic overlay trait for consistent modal/overlay behavior
pub trait Overlay {
    /// The name/type of this overlay for debugging
    fn name(&self) -> &'static str;

    /// Render the overlay to the given area
    #[instrument(level = "trace", skip_all, fields(overlay_name = self.name()))]
    fn render(&self, frame: &mut Frame<'_>, app: &AppState, area: Rect) {
        trace!("Rendering overlay");
    }

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
    #[instrument(level = "debug", skip_all, fields(overlay_name = self.name()))]
    fn on_activate(&mut self, _app: &mut AppState) {
        debug!("Overlay activated");
    }

    /// Called when overlay becomes inactive
    #[instrument(level = "debug", skip_all, fields(overlay_name = self.name()))]
    fn on_deactivate(&mut self, _app: &mut AppState) {
        debug!("Overlay deactivated");
    }
}

/// Trait for components that can be focused/unfocused
pub trait Focusable {
    /// Whether this component currently has focus
    fn is_focused(&self) -> bool;

    /// Set focus state
    #[instrument(level = "trace", skip(self))]
    fn set_focused(&mut self, focused: bool) {
        trace!("Focus changed to: {}", focused);
    }

    /// Handle focus-specific input (when focused)
    #[instrument(level = "trace", skip(self), fields(key_code = ?key.code))]
    fn handle_focused_input(&mut self, key: crossterm::event::KeyEvent) -> bool {
        trace!("Handling focused input");
        false
    }
}

/// Trait for components with scrollable content
pub trait Scrollable {
    /// Get current scroll position
    fn scroll_position(&self) -> usize;

    /// Scroll up by n items
    #[instrument(level = "trace", skip(self))]
    fn scroll_up(&mut self, n: usize) {
        trace!("Scrolling up by {} items", n);
    }

    /// Scroll down by n items  
    #[instrument(level = "trace", skip(self))]
    fn scroll_down(&mut self, n: usize) {
        trace!("Scrolling down by {} items", n);
    }

    /// Scroll to specific position
    #[instrument(level = "trace", skip(self))]
    fn scroll_to(&mut self, position: usize) {
        trace!("Scrolling to position {}", position);
    }

    /// Get total scrollable content size
    fn content_size(&self) -> usize;

    /// Get visible area size
    fn visible_size(&self) -> usize;
}
