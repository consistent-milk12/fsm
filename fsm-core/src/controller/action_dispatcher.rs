//! Simplified Modular Action Dispatcher
//! Direct action processing without batching complexity

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;
use arc_swap::ArcSwap;
use enum_map::{Enum, EnumMap, enum_map};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, error, instrument, warn};

use crate::controller::Action;
use crate::controller::event_loop::TaskResult;
use crate::controller::state_provider::StateProvider;
use crate::model::ui_state::RedrawFlag;

pub mod clipboard_dispatcher;
pub mod command_dispatcher;
pub mod fs_dispatcher;
pub mod navigation_dispatcher;
pub mod search_dispatcher;
pub mod ui_dispatcher;

use clipboard_dispatcher::ClipboardDispatcher;
use command_dispatcher::CommandDispatcher;
use fs_dispatcher::FileOpsDispatcher;
use navigation_dispatcher::NavigationDispatcher;
use search_dispatcher::SearchDispatcher;
use ui_dispatcher::UIControlDispatcher;

/// Action processing priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Enum)]
#[repr(u8)]
pub enum ActionPriority {
    Critical = 0,
    High = 1,
    Normal = 2,
    Low = 3,
}

/// Result of action processing
#[derive(Debug)]
pub enum DispatchResult {
    Continue,
    Terminate,
    NotHandled,
}

/// Action source for routing decisions
#[derive(Debug, Clone, Copy)]
pub enum ActionSource {
    UserInput,
    Background,
    System,
}

/// Trait for checking if handler can process action
pub trait ActionMatcher: Send + Sync {
    fn can_handle(&self, action: &Action) -> bool;
    fn priority(&self) -> ActionPriority {
        ActionPriority::Normal
    }
    fn name(&self) -> &'static str;
}

/// Concrete handler enum for type-safe dispatch
#[derive(Clone)]
pub enum ActionHandler {
    Navigation(NavigationDispatcher),
    FileOps(FileOpsDispatcher),
    UIControl(UIControlDispatcher),
    Search(SearchDispatcher),
    Command(CommandDispatcher),
    ClipBoard(ClipboardDispatcher),
}

impl ActionHandler {
    pub async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
        match self {
            ActionHandler::Navigation(h) => h.handle(action).await,
            ActionHandler::FileOps(h) => h.handle(action).await,
            ActionHandler::UIControl(h) => h.handle(action).await,
            ActionHandler::Search(h) => h.handle(action).await,
            ActionHandler::Command(h) => h.handle(action).await,
            ActionHandler::ClipBoard(h) => h.handle(action).await,
        }
    }
}

impl ActionMatcher for ActionHandler {
    fn can_handle(&self, action: &Action) -> bool {
        match self {
            ActionHandler::Navigation(h) => h.can_handle(action),
            ActionHandler::FileOps(h) => h.can_handle(action),
            ActionHandler::UIControl(h) => h.can_handle(action),
            ActionHandler::Search(h) => h.can_handle(action),
            ActionHandler::Command(h) => h.can_handle(action),
            ActionHandler::ClipBoard(h) => h.can_handle(action),
        }
    }

    fn priority(&self) -> ActionPriority {
        match self {
            ActionHandler::Navigation(h) => h.priority(),
            ActionHandler::FileOps(h) => h.priority(),
            ActionHandler::UIControl(h) => h.priority(),
            ActionHandler::Search(h) => h.priority(),
            ActionHandler::Command(h) => h.priority(),
            ActionHandler::ClipBoard(h) => h.priority(),
        }
    }

    fn name(&self) -> &'static str {
        match self {
            ActionHandler::Navigation(h) => h.name(),
            ActionHandler::FileOps(h) => h.name(),
            ActionHandler::UIControl(h) => h.name(),
            ActionHandler::Search(h) => h.name(),
            ActionHandler::Command(h) => h.name(),
            ActionHandler::ClipBoard(h) => h.name(),
        }
    }
}

