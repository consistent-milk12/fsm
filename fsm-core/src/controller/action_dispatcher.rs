//! action_dispatchers/mod.rs
//! Modular action dispatch system with specialized handlers
//!
//! This module provides a clean, modular approach to action dispatching that:
//! - Eliminates the monolithic dispatcher anti-pattern
//! - Prevents deadlocks by releasing locks before async operations
//! - Optimizes performance with specialized handlers
//! - Maintains clean separation of concerns
//! - Provides proper error handling and resource management

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, error, warn};

use crate::controller::event_loop::TaskResult;
use crate::controller::state_coordinator::StateCoordinator;
use crate::controller::{
    Action,
    action_batcher::{ActionBatcher, ActionSource},
};

pub mod command_dispatcher;
pub mod fs_dispatcher;
pub mod navigation_dispatcher;
pub mod search_dispatcher;
pub mod ui_dispatcher;

use command_dispatcher::CommandDispatcher;
use fs_dispatcher::FileOpsDispatcher;
use navigation_dispatcher::NavigationDispatcher;
use search_dispatcher::SearchDispatcher;
use ui_dispatcher::UIControlDispatcher;

/// Result of action processing
#[derive(Debug)]
pub enum DispatchResult {
    /// Continue processing
    Continue,
    /// Terminate application
    Terminate,
    /// Action not handled by this dispatcher
    NotHandled,
}

/// Trait for specialized action dispatchers
pub trait ActionHandler: Send + Sync {
    /// Check if this handler can process the action
    fn can_handle(&self, action: &Action) -> bool;

    /// Process the action and return result
    fn handle(&mut self, action: &Action) -> impl Future<Output = Result<DispatchResult>> + Send;

    /// Handler priority (lower = higher priority)
    fn priority(&self) -> u8 {
        100
    }

    /// Handler name for debugging
    fn name(&self) -> &'static str;
}

/// Main action dispatcher coordinating specialized handlers
pub struct ModularActionDispatcher {
    batcher: ActionBatcher,
    state: Arc<StateCoordinator>,

    #[allow(unused)]
    task_tx: UnboundedSender<TaskResult>,

    // Specialized dispatchers
    navigation: NavigationDispatcher,
    file_ops: FileOpsDispatcher,
    ui_control: UIControlDispatcher,
    search: SearchDispatcher,
    command: CommandDispatcher,
}

impl ModularActionDispatcher {
    pub fn new(state: Arc<StateCoordinator>, task_tx: UnboundedSender<TaskResult>) -> Self {
        debug!("Creating modular action dispatcher with specialized handlers");

        Self {
            batcher: ActionBatcher::new(),
            navigation: NavigationDispatcher::new(state.clone()),
            file_ops: FileOpsDispatcher::new(state.clone(), task_tx.clone()),
            ui_control: UIControlDispatcher::new(state.clone()),
            search: SearchDispatcher::new(state.clone()),
            command: CommandDispatcher::new(state.clone(), task_tx.clone()),
            state,
            task_tx,
        }
    }

    /// Process action through appropriate specialized handler
    pub async fn handle(&mut self, action: Action, source: ActionSource) -> bool {
        // Batch actions for performance optimization
        let maybe_batch: Option<Vec<Action>> = self.batcher.add_action(action, source);

        if let Some(actions) = maybe_batch {
            for action in actions {
                match self.dispatch_action(&action).await {
                    Ok(DispatchResult::Terminate) => {
                        debug!("Termination requested by action handler");
                        return false;
                    }

                    Ok(DispatchResult::Continue) => {
                        // Action processed successfully, continue
                    }

                    Ok(DispatchResult::NotHandled) => {
                        warn!("Unhandled action: {action:?}");
                        self.show_error("Unknown action - please check your input");
                    }

                    Err(e) => {
                        error!("Action dispatch error: {}", e);
                        self.show_error(&format!("Action failed: {}", e));
                    }
                }
            }
        }

        true
    }

    /// Force flush all pending actions
    pub async fn flush(&mut self) -> bool {
        let actions: Vec<Action> = self.batcher.flush_all_batches();

        for action in actions {
            match self.dispatch_action(&action).await {
                Ok(DispatchResult::Terminate) => {
                    debug!("Termination requested during flush");

                    return false;
                }

                Ok(_) => {
                    // Continue processing
                }

                Err(e) => {
                    error!("Action flush error: {}", e);

                    self.show_error(&format!("Action failed: {}", e));
                }
            }
        }

        true
    }

