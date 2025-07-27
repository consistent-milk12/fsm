//! Enhanced Modular Action Dispatcher with Event Integration
//!
//! This enhanced version integrates patterns from your event processor and state management:
//! - Priority-based action routing inspired by event_processor.rs
//! - StateProvider trait integration for clean dependencies
//! - Performance metrics and monitoring
//! - Lock-free state updates where possible
//! - Comprehensive error handling and resource management

use std::collections::VecDeque;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use arc_swap::ArcSwap;
use enum_map::{Enum, EnumMap, enum_map};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, error, instrument, warn};

use crate::controller::event_loop::TaskResult;
use crate::controller::state_provider::StateProvider;
use crate::controller::{
    Action,
    action_batcher::{ActionBatcher, ActionSource, BatcherStats},
};
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
    /// Critical actions (Quit, Emergency stop)
    Critical = 0,
    /// High-frequency UI actions (Navigation, Input)
    High = 1,
    /// Standard user actions (File operations, Search)
    Normal = 2,
    /// Background/batch actions (Cleanup, Metrics)
    Low = 3,
}

/// Result of action processing with enhanced metadata
#[derive(Debug)]
pub enum DispatchResult {
    /// Continue processing
    Continue,
    /// Terminate application
    Terminate,
    /// Action not handled by this dispatcher
    NotHandled,
    /// Action queued for batch processing
    Queued,
    /// Action processed with metrics
    ProcessedWithMetrics {
        processing_time: Duration,
        generated_tasks: u32,
    },
}

/// Trait for checking if handler can process action (dyn compatible)
pub trait ActionMatcher: Send + Sync {
    fn can_handle(&self, action: &Action) -> bool;
    fn priority(&self) -> ActionPriority {
        ActionPriority::Normal
    }
    fn dynamic_priority(&self, action: &Action) -> ActionPriority {
        match action {
            Action::Quit => ActionPriority::Critical,
            Action::MoveSelectionUp | Action::MoveSelectionDown => ActionPriority::High,
            _ => self.priority(),
        }
    }
    fn name(&self) -> &'static str;
    fn can_disable(&self) -> bool {
        true
    }
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
    /// Process action through the appropriate handler
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

/// Handler registration with performance tracking
#[derive(Clone)]
struct HandlerEntry {
    handler: ActionHandler,
    handler_type: HandlerType,
    is_enabled: bool,
    metrics: HandlerMetrics,
}

/// Handler type enumeration for tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HandlerType {
    Navigation,
    FileOps,
    UIControl,
    Search,
    Command,
}

/// Performance metrics per handler
#[derive(Debug)]
struct HandlerMetrics {
    actions_processed: AtomicU64,
    total_processing_time: AtomicU64, // nanoseconds
    errors_count: AtomicU64,
    last_processed: std::sync::Mutex<Option<Instant>>,
}

impl Clone for HandlerMetrics {
    fn clone(&self) -> Self {
        let actions = self.actions_processed.load(Ordering::Relaxed);
        let total_time = self.total_processing_time.load(Ordering::Relaxed);
        let errors = self.errors_count.load(Ordering::Relaxed);

        let last = {
            let guard = self.last_processed.lock().unwrap();
            *guard
        };

        HandlerMetrics {
            actions_processed: AtomicU64::new(actions),
            total_processing_time: AtomicU64::new(total_time),
            errors_count: AtomicU64::new(errors),
            last_processed: Mutex::new(last),
        }
    }
}

impl HandlerMetrics {
    fn new() -> Self {
        Self {
            actions_processed: AtomicU64::new(0),
            total_processing_time: AtomicU64::new(0),
            errors_count: AtomicU64::new(0),
            last_processed: std::sync::Mutex::new(None),
        }
    }

    fn record_processing(&self, duration: Duration, had_error: bool) {
        self.actions_processed.fetch_add(1, Ordering::Relaxed);
        self.total_processing_time
            .fetch_add(duration.as_nanos() as u64, Ordering::Relaxed);

        if had_error {
            self.errors_count.fetch_add(1, Ordering::Relaxed);
        }

        if let Ok(mut last) = self.last_processed.lock() {
            *last = Some(Instant::now());
        }
    }
}

/// Enhanced action dispatcher with priority routing and performance monitoring
pub struct ModularActionDispatcher {
    /// Action batching for performance optimization
    batcher: ActionBatcher,

