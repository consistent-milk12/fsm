//! Action Dispatcher with comprehensive tracing and lock-free performance

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::Result;
use arc_swap::ArcSwap;
use enum_map::{EnumMap, enum_map};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{
    Level, debug, debug_span, error, event, field::Empty, info, instrument, trace, warn,
};

use crate::{
    UIState,
    controller::{Action, event_loop::TaskResult, state_provider::StateProvider},
    error::AppError,
    model::ui_state::RedrawFlag,
};

pub mod clipboard_dispatcher;
pub mod command_dispatcher;
pub mod fs_dispatcher;
pub mod navigation_dispatcher;
pub mod search_dispatcher;
pub mod ui_dispatcher;
pub mod utils;

use clipboard_dispatcher::ClipboardDispatcher;
use command_dispatcher::CommandDispatcher;
use fs_dispatcher::FileOpsDispatcher;
use navigation_dispatcher::NavigationDispatcher;
use search_dispatcher::SearchDispatcher;
use ui_dispatcher::UIControlDispatcher;
pub use utils::*;

/// Action priority levels for dispatch ordering
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, enum_map::Enum)]
pub enum ActionPriority {
    Critical = 0, // Quit, emergency operations
    High = 1,     // Navigation, UI responsiveness
    Normal = 2,   // File operations, clipboard
    Low = 3,      // Background tasks, tick events
}

impl std::fmt::Display for ActionPriority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActionPriority::Critical => write!(f, "Critical"),
            ActionPriority::High => write!(f, "High"),
            ActionPriority::Normal => write!(f, "Normal"),
            ActionPriority::Low => write!(f, "Low"),
        }
    }
}

/// Dispatch operation results
#[derive(Debug, Clone, PartialEq)]
pub enum DispatchResult {
    Continue,   // Keep processing
    Terminate,  // Shutdown application
    NotHandled, // Pass to next handler
}

/// Action source tracking for debugging and metrics
#[derive(Debug, Clone, Copy, PartialEq, Eq, enum_map::Enum)]
pub enum ActionSource {
    Keyboard,
    Timer,
    System,
    Internal,
}

impl std::fmt::Display for ActionSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActionSource::Keyboard => write!(f, "Keyboard"),
            ActionSource::Timer => write!(f, "Timer"),
            ActionSource::System => write!(f, "System"),
            ActionSource::Internal => write!(f, "Internal"),
        }
    }
}

/// Core dispatcher trait with comprehensive tracing integration
pub trait ActionMatcher: Send + Sync {
    fn can_handle(&self, action: &Action) -> bool;

    // Fixed: Add explicit Send bound to the Future
    fn handle(
        &mut self,
        action: Action,
    ) -> impl std::future::Future<Output = Result<DispatchResult>> + Send;

    fn priority(&self) -> ActionPriority;

    fn name(&self) -> &'static str;

    fn dynamic_priority(&self, _action: &Action) -> ActionPriority {
        self.priority()
    }

    fn can_disable(&self) -> bool {
        true
    }
}

