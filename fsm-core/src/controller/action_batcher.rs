//! ActionBatcher: Event optimization with batching for Phase 4.0
//!
//! Optimizes action processing through intelligent batching:
//! - Batch compatible actions for reduced overhead
//! - Priority-based action routing with deadlines
//! - Navigation movement buffering to prevent lag
//! - Atomic operation grouping for consistency
//! - Performance metrics and throttling controls

use crate::controller::actions::Action;

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};
use tracing::{debug, info};

/// Action priority levels for batching optimization
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum ActionPriority {
    Critical = 0, // Emergency/quit actions - immediate execution
    High = 1,     // UI updates, user feedback - <16ms deadline
    Normal = 2,   // File operations, navigation - <50ms deadline
    Low = 3,      // Background tasks, cleanup - <200ms deadline
    Deferred = 4, // Non-critical operations - can be delayed
}

/// Action metadata for batching decisions
#[derive(Debug, Clone)]
pub struct ActionMetadata {
    pub action: Action,
    pub priority: ActionPriority,
    pub timestamp: Instant,
    pub source: ActionSource,
    pub deadline: Option<Instant>,
    pub can_batch: bool,
    pub batch_group: Option<BatchGroup>,
}

/// Source of the action for optimization decisions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ActionSource {
    UserInput = 0,  // Direct user keyboard/mouse input
    FileSystem = 1, // File system events and updates
    Timer = 2,      // Scheduled or periodic actions
    Background = 3, // Background task completion
    System = 4,     // System-level events
}

/// Batch grouping for compatible actions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum BatchGroup {
    Navigation = 0,     // Movement actions (up/down/page)
    Selection = 1,      // Mark/unmark/visual selection
    FileOperations = 2, // Copy/move/delete operations
    UIUpdates = 3,      // Redraw and UI state changes
    Search = 4,         // Search operations and results
    ClipBoard = 5,      // Clipboard operations
}

/// Intelligent action batcher with performance optimization
pub struct ActionBatcher {
    // Pending actions organized by priority
    priority_queues: [VecDeque<ActionMetadata>; 5],

    // Batch configuration
    batch_timeout: Duration,
    max_batch_size: usize,
    last_flush: Instant,

    // Navigation buffering for smooth movement
    navigation_buffer: NavigationBuffer,

    // Batch grouping state
    active_batch_groups: HashMap<BatchGroup, Vec<ActionMetadata>>,

    // Performance tracking
    actions_processed: u64,
    batches_created: u64,
    total_batch_time: Duration,
    optimization_stats: OptimizationStats,
}

/// Navigation movement buffering to prevent lag
#[derive(Debug)]
struct NavigationBuffer {
    movements: VecDeque<NavigationMovement>,
    last_movement_time: Instant,
    movement_timeout: Duration,
    max_buffer_size: usize,
}

/// Navigation movement types for optimization
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NavigationMovement {
    Up(u32),
    Down(u32),
    Left(u32),
    Right(u32),
    PageUp(u32),
    PageDown(u32),
    Home,
    End,
}

/// Batch optimization statistics
#[derive(Debug, Default)]
struct OptimizationStats {
    navigation_movements_combined: u64,
    ui_updates_batched: u64,
    file_operations_grouped: u64,
    total_actions_reduced: u64,
    average_batch_size: f64,
}

impl Default for ActionBatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionBatcher {
    /// Create new action batcher with optimized defaults
    pub fn new() -> Self {
        Self {
            priority_queues: [
                VecDeque::new(), // Critical
                VecDeque::new(), // High
                VecDeque::new(), // Normal
                VecDeque::new(), // Low
                VecDeque::new(), // Deferred
            ],
            batch_timeout: Duration::from_millis(8), // Half frame time for responsiveness
            max_batch_size: 32,
            last_flush: Instant::now(),
            navigation_buffer: NavigationBuffer {
                movements: VecDeque::new(),
                last_movement_time: Instant::now(),
                movement_timeout: Duration::from_millis(16), // One frame timeout
                max_buffer_size: 8,
            },
            active_batch_groups: HashMap::new(),
            actions_processed: 0,
            batches_created: 0,
            total_batch_time: Duration::ZERO,
            optimization_stats: OptimizationStats::default(),
        }
    }

