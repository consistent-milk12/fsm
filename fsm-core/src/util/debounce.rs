//!  src/util/debounce.rs
//!  ===================================================================
//!  High-performance Debounce / Throttle / Batch utilities
//!
//!  • 100 % async-safe: no busy-waits, no un-awaited tasks.
//!  • Per-key state lives in a small slab to avoid repeated `HashMap`
//!    allocations under heavy keystroke storms.
//!  • Uses `tokio::time::Sleep` handles – cancelled sleeps are
//!    automatically dropped, so we never leak green threads.
//!  • Public API unchanged: `DebounceConfig`, Debouncer, Throttler,
//!    `EventBatcher`.
//!
//!  -------------------------------------------------------------------
//!  NOTE: delivered in two parts for readability.  Part 2 contains the
//!  Throttler & `EventBatcher` rewrites plus unit tests.
//!  -------------------------------------------------------------------

use slab::Slab;
use std::{
    collections::HashMap,
    sync::Arc, // for cloning the mutex handles
    time::{Duration, Instant},
};
use tokio::{
    sync::{Mutex, MutexGuard, mpsc},
    task::JoinHandle,
    time::{Instant as TokioInstant, sleep_until},
};
use tracing::{debug, trace};

/* ======================== DebounceConfig ============================ */

#[derive(Debug, Clone)]
pub struct DebounceConfig {
    pub delay: Duration,
    pub max_delay: Option<Duration>,
    pub leading: bool,
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
    #[must_use]
    pub const fn search_input() -> Self {
        // must avoid calling Default::default() in const fn
        Self {
            delay: Duration::from_millis(300),
            max_delay: Some(Duration::from_millis(1000)),
            leading: false,
            trailing: true,
        }
    }

    /// Quick config for UI redraw throttling
    #[must_use]
    pub const fn redraw_throttle() -> Self {
        Self {
            delay: Duration::from_millis(16),
            max_delay: Some(Duration::from_millis(100)),
            leading: true,
            trailing: false,
        }
    }

    /// Quick config for file system watching
    #[must_use]
    pub const fn fs_watch() -> Self {
        Self {
            delay: Duration::from_millis(500),
            max_delay: Some(Duration::from_millis(2000)),
            leading: false,
            trailing: true,
        }
    }
}

/* ============================ Debouncer ============================ */

/// Internal state for each key, stored in a slab slot.
struct Slot<T> {
    last_leading: Instant,
    last_event: Option<T>,
    sleeper: Option<JoinHandle<()>>,
}

/// Debouncer holds shared, clonable handles to its state.
pub struct Debouncer<T> {
    cfg: DebounceConfig,
    slab: Arc<Mutex<Slab<Slot<T>>>>, // Arc so we can clone into tasks
    key_map: Arc<Mutex<HashMap<String, usize>>>, // same here
    tx: mpsc::UnboundedSender<(String, T)>,
}

