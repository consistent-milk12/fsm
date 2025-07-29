//! fsm-core/src/controller/action_dispatcher/navigation_dispatcher.rs
//! Comprehensive tracing for navigation actions

use anyhow::Result;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use tracing::{debug, info, instrument, trace};

use crate::controller::Action;
use crate::controller::state_provider::StateProvider;
use crate::model::ui_state::RedrawFlag;

use super::{ActionMatcher, ActionPriority, DispatchResult};

/// Dispatcher for all navigation‑related actions with tracing.
#[derive(Clone, Debug)]
pub struct NavigationDispatcher {
    /// Provides access to shared application state.
    state_provider: Arc<dyn StateProvider>,
}

impl NavigationDispatcher {
    /// Create a new NavigationDispatcher.
    #[instrument(level = "debug", skip(state_provider))]
    pub fn new(state_provider: Arc<dyn StateProvider>) -> Self {
        Self { state_provider }
    }

    /// Handle single‑step selection moves (up/down, first/last/index).
    #[instrument(level = "debug", skip(self, action), fields(action = ?action))]
    fn handle_selection(&self, action: &Action) -> Option<DispatchResult> {
        // Attempt to move selection; record whether it changed.
        let moved = {
            // Acquire mutable filesystem state
            let mut fs = self.state_provider.fs_state();
            let pane = fs.active_pane_mut();

            // Match on the action type
            match action {
                Action::MoveSelectionUp => pane.move_selection_up(),
                Action::MoveSelectionDown => pane.move_selection_down(),
                Action::SelectFirst => {
                    // Jump to first entry
                    pane.selected.store(0, Ordering::Relaxed);
                    true
                }
                Action::SelectLast => {
                    // Jump to last entry if any
                    let len = pane.entries.len();
                    if len > 0 {
                        pane.selected.store(len - 1, Ordering::Relaxed);
                        true
                    } else {
                        false
                    }
                }
                Action::SelectIndex(index) => {
                    // Select given index if valid
                    if *index < pane.entries.len() {
                        pane.selected.store(*index, Ordering::Relaxed);
                        true
                    } else {
                        false
                    }
                }
                _ => {
                    // Not a selection action
                    trace!("handle_selection: action not applicable");
                    return None;
                }
            }
        };

        debug!("selection moved: {}", moved);
        if moved {
            // Queue a redraw of the main pane
            info!("requesting redraw after selection change");
            self.state_provider.request_redraw(RedrawFlag::Main);
        }

        Some(DispatchResult::Continue)
    }

    /// Handle page‑up and page‑down navigation.
    #[instrument(level = "debug", skip(self, action), fields(action = ?action))]
    fn handle_page_nav(&self, action: &Action) -> Option<DispatchResult> {
        match action {
            Action::PageUp | Action::PageDown => {
                // Get number of lines per page
                let lines = {
                    let fs = self.state_provider.fs_state();
                    fs.active_pane().viewport_height.load(Ordering::Relaxed)
                };
                trace!("page nav lines: {}", lines);

                // Move selection by page
                let moved = {
                    let mut fs = self.state_provider.fs_state();
                    let pane = fs.active_pane_mut();

                    match action {
                        Action::PageUp => (0..lines).any(|_| pane.move_selection_up()),
                        Action::PageDown => (0..lines).any(|_| pane.move_selection_down()),
                        _ => unreachable!(),
                    }
                };

                debug!("page navigation moved: {}", moved);
                if moved {
                    // Redraw full pane if movement occurred
                    info!("requesting redraw after page navigation");
                    self.state_provider.request_redraw(RedrawFlag::Main);
                }

                Some(DispatchResult::Continue)
            }
            _ => {
                trace!("handle_page_nav: action not applicable");
                None
            }
        }
    }

    /// The main handler entry point for navigation actions.
    #[instrument(level = "debug", skip(self, action), fields(action = ?action))]
    pub async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
        // Try selection moves first
        if let Some(result) = self.handle_selection(&action) {
            return Ok(result);
        }

        // Then try page navigation
        if let Some(result) = self.handle_page_nav(&action) {
            return Ok(result);
        }

        // Handle resize events
        if let Action::Resize(_width, height) = action {
            let new_height = height.saturating_sub(2) as usize;
            {
                // Update viewport height
                let mut fs = self.state_provider.fs_state();
                fs.active_pane_mut()
                    .viewport_height
                    .store(new_height, Ordering::Relaxed);
            }
            // Redraw everything
            info!("handling Resize → new_height = {}", new_height);
            self.state_provider.request_redraw(RedrawFlag::All);
            Ok(DispatchResult::Continue)
        } else {
            // Not handled here
            trace!(
                marker = "ACTION_NOT_HANDLED",
                operation_type = "action_dispatch",
                "action not handled by navigation_dispatcher"
            );
            Ok(DispatchResult::NotHandled)
        }
    }
}

impl ActionMatcher for NavigationDispatcher {
    /// True if this dispatcher can handle the given action.
    fn can_handle(&self, action: &Action) -> bool {
        matches!(
            action,
            Action::MoveSelectionUp
                | Action::MoveSelectionDown
                | Action::SelectFirst
                | Action::SelectLast
                | Action::SelectIndex(_)
                | Action::PageUp
                | Action::PageDown
                | Action::Resize(_, _)
        )
    }

    /// Delegate to the async handler.
    #[instrument(level = "debug", skip(self, action), fields(action = ?action))]
    async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
        self.handle(action).await
    }

    /// Navigation is always high priority.
    fn priority(&self) -> ActionPriority {
        ActionPriority::High
    }

    /// Identifier for logging.
    fn name(&self) -> &'static str {
        "navigation"
    }
}