    /// Add action to batcher with intelligent priority assignment
    pub fn add_action(&mut self, action: Action, source: ActionSource) -> Option<Vec<Action>> {
        let priority = self.determine_priority(&action, source);
        let batch_group = self.determine_batch_group(&action);
        let can_batch = self.can_batch_action(&action);

        let metadata = ActionMetadata {
            action: action.clone(),
            priority,
            timestamp: Instant::now(),
            source,
            deadline: self.calculate_deadline(priority),
            can_batch,
            batch_group,
        };

        // Handle navigation buffering specially for smooth movement
        if let Some(movement) = self.extract_navigation_movement(&action) {
            self.buffer_navigation_movement(movement);
            // Don't add to regular queue, handle in navigation buffer
            return self.check_flush_conditions();
        }

        // Add to appropriate priority queue
        let queue_index = priority as usize;
        self.priority_queues[queue_index].push_back(metadata.clone());

        // Add to batch group if applicable
        if let Some(group) = batch_group {
            self.active_batch_groups
                .entry(group)
                .or_default()
                .push(metadata);
        }

        self.actions_processed += 1;

        // Check if we should flush batches
        self.check_flush_conditions()
    }

    /// Check if batches should be flushed and return actions
    fn check_flush_conditions(&mut self) -> Option<Vec<Action>> {
        let should_flush = self.last_flush.elapsed() >= self.batch_timeout
            || self.total_pending_actions() >= self.max_batch_size
            || self.has_critical_actions()
            || self.navigation_buffer_should_flush();

        if should_flush {
            Some(self.flush_all_batches())
        } else {
            None
        }
    }

    /// Flush all pending batches and return optimized actions
    pub fn flush_all_batches(&mut self) -> Vec<Action> {
        let flush_start = Instant::now();
        let mut actions = Vec::new();

        // Process navigation buffer first for responsiveness
        actions.extend(self.flush_navigation_buffer());

        // Process priority queues in order (critical first)
        for priority_index in 0..5 {
            let queue_actions = self.flush_priority_queue(priority_index);
            actions.extend(queue_actions);
        }

        // Process batch groups for optimization
        actions.extend(self.flush_batch_groups());

        // Update performance metrics
        self.batches_created += 1;
        self.total_batch_time += flush_start.elapsed();
        self.last_flush = Instant::now();

        if !actions.is_empty() {
            debug!(
                "Flushed batch: {} actions in {:?}",
                actions.len(),
                flush_start.elapsed()
            );
        }

        actions
    }

    /// Flush navigation buffer with movement optimization
    fn flush_navigation_buffer(&mut self) -> Vec<Action> {
        if self.navigation_buffer.movements.is_empty() {
            return Vec::new();
        }

        let mut actions = Vec::new();
        let movements = self
            .navigation_buffer
            .movements
            .drain(..)
            .collect::<Vec<_>>();

        // Combine consecutive movements of the same type
        let optimized_movements = self.optimize_navigation_movements(movements);

        for movement in optimized_movements {
            // Convert each movement to one or more actions
            actions.extend(self.movement_to_actions(movement));
        }

        self.optimization_stats.navigation_movements_combined += 1;
        actions
    }

    /// Flush specific priority queue
    fn flush_priority_queue(&mut self, priority_index: usize) -> Vec<Action> {
        let queue = &mut self.priority_queues[priority_index];
        let actions: Vec<Action> = queue.drain(..).map(|meta| meta.action).collect();

        if !actions.is_empty() {
            debug!(
                "Flushed {} actions from priority queue {}",
                actions.len(),
                priority_index
            );
        }

        actions
    }

    /// Flush and optimize batch groups
    fn flush_batch_groups(&mut self) -> Vec<Action> {
        let mut actions = Vec::new();
        let groups_to_process: Vec<_> = self.active_batch_groups.drain().collect();

        for (group, group_actions) in groups_to_process {
            let optimized = self.optimize_batch_group(group, group_actions);
            actions.extend(optimized);
        }

        actions
    }

    /// Optimize actions within a batch group
    fn optimize_batch_group(
        &mut self,
        group: BatchGroup,
        group_actions: Vec<ActionMetadata>,
    ) -> Vec<Action> {
        if group_actions.is_empty() {
            return Vec::new();
        }

        match group {
            BatchGroup::UIUpdates => {
                self.optimization_stats.ui_updates_batched += 1;
                self.optimize_ui_updates(group_actions)
            }
            BatchGroup::FileOperations => {
                self.optimization_stats.file_operations_grouped += 1;
                self.optimize_file_operations(group_actions)
            }
            BatchGroup::Selection => self.optimize_selection_actions(group_actions),
            _ => {
                // Default: just extract actions without special optimization
                group_actions.into_iter().map(|meta| meta.action).collect()
            }
        }
    }

