//! action_dispatchers/search.rs
//! Optimized search operations with minimal allocations

use anyhow::Result;
use std::borrow::Cow;
use std::iter::Peekable;
use std::str::Chars;
use std::sync::{Arc, MutexGuard};

use super::{ActionHandler, DispatchResult};
use crate::UIState;
use crate::controller::{Action, state_coordinator::StateCoordinator};
use crate::fs::object_info::ObjectInfo;
use crate::model::{FSState, RedrawFlag};

/// Search dispatcher with performance optimizations
pub struct SearchDispatcher {
    state: Arc<StateCoordinator>,
}

impl SearchDispatcher {
    pub fn new(state: Arc<StateCoordinator>) -> Self {
        Self { state }
    }

    /// Perform filename search with minimal allocations
    fn perform_filename_search(&self, query: &str) -> Vec<ObjectInfo> {
        if query.is_empty() {
            return Vec::new();
        }

        let fs: MutexGuard<'_, FSState> = self.state.fs_state();
        let entries: &Vec<ObjectInfo> = &fs.active_pane().entries;

        // Use Cow to avoid unnecessary allocations for case-insensitive search
        let query_cow: Cow<'_, str> = if query.chars().any(|c| c.is_uppercase()) {
            Cow::Borrowed(query)
        } else {
            Cow::Owned(query.to_lowercase())
        };

        let has_wildcards: bool = query.contains('*') || query.contains('?');

        entries
            .iter()
            .filter(|entry: &&ObjectInfo| {
                let name_cow: Cow<'_, String> = if query_cow.chars().any(|c: char| c.is_lowercase())
                {
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
        let mut p_chars: Peekable<Chars<'_>> = pattern.chars().peekable();
        let mut t_chars: Peekable<Chars<'_>> = text.chars().peekable();

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
                    let tmp: char = c.clone();

                    p_chars.next();

                    if t_chars.next() != Some(tmp) {
                        return false;
                    }
                }
            }
        }

        // Both iterators should be exhausted for a complete match
        t_chars.peek().is_none()
    }

    /// Handle search with result caching
    fn handle_filename_search(&self, query: &String) -> DispatchResult {
        let search_results: Vec<ObjectInfo> = self.perform_filename_search(&query);

        self.state
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.set_input(query);
                ui.filename_search_results = search_results;
                ui.request_redraw(RedrawFlag::Overlay);
            }));

        DispatchResult::Continue
    }
}

impl ActionHandler for SearchDispatcher {
    fn can_handle(&self, action: &Action) -> bool {
        matches!(
            action,
            Action::FileNameSearch(_)
                | Action::ShowFilenameSearchResults(_)
                | Action::ShowSearchResults(_)
        )
    }

    async fn handle(&mut self, action: &Action) -> Result<DispatchResult> {
        match action {
            Action::FileNameSearch(query) => Ok(self.handle_filename_search(query)),

            Action::ShowFilenameSearchResults(results) => {
                self.state
                    .update_ui_state(Box::new(move |ui: &mut UIState| {
                        ui.filename_search_results = results.to_vec();
                        ui.request_redraw(RedrawFlag::Overlay);
                    }));

                Ok(DispatchResult::Continue)
            }

            Action::ShowSearchResults(results) => {
                self.state
                    .update_ui_state(Box::new(move |ui: &mut UIState| {
                        ui.filename_search_results = results.to_vec();
                        ui.request_redraw(RedrawFlag::All);
                    }));

                Ok(DispatchResult::Continue)
            }

            _ => Ok(DispatchResult::NotHandled),
        }
    }

    fn priority(&self) -> u8 {
        30
    } // Medium priority

    fn name(&self) -> &'static str {
        "search"
    }
}
