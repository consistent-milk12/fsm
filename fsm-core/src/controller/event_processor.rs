//! Event processor powered by [`kanal`](https://docs.rs/kanal) channels.
//!
//! This implementation replaces the `crossfire` channels used in the
//! previous version with [`kanal`](https://docs.rs/kanal) channels.  Kanal
//! unifies synchronous and asynchronous messaging and employs several
//! performance techniques, such as copying objects directly from the
//! sender’s stack to the receiver’s stack, reducing heap allocations
//!:contentReference[oaicite:0]{index=0}.  Kanal allows a single channel to be
//! accessed in both sync and async contexts via conversion methods
//! (`clone_sync` and `clone_async`):contentReference[oaicite:1]{index=1}.  Here
//! we exclusively use the asynchronous API for both sending and
//! receiving.  Messages are submitted using the `try_send_option`
//! method so that failed sends do not drop the event, enabling the
//! caller to handle overflow gracefully.
//!
//! Handler registration, metrics, and overall architecture remain
//! similar to the `crossfire` based implementation.  We continue to
//! leverage [`ArcSwap`](https://docs.rs/arc-swap) to maintain a list of
//! event handlers in a read‑mostly fashion:contentReference[oaicite:2]{index=2},
//! and [`enum_map`](https://docs.rs/enum-map) for efficient mapping
//! from `Priority` values to counters:contentReference[oaicite:3]{index=3}.

use crate::controller::actions::Action;
use crate::controller::event_loop::TaskResult;
use arc_swap::ArcSwap;
use enum_map::{Enum, EnumMap, enum_map};
use kanal::{AsyncReceiver, AsyncSender, bounded_async};
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::select;
use tracing::{debug, trace, warn};

use crossterm::event::KeyCode;
use crossterm::event::{Event as TerminalEvent, KeyEvent};

/// Event priority levels for scheduling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Enum)]
#[repr(u8)]
pub enum Priority {
    /// ESC, Quit events (highest priority)
    Critical = 0,

    /// User input
    High = 1,

    /// Background tasks
    Normal = 2,

    /// Metrics, logging (lowest priority)
    Low = 3,
}

/// Unified event type without heap allocation.
#[derive(Debug, Clone)]
pub enum Event {
    /// Keyboard input with priority hint.
    Key { event: KeyEvent, priority: Priority },

    /// Terminal resize.
    Resize { width: u16, height: u16 },

    /// Background task completion.
    Task { result: Box<TaskResult> },

    /// Direct action injection.
    Action { action: Box<Action>, priority: Priority },

    /// Periodic tick for UI updates.
    Tick,
}

impl Event {
    /// Determine the priority of an event.
    fn priority(&self) -> Priority {
        match self {
            Event::Key { priority, .. } => *priority,

            Event::Action { priority, .. } => *priority,

            Event::Resize { .. } => Priority::High,

            Event::Task { .. } => Priority::Normal,

            Event::Tick => Priority::Low,
        }
    }
}

/// Event handler trait for modular processing.
pub trait EventHandler: Send + Sync {
    /// Check if this handler can process the event.
    fn can_handle(&self, event: &Event) -> bool;

    /// Process event and return actions.
    fn handle(&mut self, event: Event) -> Result<Vec<Action>, crate::error::AppError>;

    /// Handler priority for ordering (lower numbers run first).
    fn priority(&self) -> u8 {
        100
    }

    /// Handler name for debugging.
    fn name(&self) -> &'static str;
}

/// Internal representation of a registered event handler.
///
/// A `HandlerEntry` bundles a handler’s static metadata (its
/// priority and name) together with the handler itself wrapped in an
/// `Arc<Mutex<...>>` for interior mutability.  The handler is
/// referenced via an `Arc` so that multiple snapshots of the handler
/// list (held in the [`ArcSwap`]) share the same underlying handler
/// instance.  Cloning a `HandlerEntry` only clones the `Arc`, not the
/// underlying handler, so updates via the `Mutex` are visible to all
/// readers.
#[derive(Clone)]
struct HandlerEntry {
    /// Handler execution priority (lower values run first).
    priority: u8,

    /// Handler name for diagnostics.
    name: &'static str,

    /// The actual event handler wrapped in an `Arc<Mutex<...>>`.
    handler: Arc<Mutex<Box<dyn EventHandler>>>,
}

/// Event processor using Kanal channels.
pub struct EventProcessor {
    /// Critical priority channel (ESC, Quit)
    critical_rx: AsyncReceiver<Event>,
    critical_tx: AsyncSender<Event>,

    /// High priority channel (user input)
    high_rx: AsyncReceiver<Event>,
    high_tx: AsyncSender<Event>,

    /// Normal priority channel (background tasks)
    normal_rx: AsyncReceiver<Event>,
    normal_tx: AsyncSender<Event>,

