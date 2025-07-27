//! action_dispatchers/navigation.rs
//! High-performance navigation action handling

use anyhow::Result;
use std::sync::MutexGuard;
use std::sync::{Arc, atomic::Ordering};

use super::{ActionHandler, DispatchResult};
use crate::controller::{Action, state_coordinator::StateCoordinator};
use crate::model::{FSState, PaneState, RedrawFlag};

/// Specialized dispatcher for navigation actions
pub struct NavigationDispatcher {
    state: Arc<StateCoordinator>,
}

impl NavigationDispatcher {
    pub fn new(state: Arc<StateCoordinator>) -> Self {
        Self { state }
    }

    /// Fast path for selection movement without async overhead
    fn handle_selection_movement(&self, action: &Action) -> Option<DispatchResult> {
        let movement_result: bool = {
            let fs: MutexGuard<'_, FSState> = self.state.fs_state();
            let pane: &PaneState = fs.active_pane();

            match action {
                Action::MoveSelectionUp => pane.move_selection_up(),

                Action::MoveSelectionDown => pane.move_selection_down(),

                Action::SelectFirst => {
                    pane.selected.store(0, Ordering::Relaxed);

                    true
                }

                Action::SelectLast => {
                    let len: usize = pane.entries.len();

                    if len > 0 {
                        pane.selected.store(len - 1, Ordering::Relaxed);

                        true
                    } else {
                        false
                    }
                }

                Action::SelectIndex(index) => {
                    let len: usize = pane.entries.len();

                    if *index < len {
                        pane.selected.store(*index, Ordering::Relaxed);

                        true
                    } else {
                        false
                    }
                }

                _ => return None,
            }
        };

        if movement_result {
            self.state.request_redraw(RedrawFlag::Main);
        }

        Some(DispatchResult::Continue)
    }

    /// Handle page navigation with viewport-aware logic
    fn handle_page_navigation(&self, action: &Action) -> Option<DispatchResult> {
        match action {
            Action::PageUp | Action::PageDown => {
                let lines: usize = {
                    let fs: MutexGuard<'_, FSState> = self.state.fs_state();

                    fs.active_pane().viewport_height.load(Ordering::Relaxed)
                };

                let fs: MutexGuard<'_, FSState> = self.state.fs_state();
                let pane: &PaneState = fs.active_pane();

                let moved: bool = match action {
                    Action::PageUp => (0..lines).any(|_| pane.move_selection_up()),

                    Action::PageDown => (0..lines).any(|_| pane.move_selection_down()),

                    _ => unreachable!(),
                };

                if moved {
                    self.state.request_redraw(RedrawFlag::Main);
                }

                Some(DispatchResult::Continue)
            }

            _ => None,
        }
    }

    /// Handle resize events efficiently
    fn handle_resize(&self, _width: u16, height: u16) -> DispatchResult {
        let new_height: usize = height.saturating_sub(2) as usize;

        let fs: MutexGuard<'_, FSState> = self.state.fs_state();

        fs.active_pane()
            .viewport_height
            .store(new_height, Ordering::Relaxed);

        self.state.request_redraw(RedrawFlag::All);

        DispatchResult::Continue
    }
}

impl ActionHandler for NavigationDispatcher {
    fn can_handle(&self, action: &Action) -> bool {
        matches!(
            action,
            Action::MoveSelectionUp
                | Action::MoveSelectionDown
                | Action::PageUp
                | Action::PageDown
                | Action::SelectFirst
                | Action::SelectLast
                | Action::SelectIndex(_)
                | Action::Resize(_, _)
        )
    }

    async fn handle(&mut self, action: &Action) -> Result<DispatchResult> {
        // Fast path for selection movements (no async needed)
        if let Some(result) = self.handle_selection_movement(&action) {
            return Ok(result);
        }

        // Fast path for page navigation
        if let Some(result) = self.handle_page_navigation(&action) {
            return Ok(result);
        }

        // Handle other navigation actions
        match action {
            Action::Resize(width, height) => Ok(self.handle_resize(*width, *height)),

            _ => Ok(DispatchResult::NotHandled),
        }
    }

    fn priority(&self) -> u8 {
        10
    } // High priority for responsive navigation

    fn name(&self) -> &'static str {
        "navigation"
    }
}