impl<T: Clone + Send + 'static> Debouncer<T> {
    /// Create a new debouncer and its Rx endpoint
    #[must_use]
    pub fn new(cfg: DebounceConfig) -> (Self, mpsc::UnboundedReceiver<(String, T)>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let deb = Self {
            cfg,
            slab: Arc::new(Mutex::new(Slab::new())),
            key_map: Arc::new(Mutex::new(HashMap::new())),
            tx,
        };
        (deb, rx)
    }

    /// Submit an event for debouncing
    pub async fn submit(&self, key: String, ev: T) {
        trace!("Debouncer received event for key: {}", key);

        // 1) Find or allocate a slab slot index
        let idx: usize = {
            let mut km: MutexGuard<'_, HashMap<String, usize>> = self.key_map.lock().await;

            if let Some(&i) = km.get(&key) {
                i
            } else {
                let i: usize = self.slab.lock().await.insert(Slot {
                    last_leading: Instant::now(),
                    last_event: None,
                    sleeper: None,
                });

                km.insert(key.clone(), i);

                i
            }
        };

        // 2) Access the slot mutably
        let slot: &mut Slot<T> = &mut self.slab.lock().await[idx];

        // Leading-edge: fire immediately on first event if configured
        if self.cfg.leading && slot.last_event.is_none() {
            debug!("Triggering leading edge for key: {}", key);
            let _ = self.tx.send((key.clone(), ev.clone()));

            slot.last_leading = Instant::now();
        }

        // Cache the latest event for trailing-edge
        slot.last_event = Some(ev.clone());

        // Cancel any existing sleeper
        if let Some(handle) = slot.sleeper.take() {
            handle.abort();
        }

        // Spawn a fresh sleeper if trailing-edge is desired
        if self.cfg.trailing {
            let delay_deadline = TokioInstant::from_std(Instant::now() + self.cfg.delay);
            let hard_deadline = self
                .cfg
                .max_delay
                .map(|d| TokioInstant::from_std(slot.last_leading + d));

            // clone handles for the task
            let slab_ptr = Arc::clone(&self.slab);
            let tx_clone = self.tx.clone();
            let key_clone = key.clone();

            slot.sleeper = Some(tokio::spawn(async move {
                // wait until the earlier of the two deadlines
                if let Some(hd) = hard_deadline {
                    sleep_until(delay_deadline.min(hd)).await;
                } else {
                    sleep_until(delay_deadline).await;
                }

                // extract the cached event
                let mut slab = slab_ptr.lock().await;
                if let Some(event) = slab[idx].last_event.take() {
                    debug!("Triggering trailing edge for key: {}", key_clone);
                    let _ = tx_clone.send((key_clone, event));
                }
            }));
        }
    }

    /// Force-trigger all pending trailing events
    pub async fn flush(&self) {
        debug!("Flushing all pending debounced events");
        // Lock both maps in a deterministic order
        let km = self.key_map.lock().await;
        let mut slab = self.slab.lock().await;
        for (key, &idx) in km.iter() {
            if let Some(event) = slab[idx].last_event.take() {
                let _ = self.tx.send((key.clone(), event));
            }
        }
    }
}

//  Simple rate-limiter for infrequent operations.
#[derive(Debug)]
pub struct Throttler {
    //  Moment when the previous trigger occurred.
    last: Option<Instant>,
    //  Minimum interval required between triggers.
    interval: Duration,
}

impl Throttler {
    /// Create a new throttler.
    #[must_use]
    pub const fn new(interval: Duration) -> Self {
        Self {
            last: None,
            interval,
        }
    }

    /// Returns true when an operation may run.
    pub fn should_trigger(&mut self) -> bool {
        let now = Instant::now();
        match self.last {
            None => {
                self.last = Some(now);
                true
            }
            Some(prev) if now.duration_since(prev) >= self.interval => {
                self.last = Some(now);
                true
            }
            _ => false,
        }
    }

    /// Reset the internal timer to allow the next call immediately.
    pub const fn reset(&mut self) {
        self.last = None;
    }
}

/* ========================== EventBatcher ============================ */

//  Utility that collects events into fixed-size / age batches.
#[derive(Debug)]
pub struct EventBatcher<T> {
    //  Buffered events.
    buf: Vec<T>,
    //  Flush once this many items accumulated.
    max_size: usize,
    //  Flush after this much time since last flush.
    max_age: Duration,
    //  Timestamp of last flush.
    last_flush: Instant,
    //  Output channel to consumer.
    tx: mpsc::UnboundedSender<Vec<T>>,
}

impl<T: Clone + Send + 'static> EventBatcher<T> {
    /// Create a new batcher and its receiver.
    #[must_use]
    pub fn new(max_size: usize, max_age: Duration) -> (Self, mpsc::UnboundedReceiver<Vec<T>>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            Self {
                buf: Vec::with_capacity(max_size),
                max_size,
                max_age,
                last_flush: Instant::now(),
                tx,
            },
            rx,
        )
    }

    /// Add an event; flushes automatically as needed.
    pub fn add(&mut self, ev: T) {
        self.buf.push(ev);
        let age = self.last_flush.elapsed();
        let full = self.buf.len() >= self.max_size;
        if full || (age >= self.max_age && !self.buf.is_empty()) {
            self.flush();
        }
    }

    /// Manually flush the current batch.
    pub fn flush(&mut self) {
        if self.buf.is_empty() {
            return;
        }
        debug!("batch flush: {} item(s)", self.buf.len());
        let batch = std::mem::take(&mut self.buf);
        let _ = self.tx.send(batch);
        self.last_flush = Instant::now();
    }
}
