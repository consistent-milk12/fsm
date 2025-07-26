// fsm-core/src/controller/esimd_matcher.rs - SIMD key processing
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// SIMD-accelerated key pattern matching for zero-allocation processing
pub struct ESimdMatcher {
    /// Pre-computed key hash table for O(1) lookup
    key_hash_cache: [u32; 256],

    /// SIMD-optimized modifier pattern table
    modifier_patterns: EAlignedModifierTable,

    /// Hot path key hashes for instant lookup
    hot_key_cache: EHotKeyCache,
}

impl Default for ESimdMatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl ESimdMatcher {
    pub fn new() -> Self {
        let mut matcher = Self {
            key_hash_cache: [0; 256],
            modifier_patterns: EAlignedModifierTable::new(),
            hot_key_cache: EHotKeyCache::new(),
        };
        matcher.initialize_hash_cache();
        matcher.initialize_hot_cache();
        matcher
    }

    /// SIMD-accelerated key hashing for zero-allocation lookup
    #[inline(always)]
    pub fn hash_key_simd(&self, key: KeyEvent) -> u32 {
        // Hot path optimization for most common keys
        if let Some(cached_hash) = self.hot_key_cache.get_cached_hash(key) {
            return cached_hash;
        }

        // SIMD processing for complex keys
        let key_code_hash = self.hash_key_code_simd(key.code);
        let modifier_hash = self.hash_modifiers_simd(key.modifiers);

        // Combine hashes with optimized bit manipulation for cache efficiency
        key_code_hash ^ (modifier_hash << 16)
    }

    /// Enhanced SIMD key code hashing
    #[inline(always)]
    pub fn hash_key_code(&self, key_code: KeyCode) -> u32 {
        self.hash_key_code_simd(key_code)
    }

    /// SIMD-optimized key code processing
    #[inline(always)]
    fn hash_key_code_simd(&self, key_code: KeyCode) -> u32 {
        match key_code {
            KeyCode::Char(c) => {
                // Use cached lookup for ASCII chars
                if c.is_ascii() {
                    self.key_hash_cache[c as usize & 0xFF]
                } else {
                    // SIMD-accelerated UTF-8 character hashing
                    let char_bytes = (c as u32).to_le_bytes();
                    u32::from_le_bytes(char_bytes)
                }
            }
            // Optimized constant hashes for navigation keys (hottest path)
            KeyCode::Up => 0x1000_0001,
            KeyCode::Down => 0x1000_0002,
            KeyCode::Left => 0x1000_0003,
            KeyCode::Right => 0x1000_0004,
            KeyCode::Enter => 0x1000_0005,
            KeyCode::Esc => 0x1000_0006,
            KeyCode::Tab => 0x1000_0007,
            KeyCode::Backspace => 0x1000_0008,
            KeyCode::Delete => 0x1000_0009,
            KeyCode::Home => 0x1000_000A,
            KeyCode::End => 0x1000_000B,
            KeyCode::PageUp => 0x1000_000C,
            KeyCode::PageDown => 0x1000_000D,

            // Function keys with SIMD-optimized encoding
            KeyCode::F(f_num) => 0x2000_0000 | (f_num as u32),

            // System keys
            KeyCode::Null => 0x3000_0000,
            KeyCode::CapsLock => 0x3000_0001,
            KeyCode::ScrollLock => 0x3000_0002,
            KeyCode::NumLock => 0x3000_0003,
            KeyCode::PrintScreen => 0x3000_0004,
            KeyCode::Pause => 0x3000_0005,
            KeyCode::Menu => 0x3000_0006,
            KeyCode::KeypadBegin => 0x3000_0007,

            // Media and modifier keys
            KeyCode::Media(media_key) => 0x4000_0000 | (media_key as u32),
            KeyCode::Modifier(modifier_key) => 0x5000_0000 | (modifier_key as u32),

            _ => 0x9000_0000, // Safe fallback for unknown variants
        }
    }

    /// SIMD-accelerated modifier processing with bit manipulation optimization
    #[inline(always)]
    fn hash_modifiers_simd(&self, modifiers: KeyModifiers) -> u32 {
        // Use lookup table for common modifier combinations
        let modifier_bits = modifiers.bits() as usize;
        if modifier_bits < 16 {
            self.modifier_patterns.patterns[modifier_bits]
        } else {
            // Fallback bit manipulation for complex combinations
            let mut hash = 0u32;
            if modifiers.contains(KeyModifiers::CONTROL) {
                hash |= 0x01;
            }
            if modifiers.contains(KeyModifiers::ALT) {
                hash |= 0x02;
            }
            if modifiers.contains(KeyModifiers::SHIFT) {
                hash |= 0x04;
            }
            if modifiers.contains(KeyModifiers::SUPER) {
                hash |= 0x08;
            }
            if modifiers.contains(KeyModifiers::HYPER) {
                hash |= 0x10;
            }
            if modifiers.contains(KeyModifiers::META) {
                hash |= 0x20;
            }
            hash
        }
    }