/// Handler entry with metrics
#[derive(Clone)]
struct HandlerEntry {
    handler: ActionHandler,
    handler_type: HandlerType,
    is_enabled: bool,
    actions_processed: AtomicU64,
    errors_count: AtomicU64,
}

/// Handler type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HandlerType {
    Navigation,
    FileOps,
    UIControl,
    Search,
    Command,
    Clipboard,
}

/// Simplified action dispatcher
pub struct ModularActionDispatcher {
    state_provider: Arc<dyn StateProvider>,
    task_tx: UnboundedSender<TaskResult>,
    handlers: ArcSwap<Vec<HandlerEntry>>,
    metrics: DispatcherMetrics,
}

/// Performance metrics
#[derive(Debug)]
struct DispatcherMetrics {
    total_actions: AtomicU64,
    priority_counts: EnumMap<ActionPriority, AtomicU64>,
}

impl Default for DispatcherMetrics {
    fn default() -> Self {
        Self {
            total_actions: AtomicU64::new(0),
            priority_counts: enum_map! {
                ActionPriority::Critical => AtomicU64::new(0),
                ActionPriority::High => AtomicU64::new(0),
                ActionPriority::Normal => AtomicU64::new(0),
                ActionPriority::Low => AtomicU64::new(0),
            },
        }
    }
}

impl ModularActionDispatcher {
    pub fn new(
        state_provider: Arc<dyn StateProvider>,
        task_tx: UnboundedSender<TaskResult>,
    ) -> Self {
        debug!("Creating simplified action dispatcher");

        let mut dispatcher = Self {
            state_provider: state_provider.clone(),
            task_tx: task_tx.clone(),
            handlers: ArcSwap::from_pointee(Vec::new()),
            metrics: DispatcherMetrics::default(),
        };

        dispatcher.register_default_handlers();
        dispatcher
    }

    fn register_default_handlers(&mut self) {
        self.register_handler(
            ActionHandler::Navigation(NavigationDispatcher::new(self.state_provider.clone())),
            HandlerType::Navigation,
        );

        self.register_handler(
            ActionHandler::UIControl(UIControlDispatcher::new(self.state_provider.clone())),
            HandlerType::UIControl,
        );

        self.register_handler(
            ActionHandler::Search(SearchDispatcher::new(self.state_provider.clone())),
            HandlerType::Search,
        );

        self.register_handler(
            ActionHandler::FileOps(FileOpsDispatcher::new(
                self.state_provider.clone(),
                self.task_tx.clone(),
            )),
            HandlerType::FileOps,
        );

        self.register_handler(
            ActionHandler::Command(CommandDispatcher::new(
                self.state_provider.clone(),
                self.task_tx.clone(),
            )),
            HandlerType::Command,
        );

        self.register_handler(
            ActionHandler::ClipBoard(ClipboardDispatcher::new(self.state_provider.clone())),
            HandlerType::Clipboard,
        );
    }

    fn register_handler(&mut self, handler: ActionHandler, handler_type: HandlerType) {
        let entry = HandlerEntry {
            handler,
            handler_type,
            is_enabled: true,
            actions_processed: AtomicU64::new(0),
            errors_count: AtomicU64::new(0),
        };

        let current = self.handlers.load_full();
        let mut new_handlers = current.as_ref().clone();
        new_handlers.push(entry);

        new_handlers.sort_by_key(|h| h.handler.priority());
        self.handlers.store(Arc::new(new_handlers));

        debug!("Registered {:?} handler", handler_type);
    }

