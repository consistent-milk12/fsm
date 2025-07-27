//! action_dispatchers/navigation.rs
//! High-performance navigation action handling

use anyhow::Result;
use std::sync::atomic::Ordering;
use std::sync::{Arc, MutexGuard};

use crate::controller::Action;
use crate::controller::state_provider::StateProvider;
use crate::model::ui_state::RedrawFlag;
use crate::model::{FSState, PaneState};

use super::{ActionMatcher, ActionPriority, DispatchResult};

/// Specialized dispatcher for navigation actions
#[derive(Clone)]
pub struct NavigationDispatcher {
    state_provider: Arc<dyn StateProvider>,
}

impl NavigationDispatcher {
    pub fn new(state_provider: Arc<dyn StateProvider>) -> Self {
        Self { state_provider }
    }

    /// Fast path for selection movement without async overhead
    fn handle_selection_movement(&self, action: &Action) -> Option<DispatchResult> {
        let movement_result = {
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
                    let len = pane.entries.len();
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
            self.state_provider.request_redraw(RedrawFlag::Main);
        }

        Some(DispatchResult::Continue)
    }

    /// Handle page navigation with viewport-aware logic
    fn handle_page_navigation(&self, action: &Action) -> Option<DispatchResult> {
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

    /// Handle resize events efficiently
    fn handle_resize(&self, _width: u16, height: u16) -> DispatchResult {
        let new_height = height.saturating_sub(2) as usize;

        {
            let mut fs = self.state_provider.fs_state();
            fs.active_pane_mut()
                .viewport_height
                .store(new_height, Ordering::Relaxed);
        }

        self.state_provider.request_redraw(RedrawFlag::All);
        DispatchResult::Continue
    }

    /// Handle action asynchronously
    pub async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
        // Fast path for selection movements
        if let Some(result) = self.handle_selection_movement(&action) {
            return Ok(result);
        }

        // Fast path for page navigation
        if let Some(result) = self.handle_page_navigation(&action) {
            return Ok(result);
        }

        match action {
            Action::Resize(width, height) => Ok(self.handle_resize(width, height)),
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
        ActionPriority::High // Navigation needs to be responsive
    }

    fn dynamic_priority(&self, action: &Action) -> ActionPriority {
        match action {
            Action::MoveSelectionUp | Action::MoveSelectionDown => ActionPriority::High,
            Action::PageUp | Action::PageDown => ActionPriority::High,
            Action::Resize(_, _) => ActionPriority::Critical,
            _ => self.priority(),
        }
    }

    fn name(&self) -> &'static str {
        "navigation"
    }

    fn can_disable(&self) -> bool {
        false // Navigation is essential
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{app_state::AppState, fs_state::FSState, ui_state::UIState};
    use std::sync::{Mutex, RwLock};

    // Mock StateProvider for testing
    struct MockStateProvider {
        ui_state: Arc<RwLock<UIState>>,
        fs_state: Arc<Mutex<FSState>>,
        app_state: Arc<Mutex<AppState>>,
    }

    impl StateProvider for MockStateProvider {
        fn ui_state(&self) -> Arc<RwLock<UIState>> {
            self.ui_state.clone()
        }

        fn update_ui_state(&self, update: Box<dyn FnOnce(&mut UIState) + Send>) {
            if let Ok(mut ui) = self.ui_state.write() {
                update(&mut ui);
            }
        }

        fn fs_state(&self) -> std::sync::MutexGuard<'_, FSState> {
            self.fs_state.lock().unwrap()
        }

        fn app_state(&self) -> std::sync::MutexGuard<'_, AppState> {
            self.app_state.lock().unwrap()
        }

        fn request_redraw(&self, _flag: RedrawFlag) {}
        fn needs_redraw(&self) -> bool {
            false
        }
        fn clear_redraw(&self) {}
    }

    fn create_test_dispatcher() -> NavigationDispatcher {
        let state_provider = Arc::new(MockStateProvider {
            ui_state: Arc::new(RwLock::new(UIState::default())),
            fs_state: Arc::new(Mutex::new(FSState::default())),
            app_state: Arc::new(Mutex::new(AppState::default())),
        });

        NavigationDispatcher::new(state_provider)
    }

    #[tokio::test]
    async fn test_navigation_up_down() {
        let mut dispatcher = create_test_dispatcher();

        let result = dispatcher.handle(Action::MoveSelectionDown).await;
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), DispatchResult::Continue));
    }

    #[test]
    fn test_can_handle() {
        let dispatcher = create_test_dispatcher();

        assert!(dispatcher.can_handle(&Action::MoveSelectionUp));
        assert!(dispatcher.can_handle(&Action::PageDown));
        assert!(!dispatcher.can_handle(&Action::Quit));
    }
}
