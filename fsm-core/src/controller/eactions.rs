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
    CopyToClipboard = 1,
    MoveToClipboard = 2,
    PasteFromClipboard = 3,
    NavigateUp = 10,
    NavigateDown = 11,
    EnterDirectory = 12,
    // ... other actions
}

impl ActionType {
    #[inline(always)]
    fn from_u8(value: u8) -> Self {
        match value {
            1 => ActionType::CopyToClipboard,
            2 => ActionType::MoveToClipboard,
            3 => ActionType::PasteFromClipboard,
            10 => ActionType::NavigateUp,
            11 => ActionType::NavigateDown,
            12 => ActionType::EnterDirectory,
            _ => ActionType::CopyToClipboard, // Safe fallback
        }
    }
}
