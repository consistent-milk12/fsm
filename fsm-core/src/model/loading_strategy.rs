//! `src/model/loading_strategy.rs`
//! ============================================================================
//! # Smoothed-K Adaptive Loading Strategy
//!
//! Implements the smoothed-K algorithm for adaptive batch loading with
//! exponential smoothing to maintain 60 FPS performance under varying loads.

use enum_map::{EnumMap, enum_map};
use std::time::Duration;
use super::fs_state::EntrySort;

/// Smoothed-K loading strategy constants
const ALPHA: f64 = 0.25;      // exponential smoothing factor
const K_INIT: f64 = 0.5;      // conservative µs per N·lgN (microseconds)

/// Adaptive loading strategy using smoothed-K algorithm
/// 
/// Maintains per-sort-mode cost estimates and dynamically adjusts
/// batch sizes to stay within frame budget (60 FPS = 16.67ms)
#[derive(Debug, Clone)]
pub struct SmoothedKStrategy {
    /// Cost estimates per sort mode (µs per N·lgN operation)
    pub k_map: EnumMap<EntrySort, f64>,
    /// Maximum time budget in microseconds (16.67ms for 60 FPS)
    pub max_budget_us: u64,
}

impl Default for SmoothedKStrategy {
    fn default() -> Self {
        Self {
            k_map: enum_map! {
                EntrySort::NameAsc => K_INIT,
                EntrySort::NameDesc => K_INIT,
                EntrySort::SizeAsc => K_INIT,
                EntrySort::SizeDesc => K_INIT,
                EntrySort::ModifiedAsc => K_INIT,
                EntrySort::ModifiedDesc => K_INIT,
                EntrySort::Custom => K_INIT,
            },
            max_budget_us: 16_670, // 60 FPS budget in microseconds
        }
    }
}

impl SmoothedKStrategy {
    /// Create new strategy with custom frame budget
    #[expect(clippy::cast_possible_truncation, reason = "Expected")]
    #[expect(clippy::cast_sign_loss, reason = "Expected")]
    #[must_use] 
    pub fn new(frame_budget_ms: f64) -> Self {
        Self {
            k_map: enum_map! {
                EntrySort::NameAsc => K_INIT,
                
                EntrySort::NameDesc => K_INIT,
                
                EntrySort::SizeAsc => K_INIT,
                
                EntrySort::SizeDesc => K_INIT,
                
                EntrySort::ModifiedAsc => K_INIT,
                
                EntrySort::ModifiedDesc => K_INIT,
                
                EntrySort::Custom => K_INIT,
            },

            max_budget_us: (frame_budget_ms * 1000.0) as u64,
        }
    }

    /// Determine if current buffer should be flushed based on cost estimate
    #[expect(clippy::cast_possible_truncation, reason = "Expected")]
    #[expect(clippy::cast_precision_loss, reason = "Expected")]
    #[expect(clippy::cast_sign_loss, reason = "Expected")]
    #[must_use] 
    pub fn should_flush(&self, entry_count: usize, sort_mode: EntrySort) -> bool {
        let n = entry_count as f64;
        
        // Handle edge cases: never flush empty/single entry buffers
        if n <= 1.0 {
            return n >= 1.0; // only flush when n == 1, never when n == 0
        }

        let k = self.k_map[sort_mode];
        let estimate = k * n * n.log2();
        estimate as u64 >= self.max_budget_us
    }

    /// Update cost estimate based on actual sort performance
    #[expect(clippy::cast_precision_loss, reason = "Expected")]
    pub fn register_sort_time(
        &mut self,
        entry_count: usize,
        sort_mode: EntrySort,
        duration: Duration,
    ) {
        let n: f64 = entry_count as f64;
        let duration_us: f64 = duration.as_micros() as f64;

        // Safe division handling for edge cases
        let measured_k: f64 = if n <= 1.0 {
            duration_us // linear fallback for tiny samples
        } else {
            duration_us / (n * n.log2())
        };

        // Exponential smoothing: new_estimate = α * measurement + (1-α) * old_estimate
        let k_ref: &mut f64 = &mut self.k_map[sort_mode];
        *k_ref = ALPHA.mul_add(measured_k, (1.0 - ALPHA) * *k_ref);
    }

    /// Get current cost estimate for a sort mode
    #[must_use] 
    pub fn get_cost_estimate(&self, sort_mode: EntrySort) -> f64 {
        self.k_map[sort_mode]
    }

    /// Predict sort time for given entry count and sort mode
    #[expect(clippy::cast_possible_truncation, reason = "Expected")]
    #[expect(clippy::cast_precision_loss, reason = "Expected")]
    #[expect(clippy::cast_sign_loss, reason = "Expected")]
    #[must_use] 
    pub fn predict_sort_time(&self, entry_count: usize, sort_mode: EntrySort) -> Duration {
        let n: f64 = entry_count as f64;
        
        if n <= 1.0 {
            return Duration::from_micros(0);
        }

        let k: f64 = self.k_map[sort_mode];
        let predicted_us: f64 = k * n * n.log2();
        Duration::from_micros(predicted_us as u64)
    }

    /// Set custom frame budget
    #[expect(clippy::cast_possible_truncation, reason = "Expected")]
    #[expect(clippy::cast_sign_loss, reason = "Expected")]
    pub fn set_frame_budget(&mut self, budget_ms: f64) {
        self.max_budget_us = (budget_ms * 1000.0) as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_flush_edge_cases() {
        let strategy = SmoothedKStrategy::default();
        
        // Edge case: empty buffer should never flush
        assert!(!strategy.should_flush(0, EntrySort::NameAsc));
        
        // Edge case: single entry should flush (n >= 1.0)
        assert!(strategy.should_flush(1, EntrySort::NameAsc));
    }

    #[test]
    fn test_register_sort_time_edge_cases() {
        let mut strategy = SmoothedKStrategy::default();
        
        // Test n=1 case (should use linear fallback)
        strategy.register_sort_time(1, EntrySort::NameAsc, Duration::from_micros(100));
        
        
        // Test n=0 case (should use linear fallback)
        strategy.register_sort_time(0, EntrySort::NameAsc, Duration::from_micros(50));
        
        // Should not panic or produce invalid values
        assert!(strategy.k_map[EntrySort::NameAsc].is_finite());
    }

    #[test]
    fn test_predict_sort_time() {
        let strategy: SmoothedKStrategy = SmoothedKStrategy::default();
        
        // Small arrays should return minimal time
        assert_eq!(strategy.predict_sort_time(0, EntrySort::NameAsc), Duration::from_micros(0));
        assert_eq!(strategy.predict_sort_time(1, EntrySort::NameAsc), Duration::from_micros(0));
        
        // Larger arrays should have predictable scaling
        let time_100: Duration = strategy.predict_sort_time(100, EntrySort::NameAsc);
        let time_1000: Duration = strategy.predict_sort_time(1000, EntrySort::NameAsc);
        
        assert!(time_1000 > time_100);
    }

    #[test]
    fn test_exponential_smoothing() {
        let mut strategy = SmoothedKStrategy::default();
        let initial_k = strategy.k_map[EntrySort::NameAsc];
        
        // Simulate multiple measurements
        for _ in 0..5 {
            strategy.register_sort_time(100, EntrySort::NameAsc, Duration::from_micros(1000));
        }
        
        let final_k = strategy.k_map[EntrySort::NameAsc];
        
        // K should converge towards measured value but not equal it due to smoothing
        assert!(final_k > initial_k); // Should trend towards measured cost
    }
}