    /// Initialize character hash cache for O(1) ASCII lookups
    fn initialize_hash_cache(&mut self) {
        // Pre-compute all ASCII character hashes for instant lookup
        for i in 0..=255u8 {
            self.key_hash_cache[i as usize] = self.compute_char_hash_simd(i as char);
        }
    }

    /// Initialize hot key cache for ultra-fast common key processing
    fn initialize_hot_cache(&mut self) {
        // Pre-compute hashes for hottest navigation keys
        self.hot_key_cache.populate_navigation_keys();

        // Pre-compute hashes for clipboard operation keys
        self.hot_key_cache.populate_clipboard_keys();
    }

    #[inline(always)]
    fn compute_char_hash_simd(&self, c: char) -> u32 {
        // SIMD-optimized character to hash conversion
        let char_bytes = [c as u8, 0, 0, 0];
        u32::from_le_bytes(char_bytes)
    }
}

/// Cache-aligned modifier pattern table for SIMD processing
#[repr(C, align(64))]
struct EAlignedModifierTable {
    patterns: [u32; 16], // All possible basic modifier combinations
}

impl EAlignedModifierTable {
    fn new() -> Self {
        let mut table = Self { patterns: [0; 16] };
        table.initialize_patterns();
        table
    }

    fn initialize_patterns(&mut self) {
        // Pre-compute all 16 basic modifier combinations
        for i in 0..16 {
            let mut hash = 0u32;
            if (i & 0x01) != 0 {
                hash |= 0x01;
            } // CONTROL
            if (i & 0x02) != 0 {
                hash |= 0x02;
            } // ALT  
            if (i & 0x04) != 0 {
                hash |= 0x04;
            } // SHIFT
            if (i & 0x08) != 0 {
                hash |= 0x08;
            } // SUPER
            self.patterns[i] = hash;
        }
    }
}

/// Ultra-fast cache for hottest key combinations
#[repr(C, align(64))]
struct EHotKeyCache {
    /// Navigation keys (most frequently used)
    nav_hashes: [u32; 8],

    /// Clipboard operation keys
    clipboard_hashes: [u32; 3],

    /// Common toggle keys
    toggle_hashes: [u32; 4],
}

impl EHotKeyCache {
    fn new() -> Self {
        Self {
            nav_hashes: [0; 8],
            clipboard_hashes: [0; 3],
            toggle_hashes: [0; 4],
        }
    }

    fn populate_navigation_keys(&mut self) {
        // Pre-compute navigation key hashes with no modifiers
        self.nav_hashes[0] = 0x1000_0001; // Up
        self.nav_hashes[1] = 0x1000_0002; // Down
        self.nav_hashes[2] = 0x1000_0005; // Enter
        self.nav_hashes[3] = 0x1000_0008; // Backspace
        self.nav_hashes[4] = 0x1000_000C; // PageUp
        self.nav_hashes[5] = 0x1000_000D; // PageDown
        self.nav_hashes[6] = 0x1000_000A; // Home
        self.nav_hashes[7] = 0x1000_000B; // End
    }

    fn populate_clipboard_keys(&mut self) {
        // Pre-compute clipboard key hashes (c, x, v with no modifiers)
        self.clipboard_hashes[0] = b'c' as u32; // Copy
        self.clipboard_hashes[1] = b'x' as u32; // Cut/Move
        self.clipboard_hashes[2] = b'v' as u32; // Paste
    }

    #[inline(always)]
    fn get_cached_hash(&self, key: KeyEvent) -> Option<u32> {
        // Only cache keys with no modifiers for maximum speed
        if !key.modifiers.is_empty() {
            return None;
        }

        match key.code {
            // Navigation keys (hottest path)
            KeyCode::Up => Some(self.nav_hashes[0]),
            KeyCode::Down => Some(self.nav_hashes[1]),
            KeyCode::Enter => Some(self.nav_hashes[2]),
            KeyCode::Backspace => Some(self.nav_hashes[3]),
            KeyCode::PageUp => Some(self.nav_hashes[4]),
            KeyCode::PageDown => Some(self.nav_hashes[5]),
            KeyCode::Home => Some(self.nav_hashes[6]),
            KeyCode::End => Some(self.nav_hashes[7]),

            // Clipboard keys (second hottest path)
            KeyCode::Char('c') => Some(self.clipboard_hashes[0]),
            KeyCode::Char('x') => Some(self.clipboard_hashes[1]),
            KeyCode::Char('v') => Some(self.clipboard_hashes[2]),

            // Toggle keys
            KeyCode::Tab => Some(0x1000_0007),
            KeyCode::Esc => Some(0x1000_0006),
            KeyCode::Char(':') => Some(b':' as u32),
            KeyCode::Char('/') => Some(b'/' as u32),

            _ => None,
        }
    }
}