    #[instrument(skip(self))]
    pub async fn handle(&mut self, action: Action, _source: ActionSource) -> bool {
        let priority = self.determine_action_priority(&action);

        self.metrics.total_actions.fetch_add(1, Ordering::Relaxed);
        self.metrics.priority_counts[priority].fetch_add(1, Ordering::Relaxed);

        if priority == ActionPriority::Critical && matches!(action, Action::Quit) {
            debug!("Processing critical quit action");
            return false;
        }

        match self.dispatch_single_action(action).await {
            Ok(DispatchResult::Terminate) => false,
            Ok(DispatchResult::NotHandled) => {
                warn!("Unhandled action");
                true
            }
            Ok(DispatchResult::Continue) => true,
            Err(e) => {
                error!("Action dispatch failed: {}", e);
                self.show_error(&format!("Action failed: {}", e));
                true
            }
        }
    }

    async fn dispatch_single_action(&mut self, action: Action) -> Result<DispatchResult> {
        let handlers = self.handlers.load_full();

        for (idx, entry) in handlers.iter().enumerate() {
            if !entry.is_enabled || !entry.handler.can_handle(&action) {
                continue;
            }

            let handlers_mut = self.handlers.load_full();
            let mut new_handlers =
                Arc::try_unwrap(handlers_mut).unwrap_or_else(|arc| (*arc).clone());

            if let Some(entry_mut) = new_handlers.get_mut(idx) {
                entry_mut.actions_processed.fetch_add(1, Ordering::Relaxed);

                let result = entry_mut.handler.handle(action.clone()).await;

                if result.is_err() {
                    entry_mut.errors_count.fetch_add(1, Ordering::Relaxed);
                }

                self.handlers.store(Arc::new(new_handlers));

                match result {
                    Ok(dispatch_result) => {
                        debug!("Handler {} processed action", entry.handler.name());
                        return Ok(dispatch_result);
                    }
                    Err(e) => {
                        warn!("Handler {} failed: {}", entry.handler.name(), e);
                        continue;
                    }
                }
            }
        }

        Ok(DispatchResult::NotHandled)
    }

    fn determine_action_priority(&self, action: &Action) -> ActionPriority {
        match action {
            Action::Quit => ActionPriority::Critical,
            Action::MoveSelectionUp | Action::MoveSelectionDown => ActionPriority::High,
            Action::Tick => ActionPriority::Low,
            _ => ActionPriority::Normal,
        }
    }

    fn show_error(&self, message: &str) {
        let msg = message.to_string();
        self.state_provider.update_ui_state(Box::new(move |ui| {
            ui.error(&msg);
        }));
        self.state_provider.request_redraw(RedrawFlag::StatusBar);
    }

    pub fn get_stats(&self) -> DispatcherStats {
        let handlers = self.handlers.load_full();
        let handler_stats = handlers
            .iter()
            .map(|entry: &HandlerEntry| HandlerPerformanceStats {
                handler_type: entry.handler_type,
                name: entry.handler.name(),
                is_enabled: entry.is_enabled,
                actions_processed: entry.actions_processed.load(Ordering::Relaxed),
                errors_count: entry.errors_count.load(Ordering::Relaxed),
            })
            .collect();

        DispatcherStats {
            total_handlers: handlers.len(),
            total_actions: self.metrics.total_actions.load(Ordering::Relaxed),
            priority_counts: enum_map! {
                ActionPriority::Critical => self.metrics.priority_counts[ActionPriority::Critical].load(Ordering::Relaxed),
                ActionPriority::High => self.metrics.priority_counts[ActionPriority::High].load(Ordering::Relaxed),
                ActionPriority::Normal => self.metrics.priority_counts[ActionPriority::Normal].load(Ordering::Relaxed),
                ActionPriority::Low => self.metrics.priority_counts[ActionPriority::Low].load(Ordering::Relaxed),
            },
            handler_stats,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DispatcherStats {
    pub total_handlers: usize,
    pub total_actions: u64,
    pub priority_counts: EnumMap<ActionPriority, u64>,
    pub handler_stats: Vec<HandlerPerformanceStats>,
}

#[derive(Debug, Clone)]
pub struct HandlerPerformanceStats {
    pub handler_type: HandlerType,
    pub name: &'static str,
    pub is_enabled: bool,
    pub actions_processed: u64,
    pub errors_count: u64,
}