    /// Route action to appropriate specialized handler with fallback
    async fn dispatch_action(&mut self, action: &Action) -> Result<DispatchResult> {
        // Handle quit immediately to prevent further processing
        if matches!(action, Action::Quit) {
            debug!("Processing quit action");

            return Ok(DispatchResult::Terminate);
        }

        // Route to handlers in priority order (performance-critical first)
        // Navigation handler - highest priority for responsive UI
        if self.navigation.can_handle(&action) {
            debug!("Routing to navigation handler");

            return self.navigation.handle(action).await;
        }

        // UI control handler - high priority for immediate feedback
        if self.ui_control.can_handle(&action) {
            debug!("Routing to UI control handler");

            return self.ui_control.handle(action).await;
        }

        // Search handler - medium priority
        if self.search.can_handle(&action) {
            debug!("Routing to search handler");

            return self.search.handle(action).await;
        }

        // File operations handler - medium priority (I/O bound)
        if self.file_ops.can_handle(&action) {
            debug!("Routing to file operations handler");

            return self.file_ops.handle(action).await;
        }

        // Command handler - lowest priority (complex processing)
        if self.command.can_handle(&action) {
            debug!("Routing to command handler");

            return self.command.handle(action).await;
        }

        // No handler found for this action
        warn!("No handler found for action: {:?}", action);

        Ok(DispatchResult::NotHandled)
    }

    /// Show error notification with consistent formatting
    fn show_error(&self, message: &str) {
        let msg = message.to_string();
        self.state
            .update_ui_state(Box::new(move |ui: &mut crate::UIState| {
                ui.show_error(&msg);
            }));
    }

    // Get dispatcher statistics for monitoring
    // pub fn get_stats(&self) -> DispatcherStats {
    //     DispatcherStats {
    //         total_handlers: 5,
    //         batcher_stats: self.batcher.get_stats(),
    //     }
    // }
}

/// Statistics for monitoring dispatcher performance
#[derive(Debug, Clone)]
pub struct DispatcherStats {
    pub total_handlers: usize,
    pub batcher_stats: crate::controller::action_batcher::BatcherStats,
}

/// Integration trait for easier testing and mocking
pub trait DispatcherInterface {
    fn handle(
        &mut self,
        action: Action,
        source: ActionSource,
    ) -> impl std::future::Future<Output = bool> + Send;
    fn flush(&mut self) -> impl std::future::Future<Output = bool> + Send;
}

impl DispatcherInterface for ModularActionDispatcher {
    async fn handle(&mut self, action: Action, source: ActionSource) -> bool {
        self.handle(action, source).await
    }

    async fn flush(&mut self) -> bool {
        self.flush().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{app_state::AppState, fs_state::FSState, ui_state::UIState};
    use std::sync::{Mutex, RwLock};
    use tokio::sync::mpsc;

    fn create_test_coordinator() -> Arc<StateCoordinator> {
        let app_state = Arc::new(Mutex::new(AppState::default()));
        let ui_state = RwLock::new(UIState::default());
        let fs_state = Arc::new(Mutex::new(FSState::default()));

        Arc::new(StateCoordinator::new(app_state, ui_state, fs_state))
    }

    #[tokio::test]
    async fn test_dispatcher_creation() {
        let state = create_test_coordinator();
        let (task_tx, _task_rx) = mpsc::unbounded_channel();

        let dispatcher = ModularActionDispatcher::new(state, task_tx);
        let stats = dispatcher.get_stats();

        assert_eq!(stats.total_handlers, 5);
    }

    #[tokio::test]
    async fn test_quit_action() {
        let state = create_test_coordinator();
        let (task_tx, _task_rx) = mpsc::unbounded_channel();

        let mut dispatcher = ModularActionDispatcher::new(state, task_tx);
        let should_continue = dispatcher
            .handle(Action::Quit, ActionSource::UserInput)
            .await;

        assert!(!should_continue);
    }

    #[tokio::test]
    async fn test_navigation_action() {
        let state = create_test_coordinator();
        let (task_tx, _task_rx) = mpsc::unbounded_channel();

        let mut dispatcher = ModularActionDispatcher::new(state, task_tx);
        let should_continue = dispatcher
            .handle(Action::MoveSelectionUp, ActionSource::UserInput)
            .await;

        assert!(should_continue);
    }
}
