//! Arc/Mutex performance monitoring for Crisis #2 validation
//! Tracks lock contention, wait times, and concurrent access patterns

use std::{
    sync::{Arc, Mutex, MutexGuard},
    thread,
    time::{Duration, Instant},
};
use tracing::{debug, instrument, warn};

use super::collector::ProfileCollector;

/// Instrumented Arc wrapper that tracks performance metrics
pub struct InstrumentedArc<T> {
    inner: Arc<Mutex<T>>,
    name: String,
    collector: Option<Arc<ProfileCollector>>,
}

/// Guard returned by InstrumentedArc that tracks lock hold time
pub struct InstrumentedGuard<'a, T> {
    guard: MutexGuard<'a, T>,
    lock_acquired_at: Instant,
    operation_name: String,
    collector: Option<Arc<ProfileCollector>>,
}

impl<T> InstrumentedArc<T> {
    /// Create a new instrumented Arc with profiling
    pub fn new(value: T, name: String) -> Self {
        Self {
            inner: Arc::new(Mutex::new(value)),
            name,
            collector: None,
        }
    }

    /// Create an instrumented Arc with a specific collector
    pub fn with_collector(value: T, name: String, collector: Arc<ProfileCollector>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(value)),
            name,
            collector: Some(collector),
        }
    }

    /// Lock with performance monitoring
    #[instrument(skip(self), fields(arc_name = %self.name))]
    pub fn lock(
        &self,
    ) -> Result<InstrumentedGuard<'_, T>, std::sync::PoisonError<MutexGuard<'_, T>>> {
        let start_time = Instant::now();

        // Try to acquire lock immediately to detect contention
        let contention_detected = self.inner.try_lock().is_err();
        if contention_detected {
            debug!("Lock contention detected for {}", self.name);
        }

        // Acquire the lock (blocking if necessary)
        match self.inner.lock() {
            Ok(guard) => {
                let wait_time = start_time.elapsed();
                let wait_time_ms = wait_time.as_millis() as u64;

                // Record metrics if collector is available
                if let Some(collector) = &self.collector {
                    collector.record_arc_operation(&self.name, wait_time_ms, contention_detected);
                }

                // Log significant wait times
                if wait_time > Duration::from_millis(10) {
                    warn!("Long lock wait time for {}: {}ms", self.name, wait_time_ms);
                } else if wait_time > Duration::from_millis(1) {
                    debug!("Lock acquired for {} after {}ms", self.name, wait_time_ms);
                }

                Ok(InstrumentedGuard {
                    guard,
                    lock_acquired_at: Instant::now(),
                    operation_name: self.name.clone(),
                    collector: self.collector.clone(),
                })
            }
            Err(e) => {
                warn!("Failed to acquire lock for {}: {}", self.name, e);
                Err(e)
            }
        }
    }

    /// Try to lock without blocking, with performance monitoring
    #[instrument(skip(self), fields(arc_name = %self.name))]
    pub fn try_lock(
        &'_ self,
    ) -> Result<Option<InstrumentedGuard<'_, T>>, std::sync::PoisonError<MutexGuard<'_, T>>> {
        let start_time = Instant::now();

        match self.inner.try_lock() {
            Ok(guard) => {
                let wait_time_ms = start_time.elapsed().as_millis() as u64;

                // Record successful non-blocking acquisition
                if let Some(collector) = &self.collector {
                    collector.record_arc_operation(
                        &format!("{}_try", self.name),
                        wait_time_ms,
                        false,
                    );
                }

                debug!("Non-blocking lock acquired for {}", self.name);

                Ok(Some(InstrumentedGuard {
                    guard,
                    lock_acquired_at: Instant::now(),
                    operation_name: self.name.clone(),
                    collector: self.collector.clone(),
                }))
            }
            Err(std::sync::TryLockError::WouldBlock) => {
                // Lock is contended, record this
                if let Some(collector) = &self.collector {
                    collector.record_arc_operation(&format!("{}_contended", self.name), 0, true);
                }

                debug!("Lock contended for {} (try_lock)", self.name);
                Ok(None)
            }
            Err(std::sync::TryLockError::Poisoned(e)) => {
                warn!("Poisoned lock detected for {}: {}", self.name, e);
                Err(e)
            }
        }
    }

    /// Get the name of this Arc for debugging
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Clone the underlying Arc (for sharing across threads)
    pub fn clone_arc(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            name: self.name.clone(),
            collector: self.collector.clone(),
        }
    }
}

impl<T> Clone for InstrumentedArc<T> {
    fn clone(&self) -> Self {
        self.clone_arc()
    }
}

impl<'a, T> std::ops::Deref for InstrumentedGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}

impl<'a, T> std::ops::DerefMut for InstrumentedGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.guard
    }
}

