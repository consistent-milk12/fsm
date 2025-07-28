// fsm-core/src/controller/action_dispatcher/navigation_dispatcher.rs
// Fast navigation with atomic operations

use anyhow::Result;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::controller::Action;
use crate::controller::state_provider::StateProvider;
use crate::model::ui_state::RedrawFlag;

use super::{ActionMatcher, ActionPriority, DispatchResult};

#[derive(Clone)]
pub struct NavigationDispatcher {
    state_provider: Arc<dyn StateProvider>,
}

impl NavigationDispatcher {
    pub fn new(state_provider: Arc<dyn StateProvider>) -> Self {
        Self { state_provider }
    }

    fn handle_selection(&self, action: &Action) -> Option<DispatchResult> {
        let moved = {
            let mut fs = self.state_provider.fs_state();
            let pane = fs.active_pane_mut();

            match action {
                Action::MoveSelectionUp => pane.move_selection_up(),
                Action::MoveSelectionDown => pane.move_selection_down(),
                Action::SelectFirst => {
                    pane.selected.store(0, Ordering::Relaxed);
                    true
                }
                Action::SelectLast => {
                    let len = pane.entries.len();
                    if len > 0 {
                        pane.selected.store(len - 1, Ordering::Relaxed);
                        true
                    } else {
                        false
                    }
                }
                Action::SelectIndex(index) => {
                    if *index < pane.entries.len() {
                        pane.selected.store(*index, Ordering::Relaxed);
                        true
                    } else {
                        false
                    }
                }
                _ => return None,
            }
        };

        if moved {
            self.state_provider.request_redraw(RedrawFlag::Main);
        }

        Some(DispatchResult::Continue)
    }

    fn handle_page_nav(&self, action: &Action) -> Option<DispatchResult> {
        match action {
            Action::PageUp | Action::PageDown => {
                let lines = {
                    let fs = self.state_provider.fs_state();
                    fs.active_pane().viewport_height.load(Ordering::Relaxed)
                };

                let moved = {
                    let mut fs = self.state_provider.fs_state();
                    let pane = fs.active_pane_mut();

                    match action {
                        Action::PageUp => (0..lines).any(|_| pane.move_selection_up()),
                        Action::PageDown => (0..lines).any(|_| pane.move_selection_down()),
                        _ => unreachable!(),
                    }
                };

                if moved {
                    self.state_provider.request_redraw(RedrawFlag::Main);
                }

                Some(DispatchResult::Continue)
            }
            _ => None,
        }
    }

    pub async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
        if let Some(result) = self.handle_selection(&action) {
            return Ok(result);
        }

        if let Some(result) = self.handle_page_nav(&action) {
            return Ok(result);
        }

        match action {
            Action::Resize(_width, height) => {
                let new_height = height.saturating_sub(2) as usize;
                {
                    let mut fs = self.state_provider.fs_state();
                    fs.active_pane_mut()
                        .viewport_height
                        .store(new_height, Ordering::Relaxed);
                }
                self.state_provider.request_redraw(RedrawFlag::All);
                Ok(DispatchResult::Continue)
            }
            _ => Ok(DispatchResult::NotHandled),
        }
    }
}

impl ActionMatcher for NavigationDispatcher {
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

    fn priority(&self) -> ActionPriority {
        ActionPriority::High
    }

    fn name(&self) -> &'static str {
        "navigation"
    }
}
