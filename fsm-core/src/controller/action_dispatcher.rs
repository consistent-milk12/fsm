//! Simplified Modular Action Dispatcher with proper channel integration

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Result;
use arc_swap::ArcSwap;
use enum_map::{EnumMap, enum_map};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, error, instrument, warn};

use crate::UIState;
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

pub mod utils;
pub use utils::*;

use clipboard_dispatcher::ClipboardDispatcher;
use command_dispatcher::CommandDispatcher;
use fs_dispatcher::FileOpsDispatcher;
use navigation_dispatcher::NavigationDispatcher;
use search_dispatcher::SearchDispatcher;
use ui_dispatcher::UIControlDispatcher;

/// Action priority levels for dispatch ordering
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, enum_map::Enum)]
pub enum ActionPriority {
    Critical = 0, // Quit, emergency operations
    High = 1,     // Navigation, UI responsiveness
    Normal = 2,   // File operations, clipboard
    Low = 3,      // Background tasks, tick events
}

/// Dispatch operation results
#[derive(Debug, Clone, PartialEq)]
pub enum DispatchResult {
    Continue,   // Keep processing
    Terminate,  // Shutdown application
    NotHandled, // Pass to next handler
}

/// Action source tracking for debugging
#[derive(Debug, Clone, Copy)]
pub enum ActionSource {
    Keyboard,
    Timer,
    System,
    Internal,
}

/// Core dispatcher trait - matches utils.rs definition
pub trait ActionMatcher: Send + Sync {
    fn can_handle(&self, action: &Action) -> bool;

    fn handle(&mut self, action: Action) -> impl Future<Output = Result<DispatchResult>>;

    fn priority(&self) -> ActionPriority;

    fn name(&self) -> &'static str;

    fn dynamic_priority(&self, _action: &Action) -> ActionPriority {
        self.priority()
    }

    fn can_disable(&self) -> bool {
        true
    }
}

/// Type-safe dispatcher enum
#[derive(Clone, Debug)]
pub enum Dispatcher {
    Navigation(NavigationDispatcher),

    FileOps(FileOpsDispatcher),

    UIControl(UIControlDispatcher),

    Search(SearchDispatcher),

    Command(CommandDispatcher),

    Clipboard(ClipboardDispatcher),
}

impl Dispatcher {
    pub async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
        match self {
            Self::Navigation(d) => d.handle(action).await,

            Self::FileOps(d) => d.handle(action).await,

            Self::UIControl(d) => d.handle(action).await,

            Self::Search(d) => d.handle(action).await,

            Self::Command(d) => d.handle(action).await,

            Self::Clipboard(d) => d.handle(action).await,
        }
    }
}

impl ActionMatcher for Dispatcher {
    fn can_handle(&self, action: &Action) -> bool {
        match self {
            Self::Navigation(d) => d.can_handle(action),

            Self::FileOps(d) => d.can_handle(action),

            Self::UIControl(d) => d.can_handle(action),

            Self::Search(d) => d.can_handle(action),

            Self::Command(d) => d.can_handle(action),

            Self::Clipboard(d) => d.can_handle(action),
        }
    }

    async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
        match self {
            Self::Navigation(d) => d.handle(action).await,

            Self::FileOps(d) => d.handle(action).await,

            Self::UIControl(d) => d.handle(action).await,

            Self::Search(d) => d.handle(action).await,

            Self::Command(d) => d.handle(action).await,

            Self::Clipboard(d) => d.handle(action).await,
        }
    }

    fn priority(&self) -> ActionPriority {
        match self {
            Self::Navigation(d) => d.priority(),

            Self::FileOps(d) => d.priority(),

            Self::UIControl(d) => d.priority(),

            Self::Search(d) => d.priority(),

            Self::Command(d) => d.priority(),

            Self::Clipboard(d) => d.priority(),
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Navigation(d) => d.name(),

            Self::FileOps(d) => d.name(),

            Self::UIControl(d) => d.name(),

            Self::Search(d) => d.name(),

            Self::Command(d) => d.name(),

            Self::Clipboard(d) => d.name(),
        }
    }

    fn dynamic_priority(&self, action: &Action) -> ActionPriority {
        match self {
            Self::Navigation(d) => d.dynamic_priority(action),

            Self::FileOps(d) => d.dynamic_priority(action),

            Self::UIControl(d) => d.dynamic_priority(action),

            Self::Search(d) => d.dynamic_priority(action),

            Self::Command(d) => d.dynamic_priority(action),

            Self::Clipboard(d) => d.dynamic_priority(action),
        }
    }

    fn can_disable(&self) -> bool {
        match self {
            Self::Navigation(d) => d.can_disable(),

            Self::FileOps(d) => d.can_disable(),

            Self::UIControl(d) => d.can_disable(),

            Self::Search(d) => d.can_disable(),

            Self::Command(d) => d.can_disable(),

            Self::Clipboard(d) => d.can_disable(),
        }
    }
}

