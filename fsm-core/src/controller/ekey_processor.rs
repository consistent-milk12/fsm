// fsm-core/src/controller/ekey_processor.rs - Extreme performance unified key processor
use crate::controller::eactions::{ActionType, AtomicAction, EAction};
use crate::controller::esimd_matcher::ESimdMatcher;
use crate::model::ui_state::{UIMode, UIOverlay};
use clipr::ClipBoard;
use compact_str::CompactString;
use crossbeam::atomic::AtomicCell;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use lockfree::map::Map as LockFreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

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

    /// Unified zero-allocation key processing with comprehensive context awareness
    #[inline(always)]
    pub fn process_key(
        &self,
        key: KeyEvent,
        ui_mode: UIMode,
        ui_overlay: UIOverlay,
        clipboard_active: bool,
    ) -> Option<EAction> {
        let start_time = Instant::now();

        // Determine if we're in a context where shortcuts should be restricted
        let shortcuts_restricted =
            self.are_shortcuts_restricted(ui_mode, ui_overlay, clipboard_active);

        // Fast path: Try cache only for unrestricted contexts
        if !shortcuts_restricted {
            let key_hash = self.pattern_matcher.hash_key_simd(key);
            if let Some(action_guard) = self.action_cache.get(&key_hash) {
                let cached_action = action_guard.val();
                // Verify cached action is valid for current context
                if self.is_action_valid_for_context(
                    cached_action,
                    ui_mode,
                    ui_overlay,
                    clipboard_active,
                ) {
                    self.stats.inc_cache_hit();
                    let latency_ns = start_time.elapsed().as_nanos() as u64;
                    self.stats.update_latency(latency_ns);
                    return Some(EAction::from_atomic(cached_action));
                }
            }
        }

        // Context-aware key processing (always for restricted contexts)
        let action = self.process_key_with_context(key, ui_mode, ui_overlay, clipboard_active)?;

        if !shortcuts_restricted {
            self.stats.inc_cache_miss();
        }
        let latency_ns = start_time.elapsed().as_nanos() as u64;
        self.stats.update_latency(latency_ns);

        Some(action)
    }

    /// Determine if shortcuts should be restricted (includes both text input and isolated navigation contexts)
    #[inline(always)]
    fn are_shortcuts_restricted(
        &self,
        ui_mode: UIMode,
        ui_overlay: UIOverlay,
        clipboard_active: bool,
    ) -> bool {
        // Clipboard overlay is an isolated navigation context - restricts character shortcuts
        if clipboard_active {
            return true;
        }

        // Command mode and search overlays are text input contexts - restrict all shortcuts
        match ui_mode {
            UIMode::Command => true,
            UIMode::Browse => matches!(
                ui_overlay,
                UIOverlay::ContentSearch | UIOverlay::FileNameSearch | UIOverlay::Prompt
            ),
            UIMode::Search | UIMode::Prompt => true,
            _ => false,
        }
    }

    /// Context-aware key processing for all UI modes
    #[inline]
    fn process_key_with_context(
        &self,
        key: KeyEvent,
        ui_mode: UIMode,
        ui_overlay: UIOverlay,
        clipboard_active: bool,
    ) -> Option<EAction> {
        // Clipboard overlay has highest priority
        if clipboard_active {
            return self.process_clipboard_overlay_key(key);
        }

        // Route based on UI mode and overlay
        match ui_mode {
            UIMode::Command => self.process_command_mode_key(key),
            UIMode::Browse => match ui_overlay {
                UIOverlay::None => self.process_browse_mode_key(key),
                UIOverlay::ContentSearch | UIOverlay::FileNameSearch => {
                    self.process_search_overlay_key(key, ui_overlay)
                }
                UIOverlay::Prompt => self.process_prompt_overlay_key(key),
                _ => None, // Other overlays handled elsewhere
            },
            UIMode::Search => self.process_search_mode_key(key),
            UIMode::Prompt => self.process_prompt_mode_key(key),
            _ => None, // Other modes handled elsewhere
        }
    }

    /// Process clipboard overlay keys (isolated navigation context - no character shortcuts)
    #[inline]
    fn process_clipboard_overlay_key(&self, key: KeyEvent) -> Option<EAction> {
        match key.code {
            // Navigation keys allowed
            KeyCode::Up => Some(EAction {
                action_type: ActionType::NavigateUp,
                param1: 1, // clipboard context flag
                param2: 0,
                flags: 0,
            }),
            KeyCode::Down => Some(EAction {
                action_type: ActionType::NavigateDown,
                param1: 1, // clipboard context flag
                param2: 0,
                flags: 0,
            }),
            KeyCode::PageUp => Some(EAction {
                action_type: ActionType::NavigatePageUp,
                param1: 1, // clipboard context flag
                param2: 0,
                flags: 0,
            }),
            KeyCode::PageDown => Some(EAction {
                action_type: ActionType::NavigatePageDown,
                param1: 1, // clipboard context flag
                param2: 0,
                flags: 0,
            }),
            KeyCode::Home => Some(EAction {
                action_type: ActionType::NavigateHome,
                param1: 1, // clipboard context flag
                param2: 0,
                flags: 0,
            }),
            KeyCode::End => Some(EAction {
                action_type: ActionType::NavigateEnd,
                param1: 1, // clipboard context flag
                param2: 0,
                flags: 0,
            }),

            // Selection and control keys allowed
            KeyCode::Enter => Some(EAction {
                action_type: ActionType::EnterDirectory,
                param1: 1, // clipboard context flag - represents paste/select action
                param2: 0,
                flags: 0,
            }),
            KeyCode::Tab => Some(EAction {
                action_type: ActionType::ToggleClipboardOverlay,
                param1: 0,
                param2: 0,
                flags: 0,
            }),
            KeyCode::Esc => Some(EAction {
                action_type: ActionType::CloseOverlay,
                param1: 0,
                param2: 0,
                flags: 0,
            }),

            // ALL character keys (c, x, v, q, m, r, h, etc.) are blocked in clipboard overlay
            // This prevents accidental shortcuts while navigating the clipboard
            KeyCode::Char(_) => None,

            // Function keys and other special keys blocked
            _ => None,
        }
    }

    /// Process command mode keys (text input context - no shortcuts except control keys)
    #[inline]
    fn process_command_mode_key(&self, key: KeyEvent) -> Option<EAction> {
        match key.code {
            KeyCode::Char(c) => Some(EAction {
                action_type: ActionType::CommandModeChar,
                param1: c as u64,
                param2: 0,
                flags: 0,
            }),
            KeyCode::Backspace => Some(EAction {
                action_type: ActionType::CommandModeBackspace,
                param1: 0,
                param2: 0,
                flags: 0,
            }),
            KeyCode::Enter => Some(EAction {
                action_type: ActionType::CommandModeEnter,
                param1: 0,
                param2: 0,
                flags: 0,
            }),
            KeyCode::Tab => Some(EAction {
                action_type: ActionType::CommandModeTab,
                param1: 0,
                param2: 0,
                flags: 0,
            }),
            KeyCode::Up | KeyCode::Down => Some(EAction {
                action_type: ActionType::CommandModeUpDown,
                param1: if key.code == KeyCode::Up { 0 } else { 1 },
                param2: 0,
                flags: 0,
            }),
            KeyCode::Esc => Some(EAction {
                action_type: ActionType::ExitCommandMode,
                param1: 0,
                param2: 0,
                flags: 0,
            }),
            // All other keys (including 'c', 'x', 'v', 'q', etc.) are treated as text input
            _ => None,
        }
    }

    /// Process browse mode keys (navigation and shortcuts - only when no overlays active)
    #[inline]
    fn process_browse_mode_key(&self, key: KeyEvent) -> Option<EAction> {
        match (key.code, key.modifiers) {
            // Command and overlay toggles
            (KeyCode::Char(':'), _) => Some(EAction {
                action_type: ActionType::EnterCommandMode,
                param1: 0,
                param2: 0,
                flags: 0,
            }),
            (KeyCode::Char('/'), _) => Some(EAction {
                action_type: ActionType::ToggleFileNameSearch,
                param1: 0,
                param2: 0,
                flags: 0,
            }),
            (KeyCode::Tab, _) => Some(EAction {
                action_type: ActionType::ToggleClipboardOverlay,
                param1: 0,
                param2: 0,
                flags: 0,
            }),
            (KeyCode::Char('h'), KeyModifiers::NONE) | (KeyCode::Char('?'), _) => Some(EAction {
                action_type: ActionType::ToggleHelp,
                param1: 0,
                param2: 0,
                flags: 0,
            }),

            // Navigation keys
            (KeyCode::Up, _) => Some(EAction {
                action_type: ActionType::NavigateUp,
                param1: 0,
                param2: 0,
                flags: 0,
            }),
            (KeyCode::Down, _) => Some(EAction {
                action_type: ActionType::NavigateDown,
                param1: 0,
                param2: 0,
                flags: 0,
            }),
            (KeyCode::PageUp, _) => Some(EAction {
                action_type: ActionType::NavigatePageUp,
                param1: 0,
                param2: 0,
                flags: 0,
            }),
            (KeyCode::PageDown, _) => Some(EAction {
                action_type: ActionType::NavigatePageDown,
                param1: 0,
                param2: 0,
                flags: 0,
            }),
            (KeyCode::Home, _) => Some(EAction {
                action_type: ActionType::NavigateHome,
                param1: 0,
                param2: 0,
                flags: 0,
            }),
            (KeyCode::End, _) => Some(EAction {
                action_type: ActionType::NavigateEnd,
                param1: 0,
                param2: 0,
                flags: 0,
            }),
            (KeyCode::Enter, _) => Some(EAction {
                action_type: ActionType::EnterDirectory,
                param1: 0,
                param2: 0,
                flags: 0,
            }),
            (KeyCode::Backspace, _) => Some(EAction {
                action_type: ActionType::NavigateParent,
                param1: 0,
                param2: 0,
                flags: 0,
            }),

            // Clipboard operations (c/x/v) - only in pure browse mode
            (KeyCode::Char('c'), _) => Some(EAction {
                action_type: ActionType::CopyToClipboard,
                param1: 0,
                param2: 0,
                flags: 0,
            }),
            (KeyCode::Char('x'), _) => Some(EAction {
                action_type: ActionType::MoveToClipboard,
                param1: 0,
                param2: 0,
                flags: 0,
            }),
            (KeyCode::Char('v'), _) => Some(EAction {
                action_type: ActionType::ToggleClipboardOverlay,
                param1: 0,
                param2: 0,
                flags: 0,
            }),

            // File operations shortcuts
            (KeyCode::Char('m'), _) | (KeyCode::Char('r'), _) => Some(EAction {
                action_type: ActionType::FileOpsShowPrompt,
                param1: match key.code {
                    KeyCode::Char(c) => c as u64,
                    _ => 0,
                },
                param2: 0,
                flags: 0,
            }),

            // System controls
            (KeyCode::Char('q'), _) => Some(EAction {
                action_type: ActionType::Quit,
                param1: 0,
                param2: 0,
                flags: 0,
            }),

            _ => None,
        }
    }

    /// Process search overlay keys (text input context - no shortcuts)
    #[inline]
    fn process_search_overlay_key(&self, key: KeyEvent, overlay: UIOverlay) -> Option<EAction> {
        let overlay_flag = match overlay {
            UIOverlay::ContentSearch => 1,
            UIOverlay::FileNameSearch => 2,
            _ => 0,
        };

        match key.code {
            // All character keys are treated as search input
            KeyCode::Char(c) => Some(EAction {
                action_type: ActionType::SearchModeChar,
                param1: c as u64,
                param2: overlay_flag,
                flags: 0,
            }),
            KeyCode::Backspace => Some(EAction {
                action_type: ActionType::SearchModeBackspace,
                param1: 0,
                param2: overlay_flag,
                flags: 0,
            }),
            KeyCode::Enter => Some(EAction {
                action_type: ActionType::SearchModeEnter,
                param1: 0,
                param2: overlay_flag,
                flags: 0,
            }),
            KeyCode::Up => Some(EAction {
                action_type: ActionType::SearchModeUp,
                param1: 0,
                param2: overlay_flag,
                flags: 0,
            }),
            KeyCode::Down => Some(EAction {
                action_type: ActionType::SearchModeDown,
                param1: 0,
                param2: overlay_flag,
                flags: 0,
            }),
            KeyCode::Esc => Some(EAction {
                action_type: ActionType::CloseOverlay,
                param1: 0,
                param2: 0,
                flags: 0,
            }),
            // All other keys (including Tab, function keys, etc.) ignored in search context
            _ => None,
        }
    }

    /// Process prompt overlay keys (text input context - similar to search)
    #[inline]
    fn process_prompt_overlay_key(&self, key: KeyEvent) -> Option<EAction> {
        match key.code {
            KeyCode::Char(c) => Some(EAction {
                action_type: ActionType::SearchModeChar,
                param1: c as u64,
                param2: 3, // prompt flag
                flags: 0,
            }),
            KeyCode::Backspace => Some(EAction {
                action_type: ActionType::SearchModeBackspace,
                param1: 0,
                param2: 3,
                flags: 0,
            }),
            KeyCode::Enter => Some(EAction {
                action_type: ActionType::SearchModeEnter,
                param1: 0,
                param2: 3,
                flags: 0,
            }),
            KeyCode::Esc => Some(EAction {
                action_type: ActionType::CloseOverlay,
                param1: 0,
                param2: 0,
                flags: 0,
            }),
            _ => None,
        }
    }

    /// Process search mode keys (when UIMode is Search)
    #[inline]
    fn process_search_mode_key(&self, key: KeyEvent) -> Option<EAction> {
        match key.code {
            KeyCode::Char(c) => Some(EAction {
                action_type: ActionType::SearchModeChar,
                param1: c as u64,
                param2: 0, // generic search mode
                flags: 0,
            }),
            KeyCode::Backspace => Some(EAction {
                action_type: ActionType::SearchModeBackspace,
                param1: 0,
                param2: 0,
                flags: 0,
            }),
            KeyCode::Enter => Some(EAction {
                action_type: ActionType::SearchModeEnter,
                param1: 0,
                param2: 0,
                flags: 0,
            }),
            KeyCode::Up => Some(EAction {
                action_type: ActionType::SearchModeUp,
                param1: 0,
                param2: 0,
                flags: 0,
            }),
            KeyCode::Down => Some(EAction {
                action_type: ActionType::SearchModeDown,
                param1: 0,
                param2: 0,
                flags: 0,
            }),
            KeyCode::Esc => Some(EAction {
                action_type: ActionType::CloseOverlay,
                param1: 0,
                param2: 0,
                flags: 0,
            }),
            _ => None,
        }
    }

    /// Process prompt mode keys (when UIMode is Prompt)
    #[inline]
    fn process_prompt_mode_key(&self, key: KeyEvent) -> Option<EAction> {
        match key.code {
            KeyCode::Char(c) => Some(EAction {
                action_type: ActionType::SearchModeChar,
                param1: c as u64,
                param2: 4, // prompt mode flag
                flags: 0,
            }),
            KeyCode::Backspace => Some(EAction {
                action_type: ActionType::SearchModeBackspace,
                param1: 0,
                param2: 4,
                flags: 0,
            }),
            KeyCode::Enter => Some(EAction {
                action_type: ActionType::SearchModeEnter,
                param1: 0,
                param2: 4,
                flags: 0,
            }),
            KeyCode::Esc => Some(EAction {
                action_type: ActionType::CloseOverlay,
                param1: 0,
                param2: 0,
                flags: 0,
            }),
            _ => None,
        }
    }

    /// Enhanced context validation that respects text input contexts
    #[inline]
    fn is_action_valid_for_context(
        &self,
        action: &AtomicAction,
        ui_mode: UIMode,
        ui_overlay: UIOverlay,
        clipboard_active: bool,
    ) -> bool {
        let (action_type, _, _, _) = action.load_atomic();

        // Clipboard overlay keys are context-sensitive (navigation allowed)
        if clipboard_active {
            return matches!(
                action_type,
                ActionType::NavigateUp
                    | ActionType::NavigateDown
                    | ActionType::EnterDirectory
                    | ActionType::CloseOverlay
            );
        }

        // Command mode - only command-specific actions allowed
        if ui_mode == UIMode::Command {
            return matches!(
                action_type,
                ActionType::CommandModeChar
                    | ActionType::CommandModeBackspace
                    | ActionType::CommandModeEnter
                    | ActionType::CommandModeTab
                    | ActionType::CommandModeUpDown
                    | ActionType::ExitCommandMode
            );
        }

        // Search overlays - only search-specific actions allowed (NO shortcuts)
        if matches!(
            ui_overlay,
            UIOverlay::ContentSearch | UIOverlay::FileNameSearch | UIOverlay::Prompt
        ) {
            return matches!(
                action_type,
                ActionType::SearchModeChar
                    | ActionType::SearchModeBackspace
                    | ActionType::SearchModeEnter
                    | ActionType::SearchModeUp
                    | ActionType::SearchModeDown
                    | ActionType::CloseOverlay
            );
        }

        // Search/Prompt modes - only search-specific actions allowed
        if matches!(ui_mode, UIMode::Search | UIMode::Prompt) {
            return matches!(
                action_type,
                ActionType::SearchModeChar
                    | ActionType::SearchModeBackspace
                    | ActionType::SearchModeEnter
                    | ActionType::SearchModeUp
                    | ActionType::SearchModeDown
                    | ActionType::CloseOverlay
            );
        }

        // Browse mode with no overlays - all shortcuts allowed
        if ui_mode == UIMode::Browse && ui_overlay == UIOverlay::None {
            return matches!(
                action_type,
                ActionType::CopyToClipboard
                    | ActionType::MoveToClipboard
                    | ActionType::PasteFromClipboard
                    | ActionType::NavigateUp
                    | ActionType::NavigateDown
                    | ActionType::NavigatePageUp
                    | ActionType::NavigatePageDown
                    | ActionType::NavigateHome
                    | ActionType::NavigateEnd
                    | ActionType::EnterDirectory
                    | ActionType::NavigateParent
                    | ActionType::ToggleClipboardOverlay
                    | ActionType::ToggleFileNameSearch
                    | ActionType::ToggleHelp
                    | ActionType::EnterCommandMode
                    | ActionType::FileOpsShowPrompt
                    | ActionType::Quit
            );
        }

        // Default: no cached actions allowed in unknown contexts
        false
    }

    /// Pre-populate action cache for zero-allocation runtime (only safe contexts)
    fn initialize_action_cache(&mut self) {
        // Only cache actions for browse mode with no overlays (safest context)
        // Navigation actions (always safe and most frequently used)
        self.insert_cached_action(KeyCode::Up, ActionType::NavigateUp, 0, 0);
        self.insert_cached_action(KeyCode::Down, ActionType::NavigateDown, 0, 0);
        self.insert_cached_action(KeyCode::Enter, ActionType::EnterDirectory, 0, 0);
        self.insert_cached_action(KeyCode::PageUp, ActionType::NavigatePageUp, 0, 0);
        self.insert_cached_action(KeyCode::PageDown, ActionType::NavigatePageDown, 0, 0);
        self.insert_cached_action(KeyCode::Home, ActionType::NavigateHome, 0, 0);
        self.insert_cached_action(KeyCode::End, ActionType::NavigateEnd, 0, 0);
        self.insert_cached_action(KeyCode::Backspace, ActionType::NavigateParent, 0, 0);

        // System toggles (generally safe)
        self.insert_cached_action(KeyCode::Char(':'), ActionType::EnterCommandMode, 0, 0);
        self.insert_cached_action(KeyCode::Char('/'), ActionType::ToggleFileNameSearch, 0, 0);
        self.insert_cached_action(KeyCode::Tab, ActionType::ToggleClipboardOverlay, 0, 0);
        self.insert_cached_action(KeyCode::Char('h'), ActionType::ToggleHelp, 0, 0);
        self.insert_cached_action(KeyCode::Char('q'), ActionType::Quit, 0, 0);

        // Clipboard operations (only safe in pure browse mode)
        // Note: These will be validated by context checking
        self.insert_cached_action(KeyCode::Char('c'), ActionType::CopyToClipboard, 0, 0);
        self.insert_cached_action(KeyCode::Char('x'), ActionType::MoveToClipboard, 0, 0);
        self.insert_cached_action(KeyCode::Char('v'), ActionType::ToggleClipboardOverlay, 0, 0);

        // File operations
        self.insert_cached_action(
            KeyCode::Char('m'),
            ActionType::FileOpsShowPrompt,
            b'm' as u64,
            0,
        );
        self.insert_cached_action(
            KeyCode::Char('r'),
            ActionType::FileOpsShowPrompt,
            b'r' as u64,
            0,
        );
    }

    #[inline]
    fn insert_cached_action(&mut self, key: KeyCode, action_type: ActionType, p1: u64, p2: u64) {
        let key_hash = self.pattern_matcher.hash_key_code(key);
        let atomic_action = AtomicAction::new(action_type, p1, p2, 0);
        self.action_cache.insert(key_hash, atomic_action);
    }

    /// Get current performance statistics
    pub fn get_performance_stats(&self) -> (f64, u64, u64) {
        (
            self.stats.cache_hit_rate(),
            self.stats.total_keys.load(Ordering::Relaxed),
            self.stats.avg_latency_ns.load(Ordering::Relaxed),
        )
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
        // Exponential moving average for better performance characteristics
        let current_avg = self.avg_latency_ns.load(Ordering::Relaxed);
        let alpha = 10; // 0.1 * 100 for integer math
        let new_avg = if current_avg == 0 {
            latency_ns
        } else {
            (current_avg * (100 - alpha) + latency_ns * alpha) / 100
        };
        self.avg_latency_ns.store(new_avg, Ordering::Relaxed);
    }

    /// Get total processed keys
    pub fn total_keys(&self) -> u64 {
        self.total_keys.load(Ordering::Relaxed)
    }

    /// Get average latency in nanoseconds
    pub fn avg_latency_ns(&self) -> u64 {
        self.avg_latency_ns.load(Ordering::Relaxed)
    }
}