    /// Optimize UI update actions by deduplicating
    fn optimize_ui_updates(&self, actions: Vec<ActionMetadata>) -> Vec<Action> {
        let mut seen_types = std::collections::HashSet::new();
        let mut optimized = Vec::new();

        // Process in reverse to keep latest of each type
        for meta in actions.into_iter().rev() {
            let action_type = std::mem::discriminant(&meta.action);
            if seen_types.insert(action_type) {
                optimized.push(meta.action);
            }
        }

        optimized.reverse();
        optimized
    }

    /// Optimize file operations by grouping similar operations
    fn optimize_file_operations(&self, actions: Vec<ActionMetadata>) -> Vec<Action> {
        // For now, just return all actions
        // TODO: Implement operation grouping (e.g., multiple file copies)
        actions.into_iter().map(|meta| meta.action).collect()
    }

    /// Optimize selection actions by combining ranges
    fn optimize_selection_actions(&self, actions: Vec<ActionMetadata>) -> Vec<Action> {
        // For now, just return all actions
        // TODO: Implement selection range optimization
        actions.into_iter().map(|meta| meta.action).collect()
    }

    /// Buffer navigation movement for smooth handling
    fn buffer_navigation_movement(&mut self, movement: NavigationMovement) {
        let buffer = &mut self.navigation_buffer;

        // Clear buffer if too much time has passed
        if buffer.last_movement_time.elapsed() > buffer.movement_timeout {
            buffer.movements.clear();
        }

        buffer.movements.push_back(movement);
        buffer.last_movement_time = Instant::now();

        // Limit buffer size
        if buffer.movements.len() > buffer.max_buffer_size {
            buffer.movements.pop_front();
        }
    }

    /// Check if navigation buffer should be flushed
    fn navigation_buffer_should_flush(&self) -> bool {
        !self.navigation_buffer.movements.is_empty()
            && (self.navigation_buffer.last_movement_time.elapsed()
                >= self.navigation_buffer.movement_timeout
                || self.navigation_buffer.movements.len() >= self.navigation_buffer.max_buffer_size)
    }

    /// Optimize navigation movements by combining consecutive ones
    fn optimize_navigation_movements(
        &mut self,
        movements: Vec<NavigationMovement>,
    ) -> Vec<NavigationMovement> {
        if movements.is_empty() {
            return Vec::new();
        }

        let mut optimized = Vec::new();
        let mut current_movement = movements[0];
        let mut count = 1u32;

        for movement in movements.into_iter().skip(1) {
            if self.can_combine_movements(current_movement, movement) {
                count += 1;
            } else {
                // Push accumulated movement
                optimized.push(self.apply_movement_count(current_movement, count));
                current_movement = movement;
                count = 1;
            }
        }

        // Push final movement
        optimized.push(self.apply_movement_count(current_movement, count));
        optimized
    }

    /// Check if two movements can be combined
    fn can_combine_movements(&self, a: NavigationMovement, b: NavigationMovement) -> bool {
        use NavigationMovement::*;
        matches!(
            (a, b),
            (Up(_), Up(_))
                | (Down(_), Down(_))
                | (Left(_), Left(_))
                | (Right(_), Right(_))
                | (PageUp(_), PageUp(_))
                | (PageDown(_), PageDown(_))
        )
    }

    /// Apply movement count to movement type
    fn apply_movement_count(&self, movement: NavigationMovement, count: u32) -> NavigationMovement {
        use NavigationMovement::*;
        match movement {
            Up(_) => Up(count),
            Down(_) => Down(count),
            Left(_) => Left(count),
            Right(_) => Right(count),
            PageUp(_) => PageUp(count),
            PageDown(_) => PageDown(count),
            Home | End => movement, // These don't accumulate
        }
    }

