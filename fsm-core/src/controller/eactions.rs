// fsm-core/src/controller/eactions.rs - Zero-allocation action system
use std::sync::atomic::{AtomicU8, AtomicU32, AtomicU64, Ordering};

/// Cache-aligned atomic action for zero-allocation dispatch
#[repr(C, align(64))]
pub struct AtomicAction {
    action_type: AtomicU8, // Action discriminant
    param1: AtomicU64,     // First parameter (file ID, path hash, etc.)
    param2: AtomicU64,     // Second parameter
    flags: AtomicU32,      // Operation flags
    _padding: [u8; 43],    // Cache line padding
}

impl AtomicAction {
    pub fn new(action_type: ActionType, p1: u64, p2: u64, flags: u32) -> Self {
        Self {
            action_type: AtomicU8::new(action_type as u8),
            param1: AtomicU64::new(p1),
            param2: AtomicU64::new(p2),
            flags: AtomicU32::new(flags),
            _padding: [0; 43],
        }
    }

    /// Load action atomically without allocations
    #[inline(always)]
    pub fn load_atomic(&self) -> (ActionType, u64, u64, u32) {
        (
            ActionType::from_u8(self.action_type.load(Ordering::Relaxed)),
            self.param1.load(Ordering::Relaxed),
            self.param2.load(Ordering::Relaxed),
            self.flags.load(Ordering::Relaxed),
        )
    }
}

/// Zero-allocation action representation
#[derive(Debug, Clone, Copy)]
pub struct EAction {
    pub action_type: ActionType,
    pub param1: u64,
    pub param2: u64,
    pub flags: u32,
}

impl EAction {
    #[inline(always)]
    pub fn from_atomic(atomic: &AtomicAction) -> Self {
        let (action_type, p1, p2, flags) = atomic.load_atomic();
        Self {
            action_type,
            param1: p1,
            param2: p2,
            flags,
        }
    }
}

/// Memory-efficient action type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ActionType {
    // Clipboard operations (1-9)
    CopyToClipboard = 1,
    MoveToClipboard = 2,
    PasteFromClipboard = 3,

    // Navigation actions (10-19)
    NavigateUp = 10,
    NavigateDown = 11,
    NavigatePageUp = 12,
    NavigatePageDown = 13,
    NavigateHome = 14,
    NavigateEnd = 15,
    EnterDirectory = 16,
    NavigateParent = 17,

    // Command mode actions (20-29)
    EnterCommandMode = 20,
    CommandModeChar = 21,
    CommandModeBackspace = 22,
    CommandModeEnter = 23,
    CommandModeTab = 24,
    CommandModeUpDown = 25,
    ExitCommandMode = 26,

    // Overlay toggles (30-39)
    ToggleClipboardOverlay = 30,
    ToggleFileNameSearch = 31,
    ToggleContentSearch = 32,
    ToggleHelp = 33,
    CloseOverlay = 34,

    // Search mode actions (40-49)
    SearchModeChar = 40,
    SearchModeBackspace = 41,
    SearchModeEnter = 42,
    SearchModeUp = 43,
    SearchModeDown = 44,

    // System actions (50-59)
    Quit = 50,
    NoOp = 51,

    // File operations (60-69)
    FileOpsShowPrompt = 60,
}

impl ActionType {
    #[inline(always)]
    pub fn from_u8(value: u8) -> Self {
        match value {
            // Clipboard operations
            1 => ActionType::CopyToClipboard,
            2 => ActionType::MoveToClipboard,
            3 => ActionType::PasteFromClipboard,

            // Navigation actions
            10 => ActionType::NavigateUp,
            11 => ActionType::NavigateDown,
            12 => ActionType::NavigatePageUp,
            13 => ActionType::NavigatePageDown,
            14 => ActionType::NavigateHome,
            15 => ActionType::NavigateEnd,
            16 => ActionType::EnterDirectory,
            17 => ActionType::NavigateParent,

            // Command mode actions
            20 => ActionType::EnterCommandMode,
            21 => ActionType::CommandModeChar,
            22 => ActionType::CommandModeBackspace,
            23 => ActionType::CommandModeEnter,
            24 => ActionType::CommandModeTab,
            25 => ActionType::CommandModeUpDown,
            26 => ActionType::ExitCommandMode,

            // Overlay toggles
            30 => ActionType::ToggleClipboardOverlay,
            31 => ActionType::ToggleFileNameSearch,
            32 => ActionType::ToggleContentSearch,
            33 => ActionType::ToggleHelp,
            34 => ActionType::CloseOverlay,

            // Search mode actions
            40 => ActionType::SearchModeChar,
            41 => ActionType::SearchModeBackspace,
            42 => ActionType::SearchModeEnter,
            43 => ActionType::SearchModeUp,
            44 => ActionType::SearchModeDown,

            // System actions
            50 => ActionType::Quit,
            51 => ActionType::NoOp,

            // File operations
            60 => ActionType::FileOpsShowPrompt,

            _ => ActionType::NoOp, // Safe fallback
        }
    }
}