impl<'a, T> Drop for InstrumentedGuard<'a, T> {
    fn drop(&mut self) {
        let hold_time = self.lock_acquired_at.elapsed();
        let hold_time_ms = hold_time.as_millis() as u64;

        // Record lock hold time
        if let Some(collector) = &self.collector {
            collector.record_event(
                &format!("{}_hold", self.operation_name),
                hold_time_ms,
                Some(&format!("thread:{:?}", thread::current().id())),
            );
        }

        // Warn about long-held locks that might cause contention
        if hold_time > Duration::from_millis(100) {
            warn!(
                "Long lock hold time for {}: {}ms",
                self.operation_name, hold_time_ms
            );
        } else if hold_time > Duration::from_millis(10) {
            debug!("Lock held for {}: {}ms", self.operation_name, hold_time_ms);
        }
    }
}

/// Arc monitoring utility functions
pub struct ArcMonitor;

impl ArcMonitor {
    /// Detect potential deadlock patterns by analyzing lock ordering
    pub fn analyze_lock_ordering(operations: &[(String, u64)]) -> Vec<String> {
        let mut warnings = Vec::new();

        // Look for patterns that might indicate lock ordering issues
        for window in operations.windows(2) {
            if let (Some((op1, time1)), Some((op2, time2))) = (window.first(), window.get(1)) {
                // If we see UI -> FS followed by FS -> UI within a short time,
                // that might indicate a deadlock risk
                if op1.contains("ui") && op2.contains("fs") && time2.saturating_sub(*time1) < 1000 {
                    warnings.push(format!(
                        "Potential deadlock pattern: {} -> {} within {}ms",
                        op1,
                        op2,
                        time2.saturating_sub(*time1)
                    ));
                }
            }
        }

        warnings
    }

    /// Generate lock contention report
    pub fn generate_contention_report(
        collector: &ProfileCollector,
    ) -> Result<String, Box<dyn std::error::Error + '_>> {
        let snapshot = collector.get_snapshot()?;
        let arc_metrics = &snapshot.arc_metrics;

        let mut report = String::new();
        report.push_str("=== Arc Contention Report ===\n");
        report.push_str(&format!(
            "Total lock acquisitions: {}\n",
            arc_metrics.lock_acquisitions
        ));
        report.push_str(&format!(
            "Contention events: {}\n",
            arc_metrics.contention_events
        ));
        report.push_str(&format!(
            "Contention rate: {:.2}%\n",
            (arc_metrics.contention_events as f64 / arc_metrics.lock_acquisitions.max(1) as f64)
                * 100.0
        ));
        report.push_str(&format!(
            "Average wait time: {:.2}ms\n",
            arc_metrics.avg_wait_time_ms
        ));
        report.push_str(&format!(
            "Contention rate: {:.2}%\n",
            (arc_metrics.contention_events as f64 / arc_metrics.lock_acquisitions.max(1) as f64)
                * 100.0
        ));
        report.push_str(&format!(
            "Deadlock near misses: {}\n",
            arc_metrics.deadlock_near_misses
        ));

        report.push_str("\n=== Access Patterns ===\n");
        let mut patterns: Vec<_> = arc_metrics.concurrent_access_patterns.iter().collect();
        patterns.sort_by_key(|(_, count)| std::cmp::Reverse(**count));

        for (operation, count) in patterns {
            report.push_str(&format!("{}: {} accesses\n", operation, count));
        }

        // Crisis #2 validation
        report.push_str("\n=== Crisis #2 Validation ===\n");
        let contention_rate = (arc_metrics.contention_events as f64
            / arc_metrics.lock_acquisitions.max(1) as f64)
            * 100.0;
        if contention_rate < 5.0 {
            report.push_str("✅ Low contention rate - Arc architecture working well\n");
        } else if contention_rate < 15.0 {
            report.push_str("⚠️  Moderate contention rate - monitor for issues\n");
        } else {
            report.push_str("❌ High contention rate - Arc architecture may need optimization\n");
        }

        if arc_metrics.avg_wait_time_ms < 10.0 {
            report.push_str("✅ Fast lock acquisition times\n");
        } else {
            report.push_str("⚠️  Slow lock acquisition times\n");
        }

        Ok(report)
    }

    /// Check if the Arc architecture is performing optimally
    pub fn is_arc_healthy(
        collector: &ProfileCollector,
    ) -> Result<bool, Box<dyn std::error::Error + '_>> {
        let snapshot = collector.get_snapshot()?;
        let arc_metrics = &snapshot.arc_metrics;

        // Healthy thresholds based on Crisis #2 resolution expectations
        let contention_rate = (arc_metrics.contention_events as f64
            / arc_metrics.lock_acquisitions.max(1) as f64)
            * 100.0;

        Ok(contention_rate < 5.0 &&  // Less than 5% contention
           arc_metrics.avg_wait_time_ms < 10.0 &&  // Fast acquisitions
           arc_metrics.deadlock_near_misses == 0) // No deadlock risks
    }
}