    /// Convert navigation movement to multiple actions if needed
    fn movement_to_actions(&self, movement: NavigationMovement) -> Vec<Action> {
        use NavigationMovement::*;
        match movement {
            Up(count) => {
                // Emit multiple single movements to achieve the desired count
                (0..count).map(|_| Action::MoveSelectionUp).collect()
            }
            Down(count) => {
                // Emit multiple single movements to achieve the desired count
                (0..count).map(|_| Action::MoveSelectionDown).collect()
            }
            Left(count) => {
                // For left movements, emit multiple parent navigations
                (0..count).map(|_| Action::GoToParent).collect()
            }
            Right(count) => {
                // For right movements, emit multiple enter actions
                (0..count).map(|_| Action::EnterSelected).collect()
            }
            PageUp(count) => (0..count).map(|_| Action::PageUp).collect(),
            PageDown(count) => (0..count).map(|_| Action::PageDown).collect(),
            Home => vec![Action::SelectFirst],
            End => vec![Action::SelectLast],
        }
    }

    /// Convert navigation movement back to single action (legacy compatibility)
    fn movement_to_action(&self, movement: NavigationMovement) -> Action {
        use NavigationMovement::*;
        match movement {
            Up(_) => Action::MoveSelectionUp,
            Down(_) => Action::MoveSelectionDown,
            Left(_) => Action::GoToParent,
            Right(_) => Action::EnterSelected,
            PageUp(_) => Action::PageUp,
            PageDown(_) => Action::PageDown,
            Home => Action::SelectFirst,
            End => Action::SelectLast,
        }
    }

    /// Extract navigation movement from action
    fn extract_navigation_movement(&self, action: &Action) -> Option<NavigationMovement> {
        use NavigationMovement::*;
        match action {
            Action::MoveSelectionUp => Some(Up(1)),
            Action::MoveSelectionDown => Some(Down(1)),
            Action::GoToParent => Some(Left(1)),
            Action::EnterSelected => Some(Right(1)),
            Action::PageUp => Some(PageUp(1)),
            Action::PageDown => Some(PageDown(1)),
            Action::SelectFirst => Some(Home),
            Action::SelectLast => Some(End),
            _ => None,
        }
    }

    /// Determine action priority based on type and source
    fn determine_priority(&self, action: &Action, source: ActionSource) -> ActionPriority {
        match (action, source) {
            // Critical actions - immediate execution
            (Action::Quit, _) => ActionPriority::Critical,

            // High priority - user interface responsiveness
            (Action::MoveSelectionUp, ActionSource::UserInput)
            | (Action::MoveSelectionDown, ActionSource::UserInput)
            | (Action::GoToParent, ActionSource::UserInput)
            | (Action::EnterSelected, ActionSource::UserInput) => ActionPriority::High,

            // Normal priority - most user actions
            (Action::EnterSelected, _)
            | (Action::CreateFile, _)
            | (Action::CreateDirectory, _)
            | (Action::Delete, _) => ActionPriority::Normal,

            // Low priority - background operations
            (Action::ReloadDirectory, ActionSource::Timer)
            | (Action::ReloadDirectory, ActionSource::Background) => ActionPriority::Low,

            // Task results and updates
            (Action::TaskResult(_), _) => ActionPriority::Low,
            (Action::DirectoryScanUpdate { .. }, _) => ActionPriority::Low,
            (Action::FileOperationProgress { .. }, _) => ActionPriority::Low,

            // Default to normal priority
            _ => ActionPriority::Normal,
        }
    }

    /// Determine if action can be batched with others
    fn can_batch_action(&self, action: &Action) -> bool {
        match action {
            // These actions should not be batched for immediate response
            Action::Quit => false,

            // Navigation can be buffered specially
            Action::MoveSelectionUp
            | Action::MoveSelectionDown
            | Action::PageUp
            | Action::PageDown
            | Action::GoToParent
            | Action::EnterSelected => true,

            // Most other actions can be safely batched
            _ => true,
        }
    }