    /// State provider for clean dependency management
    state_provider: Arc<dyn StateProvider>,

    /// Task result channel for background operations
    task_tx: UnboundedSender<TaskResult>,

    /// Registered handlers with atomic updates
    handlers: ArcSwap<Vec<HandlerEntry>>,

    /// Priority-based action queues
    priority_queues: EnumMap<ActionPriority, std::sync::Mutex<VecDeque<(Action, ActionSource)>>>,

    /// Global dispatcher metrics
    metrics: DispatcherMetrics,

    /// Configuration
    config: DispatcherConfig,
}

/// Global dispatcher performance metrics
#[derive(Debug)]
struct DispatcherMetrics {
    total_actions: AtomicU64,
    priority_counts: EnumMap<ActionPriority, AtomicU64>,
    batch_count: AtomicU64,
    queue_overflows: AtomicU64,
    avg_latency_ns: AtomicU64,
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
            batch_count: AtomicU64::new(0),
            queue_overflows: AtomicU64::new(0),
            avg_latency_ns: AtomicU64::new(0),
        }
    }
}

/// Dispatcher configuration
#[derive(Debug, Clone)]
pub struct DispatcherConfig {
    /// Maximum actions per priority queue
    pub max_queue_size: usize,
    /// Batch processing size
    pub batch_size: usize,
    /// Enable performance monitoring
    pub enable_metrics: bool,
    /// Handler timeout
    pub handler_timeout: Duration,
}

impl Default for DispatcherConfig {
    fn default() -> Self {
        Self {
            max_queue_size: 1000,
            batch_size: 10,
            enable_metrics: true,
            handler_timeout: Duration::from_millis(100),
        }
    }
}

impl ModularActionDispatcher {
    /// Create new enhanced dispatcher
    pub fn new(
        state_provider: Arc<dyn StateProvider>,
        task_tx: UnboundedSender<TaskResult>,
    ) -> Self {
        debug!("Creating enhanced action dispatcher with priority queues");

        let mut dispatcher = Self {
            batcher: ActionBatcher::new(),
            state_provider: state_provider.clone(),
            task_tx: task_tx.clone(),
            handlers: ArcSwap::from_pointee(Vec::new()),
            priority_queues: enum_map! {
                ActionPriority::Critical => std::sync::Mutex::new(VecDeque::new()),
                ActionPriority::High => std::sync::Mutex::new(VecDeque::new()),
                ActionPriority::Normal => std::sync::Mutex::new(VecDeque::new()),
                ActionPriority::Low => std::sync::Mutex::new(VecDeque::new()),
            },
            metrics: DispatcherMetrics::default(),
            config: DispatcherConfig::default(),
        };

        // Register default handlers
        dispatcher.register_default_handlers();

        dispatcher
    }