/// Type-safe dispatcher enum with comprehensive error handling
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
    #[instrument(
        level = "trace",
        name = "dispatcher_handle",
        fields(
            dispatcher_type = Empty,
            action_type = Empty,
            execution_time = Empty,
        ),
        skip(self, action)
    )]
    pub async fn handle(&mut self, action: Action) -> Result<DispatchResult> {
        let start_time = Instant::now();
        let action_type = std::any::type_name_of_val(&action);

        tracing::Span::current().record("action_type", action_type);

        let result = match self {
            Self::Navigation(d) => {
                tracing::Span::current().record("dispatcher_type", "Navigation");
                d.handle(action).await
            }
            Self::FileOps(d) => {
                tracing::Span::current().record("dispatcher_type", "FileOps");
                d.handle(action).await
            }
            Self::UIControl(d) => {
                tracing::Span::current().record("dispatcher_type", "UIControl");
                d.handle(action).await
            }
            Self::Search(d) => {
                tracing::Span::current().record("dispatcher_type", "Search");
                d.handle(action).await
            }
            Self::Command(d) => {
                tracing::Span::current().record("dispatcher_type", "Command");
                d.handle(action).await
            }
            Self::Clipboard(d) => {
                tracing::Span::current().record("dispatcher_type", "Clipboard");
                d.handle(action).await
            }
        };

        let execution_time = start_time.elapsed();
        tracing::Span::current().record("execution_time", tracing::field::debug(execution_time));

        match &result {
            Ok(dispatch_result) => {
                trace!(
                    dispatcher = self.name(),
                    result = ?dispatch_result,
                    execution_time = ?execution_time,
                    "Dispatcher handled action successfully"
                );
            }
            Err(e) => {
                warn!(
                    dispatcher = self.name(),
                    error = %e,
                    execution_time = ?execution_time,
                    "Dispatcher failed to handle action"
                );
            }
        }

        result
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
        self.handle(action).await
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

/// Dispatcher registry entry with comprehensive metrics tracking
#[derive(Debug)]
struct DispatcherEntry {
    dispatcher: Dispatcher,
    enabled: bool,
    actions_processed: AtomicU64,
    errors: AtomicU64,
    total_execution_time: AtomicU64, // Microseconds
    last_used: AtomicU64,            // Unix timestamp
}

impl Clone for DispatcherEntry {
    fn clone(&self) -> Self {
        Self {
            dispatcher: self.dispatcher.clone(),
            enabled: self.enabled,
            actions_processed: AtomicU64::new(self.actions_processed.load(Ordering::Relaxed)),
            errors: AtomicU64::new(self.errors.load(Ordering::Relaxed)),
            total_execution_time: AtomicU64::new(self.total_execution_time.load(Ordering::Relaxed)),
            last_used: AtomicU64::new(self.last_used.load(Ordering::Relaxed)),
        }
    }
}

impl DispatcherEntry {
    #[instrument(level = "trace", name = "dispatcher_entry_new", skip(dispatcher))]
    fn new(dispatcher: Dispatcher) -> Self {
        let entry = Self {
            dispatcher,
            enabled: true,
            actions_processed: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            total_execution_time: AtomicU64::new(0),
            last_used: AtomicU64::new(0),
        };

        trace!(
            dispatcher_name = entry.dispatcher.name(),
            "Dispatcher entry created"
        );

        entry
    }

    fn record_success(&self, execution_time: Duration) {
        self.actions_processed.fetch_add(1, Ordering::Relaxed);
        self.total_execution_time
            .fetch_add(execution_time.as_micros() as u64, Ordering::Relaxed);
        self.last_used.store(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            Ordering::Relaxed,
        );
    }

    fn record_error(&self, execution_time: Duration) {
        self.errors.fetch_add(1, Ordering::Relaxed);
        self.total_execution_time
            .fetch_add(execution_time.as_micros() as u64, Ordering::Relaxed);
        self.last_used.store(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            Ordering::Relaxed,
        );
    }

    fn average_execution_time(&self) -> Duration {
        let total_actions =
            self.actions_processed.load(Ordering::Relaxed) + self.errors.load(Ordering::Relaxed);

        if total_actions == 0 {
            Duration::ZERO
        } else {
            Duration::from_micros(self.total_execution_time.load(Ordering::Relaxed) / total_actions)
        }
    }
}

/// Performance metrics collection with comprehensive tracking
#[derive(Debug)]
struct Metrics {
    total_actions: AtomicU64,
    priority_counts: EnumMap<ActionPriority, AtomicU64>,
    source_counts: EnumMap<ActionSource, AtomicU64>,
    unhandled_actions: AtomicU64,
    total_execution_time: AtomicU64, // Microseconds
    start_time: std::time::SystemTime,
}

impl Metrics {
    fn new() -> Self {
        Self {
            total_actions: AtomicU64::new(0),
            priority_counts: enum_map! {
                ActionPriority::Critical => AtomicU64::new(0),
                ActionPriority::High => AtomicU64::new(0),
                ActionPriority::Normal => AtomicU64::new(0),
                ActionPriority::Low => AtomicU64::new(0),
            },
            source_counts: enum_map! {
                ActionSource::Keyboard => AtomicU64::new(0),
                ActionSource::Timer => AtomicU64::new(0),
                ActionSource::System => AtomicU64::new(0),
                ActionSource::Internal => AtomicU64::new(0),
            },
            unhandled_actions: AtomicU64::new(0),
            total_execution_time: AtomicU64::new(0),
            start_time: std::time::SystemTime::now(),
        }
    }

    fn record_action(
        &self,
        priority: ActionPriority,
        source: ActionSource,
        execution_time: Duration,
    ) {
        self.total_actions.fetch_add(1, Ordering::Relaxed);
        self.priority_counts[priority].fetch_add(1, Ordering::Relaxed);
        self.source_counts[source].fetch_add(1, Ordering::Relaxed);
        self.total_execution_time
            .fetch_add(execution_time.as_micros() as u64, Ordering::Relaxed);
    }

    fn record_unhandled(&self) {
        self.unhandled_actions.fetch_add(1, Ordering::Relaxed);
    }

    fn uptime(&self) -> Duration {
        self.start_time.elapsed().unwrap_or_default()
    }

    fn actions_per_second(&self) -> f64 {
        let uptime_secs = self.uptime().as_secs_f64();
        if uptime_secs > 0.0 {
            self.total_actions.load(Ordering::Relaxed) as f64 / uptime_secs
        } else {
            0.0
        }
    }

    fn average_execution_time(&self) -> Duration {
        let total = self.total_actions.load(Ordering::Relaxed);
        if total == 0 {
            Duration::ZERO
        } else {
            Duration::from_micros(self.total_execution_time.load(Ordering::Relaxed) / total)
        }
    }
}

/// Main action dispatcher orchestrator with comprehensive tracing
#[derive(Debug)]
pub struct ActionDispatcher {
    state_provider: Arc<dyn StateProvider>,
    dispatchers: ArcSwap<Vec<DispatcherEntry>>,
    metrics: Metrics,
    dispatcher_id: String,
}

impl ActionDispatcher {
    /// Create new dispatcher with comprehensive tracing setup
    #[instrument(
        level = "info",
        name = "action_dispatcher_new",
        fields(
            dispatcher_id = Empty,
            handlers_count = Empty,
            initialization_time = Empty,
        ),
        ret(level = "debug")
    )]
    pub fn new(
        state_provider: Arc<dyn StateProvider>,
        task_tx: UnboundedSender<TaskResult>,
    ) -> Self {
        let init_start = Instant::now();
        let dispatcher_id = nanoid::nanoid!();

        tracing::Span::current().record("dispatcher_id", &dispatcher_id);

        let init_span = debug_span!(
            "dispatcher_initialization",
            dispatcher_id = &dispatcher_id,
            state_provider_addr = format!("{:p}", state_provider.as_ref()),
        );
        let _init_guard = init_span.enter();

        // Create file operations dispatcher with comprehensive setup
        let fs_dispatcher = {
            let fs_span = debug_span!("fileops_dispatcher_creation");
            let _guard = fs_span.enter();
            FileOpsDispatcher::new(state_provider.clone(), task_tx.clone())
        };

        // Initialize all dispatchers in priority order
        let dispatchers = vec![
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

        let handlers_count = dispatchers.len();
        tracing::Span::current().record("handlers_count", handlers_count);

        // Log handler details
        for (idx, entry) in dispatchers.iter().enumerate() {
            event!(
                Level::DEBUG,
                handler_index = idx,
                handler_name = entry.dispatcher.name(),
                handler_priority = %entry.dispatcher.priority(),
                handler_enabled = entry.enabled,
                "Dispatcher handler registered"
            );
        }

        let initialization_time = init_start.elapsed();
        tracing::Span::current().record(
            "initialization_time",
            tracing::field::debug(initialization_time),
        );

        let dispatcher = Self {
            state_provider,
            dispatchers: ArcSwap::from_pointee(dispatchers),
            metrics: Metrics::new(),
            dispatcher_id,
        };

        info!(
            dispatcher_id = &dispatcher.dispatcher_id,
            handlers_count = handlers_count,
            initialization_time = ?initialization_time,
            "Action dispatcher initialized successfully"
        );

        dispatcher
    }

    /// Dispatch action with comprehensive tracing and error handling
    #[instrument(
        level = "debug",
        name = "action_dispatch",
        fields(
            dispatcher_id = %self.dispatcher_id,
            action_type = %std::any::type_name_of_val(&action),
            action_debug = ?action,
            source = %source,
            priority = Empty,
            handler_name = Empty,
            execution_time = Empty,
            result = Empty,
        ),
        skip(self, action)
    )]
    pub async fn dispatch(&self, action: Action, source: ActionSource) -> Result<bool> {
        let dispatch_start = Instant::now();
        let priority = self.determine_priority(&action);

        tracing::Span::current().record("priority", tracing::field::display(&priority));

        debug!(
            action = ?action,
            source = %source,
            priority = %priority,
            "Starting action dispatch"
        );

        // Fast path for quit with comprehensive logging
        if matches!(action, Action::Quit) {
            let execution_time = dispatch_start.elapsed();

            tracing::Span::current()
                .record("handler_name", "built_in_quit")
                .record("execution_time", tracing::field::debug(execution_time))
                .record("result", "terminate");

            self.metrics.record_action(priority, source, execution_time);

            info!(
                action = "quit",
                execution_time = ?execution_time,
                "Quit action processed - terminating application"
            );

            return Ok(false);
        }

        // Store debug info before moving action
        let action_debug = format!("{action:?}");

        // Dispatch to appropriate handler
        let result = match self.dispatch_to_handlers(action, source).await {
            Ok(DispatchResult::Terminate) => {
                tracing::Span::current().record("result", "terminate");
                Ok(false)
            }
            Ok(DispatchResult::Continue) => {
                tracing::Span::current().record("result", "continue");
                Ok(true)
            }
            Ok(DispatchResult::NotHandled) => {
                tracing::Span::current().record("result", "not_handled");
                self.metrics.record_unhandled();

                warn!(
                    action = action_debug,
                    source = %source,
                    priority = %priority,
                    "No handler found for action"
                );

                self.show_warning("Action not supported");
                Ok(true)
            }
            Err(e) => {
                tracing::Span::current().record("result", "error");

                error!(
                    action = action_debug,
                    source = %source,
                    error = %e,
                    error_debug = ?e,
                    "Action dispatch failed"
                );

                // Classify error severity
                match e.downcast_ref::<AppError>() {
                    Some(AppError::StateLock { .. }) | Some(AppError::Terminal(_)) => {
                        self.show_error(&format!("Critical error: {e}"));
                        Err(e)
                    }
                    _ => {
                        self.show_error(&format!("Action failed: {e}"));
                        Ok(true) // Continue on non-critical errors
                    }
                }
            }
        };

        let execution_time = dispatch_start.elapsed();
        tracing::Span::current().record("execution_time", tracing::field::debug(execution_time));

        // Record metrics
        self.metrics.record_action(priority, source, execution_time);

        // Log final result
        match &result {
            Ok(should_continue) => {
                debug!(
                    should_continue = should_continue,
                    execution_time = ?execution_time,
                    priority = %priority,
                    "Action dispatch completed successfully"
                );
            }
            Err(e) => {
                error!(
                    error = %e,
                    execution_time = ?execution_time,
                    priority = %priority,
                    "Action dispatch completed with error"
                );
            }
        }

        result
    }

    /// Route action to best matching dispatcher
    #[instrument(
        level = "trace",
        name = "dispatch_to_handlers",
        fields(
            available_handlers = Empty,
            selected_handler = Empty,
            handler_priority = Empty,
        ),
        skip(self, action)
    )]
    async fn dispatch_to_handlers(
        &self,
        action: Action,
        source: ActionSource,
    ) -> Result<DispatchResult> {
        let dispatchers = self.dispatchers.load();

        // Find all capable handlers with their priorities
        let capable_handlers: Vec<_> = dispatchers
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.enabled && entry.dispatcher.can_handle(&action))
            .map(|(idx, entry)| {
                let dynamic_priority = entry.dispatcher.dynamic_priority(&action);
                (idx, entry, dynamic_priority)
            })
            .collect();

        tracing::Span::current().record("available_handlers", capable_handlers.len());

        if capable_handlers.is_empty() {
            return Ok(DispatchResult::NotHandled);
        }

        // Select best handler (lowest priority value = highest priority)
        let (_handler_idx, entry, selected_priority) = capable_handlers
            .into_iter()
            .min_by_key(|(_, _, priority)| *priority)
            .unwrap();

        let handler_name = entry.dispatcher.name();

        tracing::Span::current()
            .record("selected_handler", handler_name)
            .record(
                "handler_priority",
                tracing::field::display(&selected_priority),
            );

        trace!(
            handler = handler_name,
            priority = %selected_priority,
            source = %source,
            "Selected handler for action"
        );

        // Execute handler with timing
        let execution_start = Instant::now();
        let mut dispatcher = entry.dispatcher.clone();

        let handler_result = {
            let handler_span = debug_span!(
                "handler_execution",
                handler = handler_name,
                action_type = std::any::type_name_of_val(&action),
            );
            let _guard = handler_span.enter();

            dispatcher.handle(action).await
        };

        let execution_time = execution_start.elapsed();

        // Record metrics and handle result
        match handler_result {
            Ok(result) => {
                entry.record_success(execution_time);

                debug!(
                    handler = handler_name,
                    result = ?result,
                    execution_time = ?execution_time,
                    "Handler executed successfully"
                );

                Ok(result)
            }
            Err(e) => {
                entry.record_error(execution_time);

                warn!(
                    handler = handler_name,
                    error = %e,
                    execution_time = ?execution_time,
                    "Handler execution failed"
                );

                Err(e)
            }
        }
    }

    /// Determine action priority with context awareness
    #[instrument(level = "trace", skip(self))]
    fn determine_priority(&self, action: &Action) -> ActionPriority {
        let priority = match action {
            Action::Quit => ActionPriority::Critical,
            Action::Resize(_, _) => ActionPriority::Critical,

            Action::MoveSelectionUp
            | Action::MoveSelectionDown
            | Action::PageUp
            | Action::PageDown
            | Action::SelectFirst
            | Action::SelectLast => ActionPriority::High,

            Action::CloseOverlay | Action::ToggleHelp | Action::ToggleClipboardOverlay => {
                ActionPriority::High
            }

            Action::Tick => ActionPriority::Low,

            _ => ActionPriority::Normal,
        };

        trace!(
            action_type = std::any::type_name_of_val(action),
            priority = %priority,
            "Action priority determined"
        );

        priority
    }

    /// Display error message to user
    fn show_error(&self, message: &str) {
        let msg = message.to_string();
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.error(&msg);
            }));
        self.state_provider.request_redraw(RedrawFlag::StatusBar);
    }

    /// Display warning message to user
    fn show_warning(&self, message: &str) {
        let msg = message.to_string();
        self.state_provider
            .update_ui_state(Box::new(move |ui: &mut UIState| {
                ui.error(&msg);
            }));
        self.state_provider.request_redraw(RedrawFlag::StatusBar);
    }

    /// Get comprehensive performance statistics
    #[instrument(level = "trace", name = "get_dispatcher_stats")]
    pub fn stats(&self) -> DispatcherStats {
        let dispatchers = self.dispatchers.load();

        let handler_stats: Vec<HandlerStats> = dispatchers
            .iter()
            .map(|entry| HandlerStats {
                name: entry.dispatcher.name(),
                enabled: entry.enabled,
                actions_processed: entry.actions_processed.load(Ordering::Relaxed),
                errors: entry.errors.load(Ordering::Relaxed),
                priority: entry.dispatcher.priority(),
                average_execution_time: entry.average_execution_time(),
                last_used: entry.last_used.load(Ordering::Relaxed),
            })
            .collect();

        let stats = DispatcherStats {
            total_actions: self.metrics.total_actions.load(Ordering::Relaxed),
            unhandled_actions: self.metrics.unhandled_actions.load(Ordering::Relaxed),
            priority_distribution: enum_map! {
                ActionPriority::Critical => self.metrics.priority_counts[ActionPriority::Critical].load(Ordering::Relaxed),
                ActionPriority::High => self.metrics.priority_counts[ActionPriority::High].load(Ordering::Relaxed),
                ActionPriority::Normal => self.metrics.priority_counts[ActionPriority::Normal].load(Ordering::Relaxed),
                ActionPriority::Low => self.metrics.priority_counts[ActionPriority::Low].load(Ordering::Relaxed),
            },
            // Note: source_distribution not included in DispatcherStats struct
            handlers: handler_stats,
            uptime: self.metrics.uptime(),
            actions_per_second: self.metrics.actions_per_second(),
            average_execution_time: self.metrics.average_execution_time(),
        };

        trace!(
            total_actions = stats.total_actions,
            handlers_count = stats.handlers.len(),
            "Dispatcher statistics collected"
        );

        stats
    }

    /// Enable/disable specific handler with tracing
    #[instrument(level = "debug", name = "set_handler_enabled")]
    pub fn set_handler_enabled(&self, name: &str, enabled: bool) {
        let current = self.dispatchers.load_full();
        let mut new_dispatchers = (*current).clone();

        let mut handler_found = false;
        for entry in &mut new_dispatchers {
            if entry.dispatcher.name() == name && entry.dispatcher.can_disable() {
                entry.enabled = enabled;
                handler_found = true;

                info!(handler = name, enabled = enabled, "Handler state changed");
                break;
            }
        }

        if handler_found {
            self.dispatchers.store(Arc::new(new_dispatchers));
        } else {
            warn!(
                handler = name,
                enabled = enabled,
                "Handler not found or cannot be disabled"
            );
        }
    }

    /// Get dispatcher ID for tracing correlation
    pub fn id(&self) -> &str {
        &self.dispatcher_id
    }
}