    /// Registered handlers.  Instead of storing a nested
    /// `Vec<Arc<Mutex<Box<dyn EventHandler>>>>`, which required
    /// multiple layers of pointer indirection and was hard to follow,
    /// we wrap each handler and its static metadata in a [`HandlerEntry`]
    /// and keep a vector of those entries in an [`ArcSwap`].  When
    /// registering a new handler we clone the existing vector (shallow
    /// cloning each `HandlerEntry`) and push a new entry; the
    /// [`ArcSwap`] then atomically updates the pointer to the new
    /// vector, giving readers a lock‑free snapshot:contentReference[oaicite:4]{index=4}.
    handlers: ArcSwap<Vec<HandlerEntry>>,

    /// Performance metrics.
    metrics: EventMetrics,

    /// Event processing configuration.
    config: ProcessorConfig,
}

/// Performance metrics with lock‑free counters.
#[derive(Debug)]
pub struct EventMetrics {
    /// Total events processed.
    total_events: AtomicU64,

    /// Events processed by priority.
    priority_counts: EnumMap<Priority, AtomicU64>,

    /// Exponentially weighted moving average of processing latency in
    /// nanoseconds.
    avg_latency_ns: AtomicU64,

    /// Dropped events (buffer overflow).
    dropped_events: AtomicU64,

    /// Current queue depths.
    queue_depths: EnumMap<Priority, AtomicUsize>,
}

impl Default for EventMetrics {
    fn default() -> Self {
        Self {
            total_events: AtomicU64::new(0),
            priority_counts: enum_map! {
                Priority::Critical => AtomicU64::new(0),
                Priority::High     => AtomicU64::new(0),
                Priority::Normal   => AtomicU64::new(0),
                Priority::Low      => AtomicU64::new(0),
            },
            avg_latency_ns: AtomicU64::new(0),
            dropped_events: AtomicU64::new(0),
            queue_depths: enum_map! {
                Priority::Critical => AtomicUsize::new(0),
                Priority::High     => AtomicUsize::new(0),
                Priority::Normal   => AtomicUsize::new(0),
                Priority::Low      => AtomicUsize::new(0),
            },
        }
    }
}

/// Processor configuration.
#[derive(Debug, Clone)]
pub struct ProcessorConfig {
    /// Channel buffer sizes
    pub critical_buffer: usize,
    pub high_buffer: usize,
    pub normal_buffer: usize,

    /// Event coalescing window
    pub coalesce_window: Duration,

    /// Max events to process per iteration
    pub batch_size: usize,
}

impl Default for ProcessorConfig {
    fn default() -> Self {
        Self {
            critical_buffer: 16,
            high_buffer: 64,
            normal_buffer: 256,
            coalesce_window: Duration::from_millis(16),
            batch_size: 10,
        }
    }
}

impl Default for EventProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl EventProcessor {
    /// Create new event processor with default configuration.
    pub fn new() -> Self {
        Self::with_config(ProcessorConfig::default())
    }

    /// Create a processor with custom configuration.
    pub fn with_config(config: ProcessorConfig) -> Self {
        let (critical_tx, critical_rx) = bounded_async(config.critical_buffer);

        let (high_tx, high_rx) = bounded_async(config.high_buffer);

        let (normal_tx, normal_rx) = bounded_async(config.normal_buffer);

        Self {
            critical_rx,
            critical_tx,
            high_rx,
            high_tx,
            normal_rx,
            normal_tx,
            handlers: ArcSwap::from_pointee(Vec::new()),
            metrics: EventMetrics::default(),
            config,
        }
    }

    /// Register an event handler.
    pub fn register_handler(&self, handler: Box<dyn EventHandler>) {
        // Create a new handler entry containing its static metadata and
        // the handler itself wrapped in an `Arc<Mutex<...>>`.
        let entry = HandlerEntry {
            priority: handler.priority(),
            name: handler.name(),
            handler: Arc::new(Mutex::new(handler)),
        };

        debug!("Registering handler: {}", entry.name);

        // Obtain the current list of handlers and clone it.  Use `as_ref()`
        // on the `Arc` to avoid moving its contents; `clone()` then performs
        // a shallow copy because `HandlerEntry` implements `Clone`.
        let current: Arc<Vec<HandlerEntry>> = self.handlers.load_full();

        let mut new_vec: Vec<HandlerEntry> = current.as_ref().clone();
        new_vec.push(entry);

        // Sort by the stored static priority to avoid locking each
        // handler during sorting.
        new_vec.sort_by_key(|e: &HandlerEntry| e.priority);

        // Store the new vector atomically.  Older snapshots continue
        // to point at the previous vector until replaced.
        self.handlers.store(Arc::new(new_vec));
    }