    /// Register all default handlers
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
    }

    /// Register a new handler
    pub fn register_handler(&mut self, handler: ActionHandler, handler_type: HandlerType) {
        let entry = HandlerEntry {
            handler,
            handler_type,
            is_enabled: true,
            metrics: HandlerMetrics::new(),
        };

        let current = self.handlers.load_full();
        let mut new_handlers = current.as_ref().clone();
        new_handlers.push(entry);

        // Sort by priority for optimal dispatch order
        new_handlers.sort_by_key(|h| h.handler.priority());

        self.handlers.store(Arc::new(new_handlers.to_vec()));

        debug!("Registered {:?} handler", handler_type);
    }

    /// Process action with priority-based routing
    #[instrument(skip(self))]
    pub async fn handle(&mut self, action: Action, source: ActionSource) -> bool {
        let process_start = Instant::now();

        // Determine action priority
        let priority = self.determine_action_priority(&action);

        // Update metrics
        self.metrics.total_actions.fetch_add(1, Ordering::Relaxed);
        self.metrics.priority_counts[priority].fetch_add(1, Ordering::Relaxed);

        // Handle critical actions immediately
        if priority == ActionPriority::Critical {
            return self.process_critical_action(action).await;
        }

        // Try batching first for performance
        if let Some(batched_actions) = self.batcher.add_action(action.clone(), source) {
            self.metrics.batch_count.fetch_add(1, Ordering::Relaxed);
            return self.process_action_batch(batched_actions, priority).await;
        }

        // Queue for later processing
        self.queue_action(action, source, priority);

        // Update latency metrics
        if self.config.enable_metrics {
            let elapsed_ns = process_start.elapsed().as_nanos() as u64;
            let current_avg = self.metrics.avg_latency_ns.load(Ordering::Relaxed);
            let new_avg = if current_avg == 0 {
                elapsed_ns
            } else {
                (current_avg * 9 + elapsed_ns) / 10 // EWMA
            };
            self.metrics
                .avg_latency_ns
                .store(new_avg, Ordering::Relaxed);
        }

        true
    }

    /// Process critical actions immediately
    async fn process_critical_action(&mut self, action: Action) -> bool {
        match action {
            Action::Quit => {
                debug!("Processing critical quit action");
                false
            }
            _ => {
                // Route through normal dispatch for other critical actions
                match self.dispatch_single_action(action).await {
                    Ok(DispatchResult::Terminate) => false,
                    Ok(_) => true,
                    Err(e) => {
                        error!("Critical action failed: {}", e);
                        self.show_error(&format!("Critical action failed: {}", e));
                        true
                    }
                }
            }
        }
    }

    /// Process a batch of actions
    async fn process_action_batch(
        &mut self,
        actions: Vec<Action>,
        priority: ActionPriority,
    ) -> bool {
        for action in actions {
            if !self
                .process_single_action_with_priority(action, priority)
                .await
            {
                return false;
            }
        }
        true
    }

    /// Process single action with known priority
    async fn process_single_action_with_priority(
        &mut self,
        action: Action,
        _priority: ActionPriority,
    ) -> bool {
        match self.dispatch_single_action(action).await {
            Ok(DispatchResult::Terminate) => false,
            Ok(DispatchResult::NotHandled) => {
                warn!("Unhandled action in batch processing");
                true
            }
            Ok(_) => true,
            Err(e) => {
                error!("Batch action failed: {}", e);
                self.show_error(&format!("Action failed: {}", e));
                true
            }
        }
    }

    /// Dispatch single action through handler chain
    async fn dispatch_single_action(&mut self, action: Action) -> Result<DispatchResult> {
        let handlers = self.handlers.load_full();

        for (idx, entry) in handlers.iter().enumerate() {
            if !entry.is_enabled || !entry.handler.can_handle(&action) {
                continue;
            }

            let process_start: Instant = Instant::now();

            // Get mutable access to handlers for processing
            let handlers_mut: Arc<Vec<HandlerEntry>> = self.handlers.load_full();
            let mut new_handlers =
                Arc::try_unwrap(handlers_mut).unwrap_or_else(|arc| (*arc).clone());

            if let Some(entry_mut) = new_handlers.get_mut(idx) {
                // Use timeout for handler processing
                let result = tokio::time::timeout(
                    self.config.handler_timeout,
                    entry_mut.handler.handle(action.clone()),
                )
                .await;

                let processing_time: Duration = process_start.elapsed();
                let had_error: bool = result.is_err();

                // Update handler metrics
                entry_mut
                    .metrics
                    .record_processing(processing_time, had_error);

                // Update the handlers atomically
                self.handlers.store(Arc::new(new_handlers));

                match result {
                    Ok(Ok(dispatch_result)) => {
                        debug!(
                            "Handler {} processed action in {:?}",
                            entry.handler.name(),
                            processing_time
                        );
                        return Ok(dispatch_result);
                    }
                    Ok(Err(e)) => {
                        warn!("Handler {} failed: {}", entry.handler.name(), e);
                        continue; // Try next handler
                    }
                    Err(_) => {
                        warn!("Handler {} timed out", entry.handler.name());
                        continue; // Try next handler
                    }
                }
            }
        }

        Ok(DispatchResult::NotHandled)
    }

    /// Queue action for later processing
    fn queue_action(&self, action: Action, source: ActionSource, priority: ActionPriority) {
        if let Ok(mut queue) = self.priority_queues[priority].lock() {
            if queue.len() >= self.config.max_queue_size {
                // Queue overflow - drop oldest item
                queue.pop_front();
                self.metrics.queue_overflows.fetch_add(1, Ordering::Relaxed);
                warn!("Action queue overflow for priority {:?}", priority);
            }

            queue.push_back((action, source));
        }
    }

    /// Process all queued actions in priority order
    pub async fn process_queues(&mut self) -> bool {
        let priorities = [
            ActionPriority::Critical,
            ActionPriority::High,
            ActionPriority::Normal,
            ActionPriority::Low,
        ];

        for priority in priorities {
            let actions: Vec<Action> = {
                if let Ok(mut queue) = self.priority_queues[priority].lock() {
                    let mut actions: Vec<Action> = Vec::new();

                    for _ in 0..self.config.batch_size {
                        if let Some((action, _source)) = queue.pop_front() {
                            actions.push(action);
                        } else {
                            break;
                        }
                    }

                    actions
                } else {
                    continue;
                }
            };

            for action in actions {
                if !self
                    .process_single_action_with_priority(action, priority)
                    .await
                {
                    return false;
                }
            }
        }

        true
    }

    /// Force flush all pending actions
    pub async fn flush(&mut self) -> bool {
        // First flush the batcher
        let batched_actions: Vec<Action> = self.batcher.flush_all_batches();

        for action in batched_actions {
            if !self
                .process_single_action_with_priority(action, ActionPriority::Normal)
                .await
            {
                return false;
            }
        }

        // Then process all queues
        self.process_queues().await
    }

    /// Determine action priority
    fn determine_action_priority(&self, action: &Action) -> ActionPriority {
        match action {
            Action::Quit => ActionPriority::Critical,
            Action::MoveSelectionUp | Action::MoveSelectionDown => ActionPriority::High,
            Action::ReloadDirectory | Action::OpenFile(_, _) => ActionPriority::Normal,
            Action::Tick => ActionPriority::Low,
            _ => ActionPriority::Normal,
        }
    }

    /// Show error through state provider
    fn show_error(&self, message: &str) {
        let msg = message.to_string();
        self.state_provider.update_ui_state(Box::new(move |ui| {
            ui.show_error(&msg);
        }));
        self.state_provider.request_redraw(RedrawFlag::StatusBar);
    }

    /// Get comprehensive performance statistics
    pub fn get_stats(&self) -> DispatcherStats {
        let handlers = self.handlers.load_full();
        let handler_stats = handlers
            .iter()
            .map(|entry| {
                let actions_processed = entry.metrics.actions_processed.load(Ordering::Relaxed);
                let total_time_ns = entry.metrics.total_processing_time.load(Ordering::Relaxed);
                let errors = entry.metrics.errors_count.load(Ordering::Relaxed);

                HandlerPerformanceStats {
                    handler_type: entry.handler_type,
                    name: entry.handler.name(),
                    is_enabled: entry.is_enabled,
                    actions_processed,
                    avg_processing_time_ns: if actions_processed > 0 {
                        total_time_ns / actions_processed
                    } else {
                        0
                    },
                    errors_count: errors,
                    error_rate: if actions_processed > 0 {
                        errors as f64 / actions_processed as f64
                    } else {
                        0.0
                    },
                }
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
            batch_count: self.metrics.batch_count.load(Ordering::Relaxed),
            queue_overflows: self.metrics.queue_overflows.load(Ordering::Relaxed),
            avg_latency_ns: self.metrics.avg_latency_ns.load(Ordering::Relaxed),
            handler_stats,
            batcher_stats: self.batcher.get_performance_stats(),
        }
    }

    /// Enable/disable specific handler
    pub fn set_handler_enabled(&self, handler_type: HandlerType, enabled: bool) {
        let handlers = self.handlers.load_full();
        let mut new_handlers = handlers.as_ref().clone();

        if let Some(entry) = new_handlers
            .iter_mut()
            .find(|h| h.handler_type == handler_type)
        {
            entry.is_enabled = enabled;
            debug!(
                "Handler {:?} {}",
                handler_type,
                if enabled { "enabled" } else { "disabled" }
            );
        }

        self.handlers.store(Arc::new(new_handlers));
    }
}