/// Comprehensive performance statistics
#[derive(Debug, Clone)]
pub struct DispatcherStats {
    pub total_actions: u64,
    pub unhandled_actions: u64,
    pub priority_distribution: EnumMap<ActionPriority, u64>,
    pub handlers: Vec<HandlerStats>,
    pub uptime: Duration,
    pub actions_per_second: f64,
    pub average_execution_time: Duration,
}

#[derive(Debug, Clone)]
pub struct HandlerStats {
    pub name: &'static str,
    pub enabled: bool,
    pub actions_processed: u64,
    pub errors: u64,
    pub priority: ActionPriority,
    pub average_execution_time: Duration,
    pub last_used: u64, // Unix timestamp
}

impl DispatcherStats {
    /// Calculate overall error rate as percentage
    pub fn error_rate(&self) -> f64 {
        if self.total_actions == 0 {
            0.0
        } else {
            let total_errors: u64 = self.handlers.iter().map(|h| h.errors).sum();
            (total_errors as f64 / self.total_actions as f64) * 100.0
        }
    }

    /// Get most active handler by actions processed
    pub fn most_active_handler(&self) -> Option<&HandlerStats> {
        self.handlers.iter().max_by_key(|h| h.actions_processed)
    }

    /// Get handler with highest error rate
    pub fn most_error_prone_handler(&self) -> Option<&HandlerStats> {
        self.handlers
            .iter()
            .filter(|h| h.actions_processed > 0)
            .max_by(|a, b| {
                let error_rate_a = a.errors as f64 / (a.actions_processed + a.errors) as f64;
                let error_rate_b = b.errors as f64 / (b.actions_processed + b.errors) as f64;
                error_rate_a
                    .partial_cmp(&error_rate_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /// Check if system is responsive (high priority actions processed efficiently)
    pub fn is_responsive(&self) -> bool {
        let high_priority = self.priority_distribution[ActionPriority::High];
        let critical_priority = self.priority_distribution[ActionPriority::Critical];

        // System is responsive if:
        // 1. Critical + high priority < 90% of total actions
        // 2. Average execution time is reasonable
        // 3. Error rate is low
        let priority_threshold = (self.total_actions as f64 * 0.9) as u64;
        let priority_responsive = (high_priority + critical_priority) < priority_threshold;

        let timing_responsive = self.average_execution_time < Duration::from_millis(50);
        let error_responsive = self.error_rate() < 5.0;

        priority_responsive && timing_responsive && error_responsive
    }

    /// Get priority distribution as percentages
    pub fn priority_percentages(&self) -> EnumMap<ActionPriority, f64> {
        let total = self.total_actions as f64;
        if total == 0.0 {
            enum_map! {
                ActionPriority::Critical => 0.0,
                ActionPriority::High => 0.0,
                ActionPriority::Normal => 0.0,
                ActionPriority::Low => 0.0,
            }
        } else {
            enum_map! {
                ActionPriority::Critical => (self.priority_distribution[ActionPriority::Critical] as f64 / total) * 100.0,
                ActionPriority::High => (self.priority_distribution[ActionPriority::High] as f64 / total) * 100.0,
                ActionPriority::Normal => (self.priority_distribution[ActionPriority::Normal] as f64 / total) * 100.0,
                ActionPriority::Low => (self.priority_distribution[ActionPriority::Low] as f64 / total) * 100.0,
            }
        }
    }

    /// Generate performance summary for debugging
    pub fn performance_summary(&self) -> String {
        format!(
            "Dispatcher Performance Summary:\n\
             Total Actions: {}\n\
             Actions/sec: {:.2}\n\
             Avg Execution: {:?}\n\
             Error Rate: {:.2}%\n\
             Unhandled: {}\n\
             Uptime: {:?}\n\
             Responsive: {}",
            self.total_actions,
            self.actions_per_second,
            self.average_execution_time,
            self.error_rate(),
            self.unhandled_actions,
            self.uptime,
            self.is_responsive()
        )
    }
}

impl HandlerStats {
    /// Calculate error rate for this specific handler
    pub fn error_rate(&self) -> f64 {
        let total = self.actions_processed + self.errors;
        if total == 0 {
            0.0
        } else {
            (self.errors as f64 / total as f64) * 100.0
        }
    }

    /// Check if handler has been used recently (within last hour)
    pub fn recently_used(&self) -> bool {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        current_time.saturating_sub(self.last_used) < 3600 // 1 hour
    }

    /// Calculate actions per second for this handler
    pub fn actions_per_second(&self, uptime: Duration) -> f64 {
        let uptime_secs = uptime.as_secs_f64();
        if uptime_secs > 0.0 {
            self.actions_processed as f64 / uptime_secs
        } else {
            0.0
        }
    }
}
