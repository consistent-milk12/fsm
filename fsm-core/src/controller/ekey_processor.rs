// fsm-core/src/controller/ekey_processor.rs - New performance module
use crate::controller::eactions::{ActionType, AtomicAction, EAction};
use crate::controller::esimd_matcher::ESimdMatcher;
use clipr::ClipBoard;
use compact_str::CompactString;
use crossbeam::atomic::AtomicCell;
use crossterm::event::KeyEvent;
use lockfree::map::Map as LockFreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Performance key processor with sub-microsecond response times
#[repr(align(64))]
pub struct EKeyProcessor {
    /// Pre-computed action lookup for zero-allocation dispatch
    action_cache: LockFreeMap<u32, AtomicAction>,

    /// Lock-free performance statistics
    pub stats: EKeyStats,

    /// SIMD-optimized key pattern matcher
    pattern_matcher: ESimdMatcher,

    /// Zero-copy clipboard integration
    pub clipboard: Arc<ClipBoard>,

    /// Cache-aligned current directory (hot path optimization)
    #[allow(dead_code)]
    current_dir_cache: AtomicCell<CompactString>,
}

impl EKeyProcessor {
    /// Initialize processor with pre-computed action cache
    pub fn new(clipboard: Arc<ClipBoard>) -> Self {
        let mut processor = Self {
            action_cache: LockFreeMap::new(),
            stats: EKeyStats::new(),
            pattern_matcher: ESimdMatcher::new(),
            clipboard,
            current_dir_cache: AtomicCell::new(CompactString::new("")),
        };

        // Pre-populate action cache to eliminate runtime allocations
        processor.initialize_action_cache();
        processor
    }

    /// Zero-allocation key processing with SIMD acceleration
    #[inline(always)]
    pub fn process_key(&self, key: KeyEvent) -> Option<EAction> {
        // SIMD-accelerated key pattern matching
        let key_hash = self.pattern_matcher.hash_key_simd(key);

        // Lock-free action lookup with zero allocations
        if let Some(action_guard) = self.action_cache.get(&key_hash) {
            let action = action_guard.val();
            self.stats.inc_cache_hit();

            // Atomic action construction without heap allocation
            Some(EAction::from_atomic(action))
        } else {
            self.stats.inc_cache_miss();
            None
        }
    }

    /// Pre-populate action cache for zero-allocation runtime
    fn initialize_action_cache(&mut self) {
        use crossterm::event::KeyCode;

        // Pre-compute all clipboard actions
        self.insert_cached_action(KeyCode::Char('c'), ActionType::CopyToClipboard, 0, 0);
        self.insert_cached_action(KeyCode::Char('x'), ActionType::MoveToClipboard, 0, 0);
        self.insert_cached_action(KeyCode::Char('v'), ActionType::PasteFromClipboard, 0, 0);

        // Pre-compute navigation actions for hot path optimization
        self.insert_cached_action(KeyCode::Up, ActionType::NavigateUp, 0, 0);
        self.insert_cached_action(KeyCode::Down, ActionType::NavigateDown, 0, 0);
        self.insert_cached_action(KeyCode::Enter, ActionType::EnterDirectory, 0, 0);
    }

    #[inline]
    fn insert_cached_action(
        &mut self,
        key: crossterm::event::KeyCode,
        action_type: ActionType,
        p1: u64,
        p2: u64,
    ) {
        let key_hash = self.pattern_matcher.hash_key_code(key);
        let atomic_action = AtomicAction::new(action_type, p1, p2, 0);
        self.action_cache.insert(key_hash, atomic_action);
    }
}

/// Lock-free atomic statistics for performance monitoring
#[derive(Debug)]
pub struct EKeyStats {
    total_keys: AtomicU64,
    cache_hits: AtomicU64,
    cache_misses: AtomicU64,
    avg_latency_ns: AtomicU64,
}

impl EKeyStats {
    fn new() -> Self {
        Self {
            total_keys: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
            avg_latency_ns: AtomicU64::new(0),
        }
    }

    #[inline(always)]
    fn inc_cache_hit(&self) {
        self.cache_hits.fetch_add(1, Ordering::Relaxed);
        self.total_keys.fetch_add(1, Ordering::Relaxed);
    }

    #[inline(always)]
    fn inc_cache_miss(&self) {
        self.cache_misses.fetch_add(1, Ordering::Relaxed);
        self.total_keys.fetch_add(1, Ordering::Relaxed);
    }

    /// Get cache hit rate for performance monitoring
    pub fn cache_hit_rate(&self) -> f64 {
        let hits = self.cache_hits.load(Ordering::Relaxed) as f64;
        let total = self.total_keys.load(Ordering::Relaxed) as f64;
        if total > 0.0 { hits / total } else { 0.0 }
    }

    #[inline(always)]
    pub fn update_latency(&self, latency_ns: u64) {
        // This is a simplification. A real implementation would use a more sophisticated
        // algorithm to calculate a moving average without contention.
        let current_avg = self.avg_latency_ns.load(Ordering::Relaxed);
        let new_avg = (current_avg + latency_ns) / 2;
        self.avg_latency_ns.store(new_avg, Ordering::Relaxed);
    }
}