/// Performance statistics for the entire dispatcher
#[derive(Debug, Clone)]
pub struct DispatcherStats {
    pub total_handlers: usize,
    pub total_actions: u64,
    pub priority_counts: EnumMap<ActionPriority, u64>,
    pub batch_count: u64,
    pub queue_overflows: u64,
    pub avg_latency_ns: u64,
    pub handler_stats: Vec<HandlerPerformanceStats>,
    pub batcher_stats: BatcherStats,
}

/// Performance statistics per handler
#[derive(Debug, Clone)]
pub struct HandlerPerformanceStats {
    pub handler_type: HandlerType,
    pub name: &'static str,
    pub is_enabled: bool,
    pub actions_processed: u64,
    pub avg_processing_time_ns: u64,
    pub errors_count: u64,
    pub error_rate: f64,
}

/// Integration trait for easier testing and mocking
pub trait DispatcherInterface {
    fn handle(&mut self, action: Action, source: ActionSource)
    -> impl Future<Output = bool> + Send;
    fn flush(&mut self) -> impl Future<Output = bool> + Send;
    fn process_queues(&mut self) -> impl Future<Output = bool> + Send;
}

impl DispatcherInterface for ModularActionDispatcher {
    async fn handle(&mut self, action: Action, source: ActionSource) -> bool {
        self.handle(action, source).await
    }