    /// Get channel senders for event submission.
    pub fn senders(&self) -> EventSenders {
        EventSenders {
            critical: self.critical_tx.clone(),
            high: self.high_tx.clone(),
            normal: self.normal_tx.clone(),
        }
    }

    /// Submit an event with automatic priority routing.  If the buffer
    /// is full, the event is returned to the caller.  If the channel
    /// is disconnected, the event is also returned.  Kanal’s
    /// `try_send_option` stores the event in an `Option` and only
    /// consumes it upon success, allowing us to recover it on
    /// failure.
    pub fn submit(&self, event: Event) -> Result<(), Box<Event>> {
        let priority: Priority = event.priority();

        let tx: &AsyncSender<Event> = match priority {
            Priority::Critical => &self.critical_tx,
            Priority::High => &self.high_tx,
            Priority::Normal | Priority::Low => &self.normal_tx,
        };

        let mut opt: Option<Event> = Some(event);

        match tx.try_send_option(&mut opt) {
            Ok(true) => Ok(()),

            Ok(false) => {
                let ev = opt
                    .take()
                    .expect("event should still be present on failed send");

                self.metrics.dropped_events.fetch_add(1, Ordering::Relaxed);

                warn!("Event queue full, dropping event");

                Err(Box::new(ev))
            }

            Err(_e) => {
                // Channel is closed; recover the event and return
                let ev = opt
                    .take()
                    .expect("event should still be present on channel error");

                Err(Box::new(ev))
            }
        }
    }

    /// Process a batch of events up to `batch_size`.  Returns a vector of
    /// actions to be executed or `None` if no actions were produced.
    pub async fn process_batch(&self) -> Option<Vec<Action>> {
        let start = Instant::now();
        let mut actions = Vec::new();
        let mut events_processed = 0usize;

        for _ in 0..self.config.batch_size {
            match self.recv_next_event().await {
                Some(event) => {
                    events_processed += 1;
                    match self.process_event(event) {
                        Ok(mut event_actions) => actions.append(&mut event_actions),

                        Err(e) => warn!("Error processing event: {}", e),
                    }
                }

                None => break,
            }
        }

        if events_processed > 0 {
            self.update_metrics(events_processed, start.elapsed());
            self.update_queue_depths();
        }

        if actions.is_empty() {
            None
        } else {
            Some(actions)
        }
    }

    /// Receive the next event with priority order.  Attempts
    /// non‑blocking receives on channels in critical, high and normal
    /// order.  Falls back to awaiting the first available event via
    /// `tokio::select!`.  Returns `None` when all receivers are
    /// disconnected.
    async fn recv_next_event(&self) -> Option<Event> {
        // Try critical
        match self.critical_rx.try_recv() {
            Ok(Some(event)) => {
                self.metrics.priority_counts[Priority::Critical].fetch_add(1, Ordering::Relaxed);
                return Some(event);
            }

            Ok(None) => {}

            Err(_e) => return None,
        }

        // Try high
        match self.high_rx.try_recv() {
            Ok(Some(event)) => {
                self.metrics.priority_counts[Priority::High].fetch_add(1, Ordering::Relaxed);

                return Some(event);
            }

            Ok(None) => {}

            Err(_e) => return None,
        }
        // Try normal
        match self.normal_rx.try_recv() {
            Ok(Some(event)) => {
                self.metrics.priority_counts[Priority::Normal].fetch_add(1, Ordering::Relaxed);

                return Some(event);
            }

            Ok(None) => {}

            Err(_e) => return None,
        }

        // Await whichever arrives first
        select! {
            recv_crit = self.critical_rx.recv() => {
                match recv_crit {
                    Ok(event) => {
                        self.metrics.priority_counts[Priority::Critical].fetch_add(1, Ordering::Relaxed);

                        Some(event)
                    }

                    Err(_e) => None,
                }
            }
            recv_high = self.high_rx.recv() => {
                match recv_high {
                    Ok(event) => {
                        self.metrics.priority_counts[Priority::High].fetch_add(1, Ordering::Relaxed);

                        Some(event)
                    }

                    Err(_e) => None,
                }
            }
            recv_norm = self.normal_rx.recv() => {
                match recv_norm {
                    Ok(event) => {
                        self.metrics.priority_counts[Priority::Normal].fetch_add(1, Ordering::Relaxed);

                        Some(event)
                    }

                    Err(_e) => None,
                }
            }
        }
    }

