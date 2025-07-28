// fsm-core/src/controller/action_dispatcher/search_dispatcher.rs
// Optimized search with minimal allocations

use anyhow::Result;
use std::sync::Arc;

use crate::controller::Action;
use crate::controller::state_provider::StateProvider;
use crate::fs::object_info::ObjectInfo;
use crate::model::ui_state::{RedrawFlag, UIState};

use super::{ActionMatcher, ActionPriority, DispatchResult};

#[derive(Clone)]
pub struct SearchDispatcher {
    state_provider: Arc<dyn StateProvider>,
}

impl SearchDispatcher {
    pub fn new(state_provider: Arc<dyn StateProvider>) -> Self {
        Self { state_provider }
    }

    fn search_files(&self, query: &str) -> Vec<ObjectInfo> {
        if query.is_empty() {
            return Vec::new();
        }

        let fs = self.state_provider.fs_state();
        let query_lower = query.to_lowercase();

        fs.active_pane()
            .entries
            .iter()
            .filter(|entry| {
                let name_lower = entry.name.to_lowercase();
                if query.contains('*') {
                    self.wildcard_match(&query_lower, &name_lower)
                } else {
                    name_lower.contains(&query_lower)
                }
            })
            .cloned()
            .collect()
    }

    fn wildcard_match(&self, pattern: &str, text: &str) -> bool {
        let parts: Vec<&str> = pattern.split('*').collect();
        if parts.len() == 1 {
            return text.contains(pattern);
        }

        let mut pos = 0;
        for (i, part) in parts.iter().enumerate() {
            if part.is_empty() {
                continue;
            }

            if let Some(found) = text[pos..].find(part) {
                pos += found + part.len();
                if i == 0 && !pattern.starts_with('*') && found != 0 {
                    return false;
                }
            } else {
                return false;
            }
        }

        !pattern.ends_with('*')
            || parts
                .last()
                .map_or(true, |p| p.is_empty() || text.ends_with(p))
    }

    fn handle_filename_search(&self, query: &str) -> DispatchResult {
        let results = self.search_files(query);
        let query = query.to_string();

        {
            let mut fs = self.state_provider.fs_state();
            fs.active_pane_mut().search_results = results;
        }

        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.search_query = Some(query.into());
                ui.request_redraw(RedrawFlag::Overlay);
            }));

        DispatchResult::Continue
    }

    pub async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
        match action {
            Action::FileNameSearch(query) => Ok(self.handle_filename_search(&query)),
            Action::ShowSearchResults(results) => {
                {
                    let mut fs = self.state_provider.fs_state();
                    fs.active_pane_mut().search_results = results;
                }
                self.state_provider
                    .update_ui_state(Box::new(|ui: &mut UIState| {
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
            Action::FileNameSearch(_) | Action::ShowSearchResults(_)
        )
    }

    fn priority(&self) -> ActionPriority {
        ActionPriority::Normal
    }

    fn name(&self) -> &'static str {
        "search"
    }
}
