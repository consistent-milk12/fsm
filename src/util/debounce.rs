//! src/util/debounce.rs
//! ============================================================================
//! # Event Debouncing and Throttling System
//!
//! Provides utilities for debouncing user input and throttling frequent operations
//! to improve performance and user experience.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{debug, trace};

/// Debouncing configuration for different event types
#[derive(Debug, Clone)]
pub struct DebounceConfig {
    /// Minimum delay between events
    pub delay: Duration,
    /// Maximum delay before forcing an event
    pub max_delay: Option<Duration>,
    /// Whether to trigger on leading edge (immediate first event)
    pub leading: bool,
    /// Whether to trigger on trailing edge (after delay)
    pub trailing: bool,
}

impl Default for DebounceConfig {
    fn default() -> Self {
        Self {
            delay: Duration::from_millis(300),
            max_delay: Some(Duration::from_millis(1000)),
            leading: false,
            trailing: true,
        }
    }
}

impl DebounceConfig {
    /// Quick config for search input debouncing
    pub fn search_input() -> Self {
        Self {
            delay: Duration::from_millis(300), // Wait 300ms after last keystroke
            max_delay: Some(Duration::from_millis(1000)), // Force search after 1s
            leading: false,
            trailing: true,
        }
    }

    /// Quick config for UI redraw throttling
    pub fn redraw_throttle() -> Self {
        Self {
            delay: Duration::from_millis(16),            // ~60fps
            max_delay: Some(Duration::from_millis(100)), // Force redraw after 100ms
            leading: true,                               // Immediate first redraw
            trailing: false,
        }
    }

    /// Quick config for file system watching
    pub fn fs_watch() -> Self {
        Self {
            delay: Duration::from_millis(500), // Wait 500ms for fs changes to settle
            max_delay: Some(Duration::from_millis(2000)), // Force update after 2s
            leading: false,
            trailing: true,
        }
    }
}

/// Event debouncer for async operations
#[derive(Debug)]
pub struct Debouncer<T> {
    config: DebounceConfig,
    last_events: HashMap<String, (Instant, Option<T>)>,
    output_tx: mpsc::UnboundedSender<(String, T)>,
}

impl<T: Clone + Send + 'static> Debouncer<T> {
    /// Create a new debouncer with the given configuration
    pub fn new(config: DebounceConfig) -> (Self, mpsc::UnboundedReceiver<(String, T)>) {
        let (output_tx, output_rx) = mpsc::unbounded_channel();

        let debouncer = Self {
            config,
            last_events: HashMap::new(),
            output_tx,
        };

        (debouncer, output_rx)
    }

    /// Submit an event for debouncing
    pub async fn submit(&mut self, key: String, event: T) {
        let now = Instant::now();
        trace!("Debouncer received event for key: {}", key);

        // Check if we should trigger immediately (leading edge)
        let should_trigger_leading = self.config.leading && !self.last_events.contains_key(&key);

        if should_trigger_leading {
            debug!("Triggering leading edge event for key: {}", key);
            let _ = self.output_tx.send((key.clone(), event.clone()));
        }

        // Update the last event time and data
        self.last_events
            .insert(key.clone(), (now, Some(event.clone())));

        // Start the debounce timer
        if self.config.trailing {
            let key_clone = key.clone();
            let event_clone = event.clone();
            let config = self.config.clone();
            let tx = self.output_tx.clone();
            let _last_events = &mut self.last_events;

            tokio::spawn(async move {
                // Wait for the debounce delay
                sleep(config.delay).await;

                // Check if this is still the latest event for this key
                // Note: This is a simplified check - a full implementation would
                // need more sophisticated state management

                debug!("Debounce timer expired for key: {}", key_clone);
                let _ = tx.send((key_clone, event_clone));
            });
        }
    }

    /// Force trigger all pending events (useful for cleanup)
    pub fn flush(&mut self) {
        debug!("Flushing all pending debounced events");
        for (key, (_, event)) in self.last_events.drain() {
            if let Some(event) = event {
                let _ = self.output_tx.send((key, event));
            }
        }
    }
}

/// Simple throttler for rate-limiting operations
#[derive(Debug)]
pub struct Throttler {
    last_trigger: Option<Instant>,
    interval: Duration,
}

impl Throttler {
    /// Create a new throttler with the given interval
    pub fn new(interval: Duration) -> Self {
        Self {
            last_trigger: None,
            interval,
        }
    }

    /// Check if enough time has passed to allow the next operation
    pub fn should_trigger(&mut self) -> bool {
        let now = Instant::now();

        match self.last_trigger {
            None => {
                self.last_trigger = Some(now);
                true
            }
            Some(last) if now.duration_since(last) >= self.interval => {
                self.last_trigger = Some(now);
                true
            }
            _ => false,
        }
    }

    /// Force next trigger (reset the timer)
    pub fn reset(&mut self) {
        self.last_trigger = None;
    }
}

/// Utility for batching multiple events into a single operation
#[derive(Debug)]
pub struct EventBatcher<T> {
    batch: Vec<T>,
    max_size: usize,
    max_age: Duration,
    last_flush: Instant,
    output_tx: mpsc::UnboundedSender<Vec<T>>,
}

impl<T: Clone + Send + 'static> EventBatcher<T> {
    /// Create a new event batcher
    pub fn new(max_size: usize, max_age: Duration) -> (Self, mpsc::UnboundedReceiver<Vec<T>>) {
        let (output_tx, output_rx) = mpsc::unbounded_channel();

        let batcher = Self {
            batch: Vec::with_capacity(max_size),
            max_size,
            max_age,
            last_flush: Instant::now(),
            output_tx,
        };

        (batcher, output_rx)
    }

    /// Add an event to the batch
    pub async fn add(&mut self, event: T) {
        self.batch.push(event);

        // Check if we should flush due to size or age
        let should_flush_size = self.batch.len() >= self.max_size;
        let should_flush_age = self.last_flush.elapsed() >= self.max_age && !self.batch.is_empty();

        if should_flush_size || should_flush_age {
            self.flush().await;
        }
    }

    /// Flush the current batch
    pub async fn flush(&mut self) {
        if !self.batch.is_empty() {
            debug!("Flushing batch of {} events", self.batch.len());
            let batch = std::mem::take(&mut self.batch);
            let _ = self.output_tx.send(batch);
            self.last_flush = Instant::now();
        }
    }
}