    /// Process a single event through the handler chain.
    fn process_event(&self, event: Event) -> Result<Vec<Action>, crate::error::AppError> {
        trace!("Processing event: {:?}", event);
        // Load the current handler list atomically.  This returns an
        // `Arc<Vec<HandlerEntry>>`; dereferencing provides a slice of
        // handlers without locking.
        let handlers: Arc<Vec<HandlerEntry>> = self.handlers.load_full();

        for entry in handlers.iter() {
            // Acquire the mutex for the actual handler.  We lock only
            // long enough to call `can_handle` and `handle` so that
            // other tasks can still read the handler list concurrently.
            let mut guard = entry.handler.lock();

            if guard.can_handle(&event) {
                debug!("Handler {} processing event", entry.name);
                return guard.handle(event);
            }
        }

        debug!("No handler for event: {:?}", event);

        Ok(Vec::new())
    }

    /// Update performance metrics.
    fn update_metrics(&self, count: usize, elapsed: Duration) {
        self.metrics
            .total_events
            .fetch_add(count as u64, Ordering::Relaxed);

        let elapsed_ns: u64 = if count > 0 {
            elapsed.as_nanos() as u64 / count as u64
        } else {
            0
        };

        let current_avg: u64 = self.metrics.avg_latency_ns.load(Ordering::Relaxed);

        let new_avg: u64 = if current_avg == 0 {
            elapsed_ns
        } else {
            (current_avg * 9 + elapsed_ns) / 10
        };

        self.metrics
            .avg_latency_ns
            .store(new_avg, Ordering::Relaxed);
    }

    /// Update queue depth metrics.
    fn update_queue_depths(&self) {
        self.metrics.queue_depths[Priority::Critical]
            .store(self.critical_rx.len(), Ordering::Relaxed);
        self.metrics.queue_depths[Priority::High].store(self.high_rx.len(), Ordering::Relaxed);
        self.metrics.queue_depths[Priority::Normal].store(self.normal_rx.len(), Ordering::Relaxed);
        // Low remains zero
    }

    /// Get a snapshot of the current metrics.
    pub fn metrics(&self) -> EventMetricsSnapshot {
        EventMetricsSnapshot {
            total_events: self.metrics.total_events.load(Ordering::Relaxed),
            priority_counts: enum_map! {
                Priority::Critical => self.metrics.priority_counts[Priority::Critical].load(Ordering::Relaxed),
                Priority::High     => self.metrics.priority_counts[Priority::High].load(Ordering::Relaxed),
                Priority::Normal   => self.metrics.priority_counts[Priority::Normal].load(Ordering::Relaxed),
                Priority::Low      => self.metrics.priority_counts[Priority::Low].load(Ordering::Relaxed),
            },
            avg_latency_ns: self.metrics.avg_latency_ns.load(Ordering::Relaxed),
            dropped_events: self.metrics.dropped_events.load(Ordering::Relaxed),
            queue_depths: enum_map! {
                Priority::Critical => self.metrics.queue_depths[Priority::Critical].load(Ordering::Relaxed),
                Priority::High     => self.metrics.queue_depths[Priority::High].load(Ordering::Relaxed),
                Priority::Normal   => self.metrics.queue_depths[Priority::Normal].load(Ordering::Relaxed),
                Priority::Low      => self.metrics.queue_depths[Priority::Low].load(Ordering::Relaxed),
            },
        }
    }
}

/// Channel senders for event submission.
#[derive(Clone)]
pub struct EventSenders {
    pub critical: AsyncSender<Event>,
    pub high: AsyncSender<Event>,
    pub normal: AsyncSender<Event>,
}

/// Snapshot of metrics for reporting.
#[derive(Debug, Clone)]
pub struct EventMetricsSnapshot {
    pub total_events: u64,
    pub priority_counts: EnumMap<Priority, u64>,
    pub avg_latency_ns: u64,
    pub dropped_events: u64,
    pub queue_depths: EnumMap<Priority, usize>,
}

impl EventMetricsSnapshot {
    /// Average latency in microseconds.
    pub fn avg_latency_us(&self) -> f64 {
        self.avg_latency_ns as f64 / 1_000.0
    }
    /// Average latency in milliseconds.
    pub fn avg_latency_ms(&self) -> f64 {
        self.avg_latency_ns as f64 / 1_000_000.0
    }
    /// Events per second based on the average latency.
    pub fn events_per_second(&self) -> f64 {
        if self.avg_latency_ns == 0 {
            0.0
        } else {
            1_000_000_000.0 / self.avg_latency_ns as f64
        }
    }
}

/// Convert terminal events to our event type.
pub fn terminal_event_to_event(term_event: TerminalEvent) -> Option<Event> {
    match term_event {
        TerminalEvent::Key(key_event) => {
            let priority = match key_event.code {
                KeyCode::Esc => Priority::Critical,
                KeyCode::Char('q') => Priority::Critical,
                _ => Priority::High,
            };
            Some(Event::Key {
                event: key_event,
                priority,
            })
        }

        TerminalEvent::Resize(width, height) => Some(Event::Resize { width, height }),

        TerminalEvent::Mouse(_) => None,

        _ => None,
    }
}