/// Dispatcher registry entry with metrics
#[derive(Debug)]
struct DispatcherEntry {
    dispatcher: Dispatcher,

    enabled: bool,

    actions_processed: AtomicU64,

    errors: AtomicU64,
}

impl Clone for DispatcherEntry {
    fn clone(&self) -> Self {
        Self {
            dispatcher: self.dispatcher.clone(),
            enabled: self.enabled,
            actions_processed: AtomicU64::new(self.actions_processed.load(Ordering::Relaxed)),
            errors: AtomicU64::new(self.errors.load(Ordering::Relaxed)),
        }
    }
}

impl DispatcherEntry {
    fn new(dispatcher: Dispatcher) -> Self {
        Self {
            dispatcher,
            enabled: true,
            actions_processed: AtomicU64::new(0),
            errors: AtomicU64::new(0),
        }
    }

    fn record_success(&self) {
        self.actions_processed.fetch_add(1, Ordering::Relaxed);
    }

    fn record_error(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }
}

/// Performance metrics collection
#[derive(Debug, Default)]
struct Metrics {
    total_actions: AtomicU64,
    priority_counts: EnumMap<ActionPriority, AtomicU64>,
    unhandled_actions: AtomicU64,
}

impl Metrics {
    fn record_action(&self, priority: ActionPriority) {
        self.total_actions.fetch_add(1, Ordering::Relaxed);

        self.priority_counts[priority].fetch_add(1, Ordering::Relaxed);
    }

    fn record_unhandled(&self) {
        self.unhandled_actions.fetch_add(1, Ordering::Relaxed);
    }
}

/// Main action dispatcher orchestrator
#[derive(Debug)]
pub struct ActionDispatcher {
    state_provider: Arc<dyn StateProvider>,
    dispatchers: ArcSwap<Vec<DispatcherEntry>>,
    metrics: Metrics,
}

impl ActionDispatcher {
    /// Create new dispatcher with default handlers
    pub fn new(
        state_provider: Arc<dyn StateProvider>,
        task_tx: UnboundedSender<TaskResult>,
    ) -> Self {
        let fs_dispatcher = FileOpsDispatcher::new(state_provider.clone(), task_tx.clone());

        debug!("Creating action dispatcher");

        let dispatchers = vec![
            // Order by priority - Critical first
            DispatcherEntry::new(Dispatcher::UIControl(UIControlDispatcher::new(
                state_provider.clone(),
            ))),
            DispatcherEntry::new(Dispatcher::Navigation(NavigationDispatcher::new(
                state_provider.clone(),
            ))),
            DispatcherEntry::new(Dispatcher::Clipboard(ClipboardDispatcher::new(
                state_provider.clone(),
            ))),
            DispatcherEntry::new(Dispatcher::FileOps(fs_dispatcher)),
            DispatcherEntry::new(Dispatcher::Search(SearchDispatcher::new(
                state_provider.clone(),
            ))),
            DispatcherEntry::new(Dispatcher::Command(CommandDispatcher::new(
                state_provider.clone(),
            ))),
        ];

        Self {
            state_provider,
            dispatchers: ArcSwap::from_pointee(dispatchers),
            metrics: Metrics::default(),
        }
    }

    /// Dispatch action to appropriate handler
    #[instrument(level = "trace", skip(self))]
    pub async fn dispatch(&self, action: Action, source: ActionSource) -> bool {
        let priority = self.determine_priority(&action);
        self.metrics.record_action(priority);

        // Fast path for quit
        if matches!(action, Action::Quit) {
            debug!("Processing quit action");
            return false;
        }

        match self.dispatch_to_handlers(action).await {
            Ok(DispatchResult::Terminate) => false,

            Ok(DispatchResult::Continue) => true,

            Ok(DispatchResult::NotHandled) => {
                self.metrics.record_unhandled();
                warn!("No handler for action");
                true
            }

            Err(e) => {
                error!("Dispatch failed: {}", e);

                self.show_error(&format!("Action failed: {}", e));

                true
            }
        }
    }

    /// Route action to matching dispatcher
    async fn dispatch_to_handlers(&self, action: Action) -> Result<DispatchResult> {
        let dispatchers = self.dispatchers.load();

        // Find best matching dispatcher
        let (handler_idx, _priority) = dispatchers
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.enabled && entry.dispatcher.can_handle(&action))
            .map(|(idx, entry)| (idx, entry.dispatcher.dynamic_priority(&action)))
            .min_by_key(|(_, priority)| *priority)
            .ok_or_else(|| anyhow::anyhow!("No handler found"))?;