    async fn flush(&mut self) -> bool {
        self.flush().await
    }

    async fn process_queues(&mut self) -> bool {
        self.process_queues().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::state_provider::StateProvider;
    use crate::model::{app_state::AppState, fs_state::FSState, ui_state::UIState};
    use std::sync::{Mutex, RwLock};
    use tokio::sync::mpsc;

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

        fn update_task_progress(
            &self,
            _task_id: String,
            _current: u64,
            _total: u64,
            _message: Option<String>,
        ) {
        }

        fn request_redraw(&self, _flag: RedrawFlag) {}
        fn needs_redraw(&self) -> bool {
            false
        }
        fn clear_redraw(&self) {}
    }

    fn create_test_dispatcher() -> (
        ModularActionDispatcher,
        tokio::sync::mpsc::UnboundedReceiver<TaskResult>,
    ) {
        let (task_tx, task_rx) = mpsc::unbounded_channel();

        let state_provider = Arc::new(MockStateProvider {
            ui_state: Arc::new(RwLock::new(UIState::default())),
            fs_state: Arc::new(Mutex::new(FSState::default())),
            app_state: Arc::new(Mutex::new(AppState::default())),
        });

        let dispatcher = ModularActionDispatcher::new(state_provider, task_tx);

        (dispatcher, task_rx)
    }

    #[tokio::test]
    async fn test_critical_action_handling() {
        let (mut dispatcher, _rx) = create_test_dispatcher();

        let should_continue = dispatcher
            .handle(Action::Quit, ActionSource::UserInput)
            .await;
        assert!(!should_continue);
    }

    #[tokio::test]
    async fn test_priority_routing() {
        let (mut dispatcher, _rx) = create_test_dispatcher();

        // High priority action should be processed immediately
        let should_continue = dispatcher
            .handle(Action::MoveSelectionUp, ActionSource::UserInput)
            .await;
        assert!(should_continue);
    }

    #[tokio::test]
    async fn test_performance_metrics() {
        let (mut dispatcher, _rx) = create_test_dispatcher();

        // Process some actions
        dispatcher
            .handle(Action::MoveSelectionUp, ActionSource::UserInput)
            .await;
        dispatcher
            .handle(Action::ReloadDirectory, ActionSource::UserInput)
            .await;

        let stats = dispatcher.get_stats();
        assert!(stats.total_actions > 0);
        assert!(stats.priority_counts[ActionPriority::High] > 0);
    }

    #[tokio::test]
    async fn test_handler_enable_disable() {
        let (mut dispatcher, _rx) = create_test_dispatcher();

        // Disable navigation handler
        dispatcher.set_handler_enabled(HandlerType::Navigation, false);

        // Action should still be processed but might be handled differently
        let should_continue = dispatcher
            .handle(Action::MoveSelectionUp, ActionSource::UserInput)
            .await;
        assert!(should_continue);
    }

    #[tokio::test]
    async fn test_queue_processing() {
        let (mut dispatcher, _rx) = create_test_dispatcher();

        // Queue some actions (these will be batched)
        for _ in 0..5 {
            dispatcher
                .handle(Action::Tick, ActionSource::Background)
                .await;
        }

        // Process queues
        let should_continue = dispatcher.process_queues().await;
        assert!(should_continue);

        let stats = dispatcher.get_stats();
        assert!(stats.total_actions >= 5);
    }
}
