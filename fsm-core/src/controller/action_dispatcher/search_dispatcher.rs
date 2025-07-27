//! action_dispatchers/search.rs
//! Optimized search operations with minimal allocations

use anyhow::Result;
use std::borrow::Cow;
use std::iter::Peekable;
use std::str::Chars;
use std::sync::Arc;

use crate::controller::Action;
use crate::controller::state_provider::StateProvider;
use crate::fs::object_info::ObjectInfo;
use crate::model::ui_state::{RedrawFlag, UIState};

use super::{ActionMatcher, ActionPriority, DispatchResult};

/// Search dispatcher with performance optimizations
#[derive(Clone)]
pub struct SearchDispatcher {
    state_provider: Arc<dyn StateProvider>,
}

impl SearchDispatcher {
    pub fn new(state_provider: Arc<dyn StateProvider>) -> Self {
        Self { state_provider }
    }

    /// Perform filename search with minimal allocations
    fn perform_filename_search(&self, query: &str) -> Vec<ObjectInfo> {
        if query.is_empty() {
            return Vec::new();
        }

        let fs = self.state_provider.fs_state();
        let entries = &fs.active_pane().entries;

        // Use Cow to avoid unnecessary allocations for case-insensitive search
        let query_cow = if query.chars().any(|c| c.is_uppercase()) {
            Cow::Borrowed(query)
        } else {
            Cow::Owned(query.to_lowercase())
        };

        let has_wildcards = query.contains('*') || query.contains('?');

        entries
            .iter()
            .filter(|entry| {
                let name_cow = if query_cow.chars().any(|c| c.is_lowercase()) {
                    Cow::Owned(entry.name.to_lowercase())
                } else {
                    Cow::Borrowed(&entry.name)
                };

                if has_wildcards {
                    self.wildcard_match(&query_cow, &name_cow)
                } else {
                    name_cow.contains(query_cow.as_ref())
                }
            })
            .cloned()
            .collect()
    }

    /// Optimized wildcard matching using iterative approach
    fn wildcard_match(&self, pattern: &str, text: &str) -> bool {
        let mut p_chars = pattern.chars().peekable();
        let mut t_chars = text.chars().peekable();

        while let Some(p_char) = p_chars.peek() {
            match p_char {
                '*' => {
                    p_chars.next(); // consume '*'

                    // Handle consecutive '*' characters
                    while p_chars.peek() == Some(&'*') {
                        p_chars.next();
                    }

                    // If '*' is at the end, match everything
                    if p_chars.peek().is_none() {
                        return true;
                    }

                    // Try to match the rest of the pattern
                    let remaining_pattern: String = p_chars.collect();

                    while t_chars.peek().is_some() {
                        let remaining_text: String = t_chars.clone().collect();

                        if self.wildcard_match(&remaining_pattern, &remaining_text) {
                            return true;
                        }

                        t_chars.next();
                    }

                    return false;
                }

                '?' => {
                    p_chars.next();
                    if t_chars.next().is_none() {
                        return false;
                    }
                }

                c => {
                    let expected = *c;
                    p_chars.next();

                    if t_chars.next() != Some(expected) {
                        return false;
                    }
                }
            }
        }

        // Both iterators should be exhausted for a complete match
        t_chars.peek().is_none()
    }

    /// Handle search with result caching
    fn handle_filename_search(&self, query: &str) -> DispatchResult {
        let search_results = self.perform_filename_search(query);
        let query = query.to_string();

        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.set_input(&query);
                ui.filename_search_results = search_results;
                ui.request_redraw(RedrawFlag::Overlay);
            }));

        DispatchResult::Continue
    }

    /// Handle action asynchronously
    pub async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
        match action {
            Action::FileNameSearch(query) => Ok(self.handle_filename_search(&query)),

            Action::ShowFilenameSearchResults(results) => {
                self.state_provider
                    .update_ui_state(Box::new(move |ui: &mut UIState| {
                        ui.filename_search_results = results;
                        ui.request_redraw(RedrawFlag::Overlay);
                    }));

                Ok(DispatchResult::Continue)
            }

            Action::ShowSearchResults(results) => {
                self.state_provider
                    .update_ui_state(Box::new(move |ui: &mut UIState| {
                        ui.filename_search_results = results;
                        ui.request_redraw(RedrawFlag::All);
                    }));

                Ok(DispatchResult::Continue)
            }

            _ => Ok(DispatchResult::NotHandled),
        }
    }
}

impl ActionMatcher for SearchDispatcher {
    fn can_handle(&self, action: &Action) -> bool {
        matches!(
            action,
            Action::FileNameSearch(_)
                | Action::ShowFilenameSearchResults(_)
                | Action::ShowSearchResults(_)
        )
    }

    fn priority(&self) -> ActionPriority {
        ActionPriority::Normal
    }

    fn name(&self) -> &'static str {
        "search"
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

    fn create_test_dispatcher() -> SearchDispatcher {
        let state_provider = Arc::new(MockStateProvider {
            ui_state: Arc::new(RwLock::new(UIState::default())),
            fs_state: Arc::new(Mutex::new(FSState::default())),
            app_state: Arc::new(Mutex::new(AppState::default())),
        });

        SearchDispatcher::new(state_provider)
    }

    #[tokio::test]
    async fn test_filename_search() {
        let mut dispatcher = create_test_dispatcher();

        let result = dispatcher
            .handle(Action::FileNameSearch("test".to_string()))
            .await;
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), DispatchResult::Continue));
    }

    #[test]
    fn test_wildcard_matching() {
        let dispatcher = create_test_dispatcher();

        assert!(dispatcher.wildcard_match("test*", "testing"));
        assert!(dispatcher.wildcard_match("*test", "mytest"));
        assert!(dispatcher.wildcard_match("t?st", "test"));
        assert!(!dispatcher.wildcard_match("test", "testing"));
    }

    #[test]
    fn test_can_handle() {
        let dispatcher = create_test_dispatcher();

        assert!(dispatcher.can_handle(&Action::FileNameSearch("test".to_string())));
        assert!(!dispatcher.can_handle(&Action::Quit));
    }
}