        // Clone and execute - avoids holding ArcSwap reference
        let mut dispatcher = dispatchers[handler_idx].dispatcher.clone();
        let entry = &dispatchers[handler_idx];

        match dispatcher.handle(action).await {
            Ok(result) => {
                entry.record_success();
                debug!("Handler {} succeeded", dispatcher.name());
                Ok(result)
            }
            Err(e) => {
                entry.record_error();
                warn!("Handler {} failed: {}", dispatcher.name(), e);
                Err(e)
            }
        }
    }

    /// Determine action priority with context awareness
    fn determine_priority(&self, action: &Action) -> ActionPriority {
        match action {
            Action::Quit => ActionPriority::Critical,

            Action::Resize(_, _) => ActionPriority::Critical,

            Action::MoveSelectionUp
            | Action::MoveSelectionDown
            | Action::PageUp
            | Action::PageDown => ActionPriority::High,

            Action::CloseOverlay | Action::ToggleHelp => ActionPriority::High,

            Action::Tick => ActionPriority::Low,

            _ => ActionPriority::Normal,
        }
    }

    /// Display error to user
    fn show_error(&self, message: &str) {
        let msg = message.to_string();
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.error(&msg);
            }));
        self.state_provider.request_redraw(RedrawFlag::StatusBar);
    }

    /// Get performance statistics
    pub fn stats(&self) -> DispatcherStats {
        let dispatchers: arc_swap::Guard<Arc<Vec<DispatcherEntry>>> = self.dispatchers.load();

        let handler_stats: Vec<HandlerStats> = dispatchers
            .iter()
            .map(|entry: &DispatcherEntry| HandlerStats {
                name: entry.dispatcher.name(),
                enabled: entry.enabled,
                actions_processed: entry.actions_processed.load(Ordering::Relaxed),
                errors: entry.errors.load(Ordering::Relaxed),
                priority: entry.dispatcher.priority(),
            })
            .collect();

        DispatcherStats {
            total_actions: self.metrics.total_actions.load(Ordering::Relaxed),
            unhandled_actions: self.metrics.unhandled_actions.load(Ordering::Relaxed),
            priority_distribution: enum_map! {
                ActionPriority::Critical => self.metrics.priority_counts[ActionPriority::Critical].load(Ordering::Relaxed),
                ActionPriority::High => self.metrics.priority_counts[ActionPriority::High].load(Ordering::Relaxed),
                ActionPriority::Normal => self.metrics.priority_counts[ActionPriority::Normal].load(Ordering::Relaxed),
                ActionPriority::Low => self.metrics.priority_counts[ActionPriority::Low].load(Ordering::Relaxed),
            },
            handlers: handler_stats,
        }
    }

    /// Enable/disable specific handler
    pub fn set_handler_enabled(&self, name: &str, enabled: bool) {
        let current: Arc<Vec<DispatcherEntry>> = self.dispatchers.load_full();
        let mut new_dispatchers: Vec<DispatcherEntry> = (*current).clone();

        for entry in &mut new_dispatchers {
            if entry.dispatcher.name() == name && entry.dispatcher.can_disable() {
                entry.enabled = enabled;
                debug!(
                    "Handler {} {}",
                    name,
                    if enabled { "enabled" } else { "disabled" }
                );
                break;
            }
        }

        self.dispatchers.store(Arc::new(new_dispatchers));
    }
}

/// Performance statistics
#[derive(Debug, Clone)]
pub struct DispatcherStats {
    pub total_actions: u64,
    pub unhandled_actions: u64,
    pub priority_distribution: EnumMap<ActionPriority, u64>,
    pub handlers: Vec<HandlerStats>,
}

#[derive(Debug, Clone)]
pub struct HandlerStats {
    pub name: &'static str,
    pub enabled: bool,
    pub actions_processed: u64,
    pub errors: u64,
    pub priority: ActionPriority,
}

impl DispatcherStats {
    /// Calculate error rate as percentage
    pub fn error_rate(&self) -> f64 {
        if self.total_actions == 0 {
            0.0
        } else {
            let total_errors: u64 = self.handlers.iter().map(|h| h.errors).sum();
            (total_errors as f64 / self.total_actions as f64) * 100.0
        }
    }

    /// Get most active handler
    pub fn most_active_handler(&self) -> Option<&HandlerStats> {
        self.handlers.iter().max_by_key(|h| h.actions_processed)
    }

    /// Check if system is responsive (high priority actions processed quickly)
    pub fn is_responsive(&self) -> bool {
        let high_priority = self.priority_distribution[ActionPriority::High];
        let critical_priority = self.priority_distribution[ActionPriority::Critical];

        // Consider responsive if critical + high priority actions < 90% of total
        let responsive_threshold = (self.total_actions as f64 * 0.9) as u64;
        (high_priority + critical_priority) < responsive_threshold
    }
}