    /// Determine batch group for action optimization
    fn determine_batch_group(&self, action: &Action) -> Option<BatchGroup> {
        match action {
            Action::MoveSelectionUp
            | Action::MoveSelectionDown
            | Action::PageUp
            | Action::PageDown
            | Action::SelectFirst
            | Action::SelectLast
            | Action::GoToParent
            | Action::EnterSelected => Some(BatchGroup::Navigation),

            // File operations
            Action::Delete
            | Action::CreateFile
            | Action::CreateDirectory
            | Action::ExecuteCopy { .. }
            | Action::ExecuteMove { .. }
            | Action::ExecuteRename { .. } => Some(BatchGroup::FileOperations),

            // UI updates
            Action::ReloadDirectory | Action::ToggleShowHidden => Some(BatchGroup::UIUpdates),

            // Search operations
            Action::FileNameSearch(_)
            | Action::ContentSearch(_)
            | Action::DirectContentSearch(_)
            | Action::ToggleFileNameSearch
            | Action::ToggleContentSearch => Some(BatchGroup::Search),

            // Clipboard operations
            Action::ToggleClipboardOverlay => Some(BatchGroup::ClipBoard),

            _ => None,
        }
    }

    /// Calculate deadline for action based on priority
    fn calculate_deadline(&self, priority: ActionPriority) -> Option<Instant> {
        let timeout = match priority {
            ActionPriority::Critical => Duration::from_millis(1), // Immediate
            ActionPriority::High => Duration::from_millis(16),    // One frame
            ActionPriority::Normal => Duration::from_millis(50),  // Responsive
            ActionPriority::Low => Duration::from_millis(200),    // Noticeable
            ActionPriority::Deferred => return None,              // No deadline
        };

        Some(Instant::now() + timeout)
    }

    /// Check if there are any critical actions requiring immediate flush
    fn has_critical_actions(&self) -> bool {
        !self.priority_queues[ActionPriority::Critical as usize].is_empty()
    }

    /// Get total pending actions across all queues
    fn total_pending_actions(&self) -> usize {
        self.priority_queues.iter().map(|q| q.len()).sum::<usize>()
            + self.navigation_buffer.movements.len()
            + self
                .active_batch_groups
                .values()
                .map(|v| v.len())
                .sum::<usize>()
    }

    /// Get performance statistics
    pub fn get_performance_stats(&self) -> BatcherStats {
        BatcherStats {
            actions_processed: self.actions_processed,
            batches_created: self.batches_created,
            average_batch_time_us: if self.batches_created > 0 {
                self.total_batch_time.as_micros() as f64 / self.batches_created as f64
            } else {
                0.0
            },
            pending_actions: self.total_pending_actions(),
            navigation_movements_combined: self.optimization_stats.navigation_movements_combined,
            ui_updates_batched: self.optimization_stats.ui_updates_batched,
            file_operations_grouped: self.optimization_stats.file_operations_grouped,
            optimization_ratio: if self.actions_processed > 0 {
                self.optimization_stats.total_actions_reduced as f64 / self.actions_processed as f64
            } else {
                0.0
            },
        }
    }

    /// Configure batch timing parameters
    pub fn configure_timing(&mut self, batch_timeout: Duration, max_batch_size: usize) {
        self.batch_timeout = batch_timeout;
        self.max_batch_size = max_batch_size;
        info!(
            "ActionBatcher timing configured: {:?} timeout, {} max batch size",
            batch_timeout, max_batch_size
        );
    }

    /// Reset performance statistics
    pub fn reset_stats(&mut self) {
        self.actions_processed = 0;
        self.batches_created = 0;
        self.total_batch_time = Duration::ZERO;
        self.optimization_stats = OptimizationStats::default();
        info!("ActionBatcher statistics reset");
    }
}

/// Performance statistics for the action batcher
#[derive(Debug, Clone)]
pub struct BatcherStats {
    pub actions_processed: u64,
    pub batches_created: u64,
    pub average_batch_time_us: f64,
    pub pending_actions: usize,
    pub navigation_movements_combined: u64,
    pub ui_updates_batched: u64,
    pub file_operations_grouped: u64,
    pub optimization_ratio: f64,
}

impl BatcherStats {
    /// Check if batcher performance is healthy
    pub fn is_healthy(&self) -> bool {
        self.average_batch_time_us < 1000.0 && // Sub-millisecond batching
        self.pending_actions < 100 &&           // Reasonable queue size
        self.optimization_ratio > 0.1 // Some optimization happening
    }

    /// Generate performance report
    pub fn report(&self) -> String {
        format!(
            "Batcher: {} actions � {} batches ({:.1}�s avg), {} pending, {:.1}% optimized",
            self.actions_processed,
            self.batches_created,
            self.average_batch_time_us,
            self.pending_actions,
            self.optimization_ratio * 100.0
        )
    }
}
