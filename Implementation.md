# FSM Implementation Specification

**ACTIVE FEATURE:** Phase 3.3: Clipboard Overlay UI with Extreme Performance Rendering

## 1. Executive Summary
**Objective:** Implement high-performance clipboard overlay UI for viewing and selecting clipboard items with zero-allocation rendering  
**Priority:** High (completes clipboard UX with performance-optimized visualization)  
**Complexity:** Medium (UI component with performance constraints)  
**Dependencies:** Phase 3.2 Extreme Performance Copy/Move Operations (✅ Complete)  
**Estimated Effort:** 1-2 development sessions focusing on zero-allocation UI rendering
**Current Status:** 🚀 Ready for performance-optimized UI implementation

## 2. Context & Background
**Problem:** Users need to view and select from clipboard items but current implementation lacks UI visibility  
**Current State:** Phase 3.2 complete - c/x/v keys work with extreme performance but no visual feedback  
**Architecture Foundation:** Production-ready extreme performance infrastructure with Arc<ClipBoard> sharing
**Required:** Tab-toggled overlay UI with zero-allocation rendering and <100µs performance target  
**Integration Point:** Existing EKeyProcessor + clipr crate + heapless UI components

### Phase 3.2 Foundation Available
- **EKeyProcessor**: Zero-allocation key processing with sub-microsecond response times
- **Arc<ClipBoard>**: Thread-safe clipboard sharing between UI and processor
- **Lock-Free Operations**: clipr crate with get_all_items() and clear_on_paste() methods
- **Heapless Infrastructure**: heapless::String patterns established in status_bar.rs
- **Performance Monitoring**: Built-in cache hit rates and latency tracking  

## 3. Performance & UX Design Decisions

### ADR-008: Zero-Allocation UI Rendering Architecture (2024-07-25)
**Status:** Accepted  
**Context:** UI rendering can become performance bottleneck with string allocations and inefficient layout calculations  
**Decision:** Implement zero-allocation rendering using heapless strings and pre-computed layout caches  
**Performance Targets:**
- **Render Time**: <100µs for full overlay refresh
- **Memory Allocations**: Zero allocations during UI updates
- **Layout Calculations**: Pre-computed and cached for instant display
- **String Operations**: heapless::String for all text formatting

**Consequences:**
- ✅ Consistent sub-millisecond UI responsiveness
- ✅ Zero garbage collection pressure during rendering
- ✅ Predictable memory usage patterns
- ✅ Smooth 120fps+ rendering capability
- ⚠️ Complex UI component architecture
- ⚠️ Limited dynamic text length (heapless constraints)

## 4. Success Criteria
### Must Have (P0) - Core Functionality
- [ ] **Clipboard Overlay Toggle**: Tab key opens/closes clipboard overlay
- [ ] **Item List Display**: Shows all clipboard items with metadata (path, type, size, date)
- [ ] **Item Selection**: Arrow keys navigate, Enter selects for paste operation
- [ ] **Visual Indicators**: Clear distinction between Copy (blue) and Move (yellow) operations
- [ ] **Performance Metrics**: Real-time clipboard statistics display
- [ ] **Responsive Layout**: Adapts to terminal size with intelligent truncation

### Should Have (P1) - Enhanced UX
- [ ] **Zero-Allocation Rendering**: No heap allocations during UI updates
- [ ] **Instant Response**: <100µs render time for overlay updates
- [ ] **Smart Truncation**: Intelligent path shortening for optimal display
- [ ] **Status Integration**: Clipboard count in main status bar
- [ ] **Clear Empty State**: Helpful message when clipboard is empty
- [ ] **Consistent Styling**: Matches existing UI component patterns

### Could Have (P2) - Advanced Features
- [ ] **Item Preview**: Preview pane showing file contents/metadata
- [ ] **Batch Selection**: Multi-select for batch operations
- [ ] **Search Filter**: Quick search within clipboard items
- [ ] **Sort Options**: Sort by date, size, name, or type

## 5. Technical Approach
**Architecture:** Zero-allocation component with pre-computed layouts and heapless string formatting  
**Rendering:** Cache-aligned data structures with minimal ratatui widget overhead  
**Performance:** Sub-100µs render times with zero memory allocations  
**Integration:** Tab key toggle with existing overlay system  

## Implementation Specification

### 1. Zero-Allocation Clipboard Overlay Component
```rust
// fsm-core/src/view/components/clipboard_overlay.rs - New high-performance component
use heapless::{String as HeaplessString, Vec as HeaplessVec};
use ratatui::{prelude::*, widgets::*};

/// Zero-allocation clipboard overlay with sub-100µs render times
pub struct ClipboardOverlay {
    /// Pre-allocated text buffers to eliminate runtime allocations
    item_text_cache: HeaplessVec<HeaplessString<256>, 32>,
    
    /// Pre-computed layout rectangles for instant positioning
    layout_cache: LayoutCache,
    
    /// Current selection index (atomic for thread safety)
    selected_index: usize,
    
    /// Performance metrics for optimization
    render_stats: RenderStats,
}

impl ClipboardOverlay {
    /// Initialize overlay with pre-allocated buffers
    pub fn new() -> Self {
        Self {
            item_text_cache: HeaplessVec::new(),
            layout_cache: LayoutCache::new(),
            selected_index: 0,
            render_stats: RenderStats::new(),
        }
    }
    
    /// Zero-allocation rendering with performance monitoring
    pub fn render_zero_alloc(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        clipboard: &clipr::ClipBoard,
        selected_index: usize,
    ) -> Result<(), UIError> {
        let start_time = std::time::Instant::now();
        
        // Pre-compute layout to avoid runtime calculations
        let layout = self.layout_cache.get_or_compute(area);
        
        // Update selection with bounds checking
        self.selected_index = selected_index.min(clipboard.len().saturating_sub(1));
        
        // Render based on clipboard state
        if clipboard.is_empty() {
            self.render_empty_state(frame, layout.main_area);
        } else {
            self.render_clipboard_items(frame, layout, clipboard)?;
        }
        
        // Update performance metrics
        let render_time = start_time.elapsed();
        self.render_stats.record_render_time(render_time);
        
        Ok(())
    }
    
    /// Render clipboard items with zero allocations
    fn render_clipboard_items(
        &mut self,
        frame: &mut Frame,
        layout: &PrecomputedLayout,
        clipboard: &clipr::ClipBoard,
    ) -> Result<(), UIError> {
        // Clear text cache for reuse
        self.item_text_cache.clear();
        
        // Pre-allocate display items without heap allocation
        let items = clipboard.get_all_items_zero_copy();
        
        // Build display list with heapless strings
        for (index, item) in items.iter().enumerate().take(layout.max_visible_items) {
            let mut item_text = HeaplessString::new();
            
            // Format item without allocations
            self.format_clipboard_item(&mut item_text, item, index == self.selected_index)?;
            
            // Cache formatted text
            self.item_text_cache.push(item_text)
                .map_err(|_| UIError::TextCacheOverflow)?;
        }
        
        // Render list widget with cached text
        self.render_list_widget(frame, layout.list_area, &items)?;
        
        // Render metadata panel
        if let Some(selected_item) = items.get(self.selected_index) {
            self.render_metadata_panel(frame, layout.metadata_area, selected_item)?;
        }
        
        Ok(())
    }
    
    /// Format clipboard item with zero allocations
    fn format_clipboard_item(
        &self,
        buffer: &mut HeaplessString<256>,
        item: &clipr::ClipBoardItem,
        is_selected: bool,
    ) -> Result<(), UIError> {
        use core::fmt::Write;
        
        // Selection indicator
        let indicator = if is_selected { "▶ " } else { "  " };
        
        // Operation type with color coding
        let op_char = match item.operation {
            clipr::ClipBoardOperation::Copy => "C",
            clipr::ClipBoardOperation::Move => "M",
        };
        
        // Smart path truncation
        let display_path = self.truncate_path_smart(&item.source_path, 60);
        
        // Format without heap allocation
        write!(buffer, "{}{} {}", indicator, op_char, display_path)
            .map_err(|_| UIError::FormatError)?;
        
        Ok(())
    }
    
    /// Intelligent path truncation for optimal display
    fn truncate_path_smart(&self, path: &str, max_len: usize) -> &str {
        if path.len() <= max_len {
            return path;
        }
        
        // Find last separator for intelligent truncation
        if let Some(sep_pos) = path.rfind('/') {
            let filename = &path[sep_pos + 1..];
            if filename.len() < max_len - 3 {
                // Return "...filename" format
                let start_pos = path.len() - (max_len - 3);
                return &path[start_pos..];
            }
        }
        
        // Fallback to simple truncation
        &path[..max_len - 3]
    }
    
    /// Render empty clipboard state
    fn render_empty_state(&self, frame: &mut Frame, area: Rect) {
        let empty_text = "Clipboard is empty\nPress 'c' to copy or 'x' to cut files";
        let paragraph = Paragraph::new(empty_text)
            .alignment(Alignment::Center)
            .block(Block::default()
                .borders(Borders::ALL)
                .title("Clipboard"));
        
        frame.render_widget(paragraph, area);
    }
}

/// Pre-computed layout cache for zero-allocation rendering
#[derive(Debug)]
struct LayoutCache {
    cached_area: Option<Rect>,
    cached_layout: Option<PrecomputedLayout>,
}

impl LayoutCache {
    fn new() -> Self {
        Self {
            cached_area: None,
            cached_layout: None,
        }
    }
    
    /// Get cached layout or compute new one
    fn get_or_compute(&mut self, area: Rect) -> &PrecomputedLayout {
        if self.cached_area != Some(area) {
            self.cached_layout = Some(PrecomputedLayout::compute(area));
            self.cached_area = Some(area);
        }
        
        self.cached_layout.as_ref().unwrap()
    }
}

/// Pre-computed layout rectangles for instant positioning
#[derive(Debug, Clone)]
struct PrecomputedLayout {
    main_area: Rect,
    list_area: Rect,
    metadata_area: Rect,
    max_visible_items: usize,
}

impl PrecomputedLayout {
    fn compute(area: Rect) -> Self {
        // Calculate optimal layout based on terminal size
        let main_area = Rect {
            x: area.x + 2,
            y: area.y + 2,
            width: area.width.saturating_sub(4),
            height: area.height.saturating_sub(4),
        };
        
        // Split into list and metadata areas
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(main_area);
        
        let max_visible_items = chunks[0].height.saturating_sub(2) as usize;
        
        Self {
            main_area,
            list_area: chunks[0],
            metadata_area: chunks[1],
            max_visible_items,
        }
    }
}

/// Performance metrics for render optimization
#[derive(Debug)]
struct RenderStats {
    total_renders: u64,
    avg_render_time_ns: u64,
    max_render_time_ns: u64,
}

impl RenderStats {
    fn new() -> Self {
        Self {
            total_renders: 0,
            avg_render_time_ns: 0,
            max_render_time_ns: 0,
        }
    }
    
    fn record_render_time(&mut self, duration: std::time::Duration) {
        let time_ns = duration.as_nanos() as u64;
        
        self.total_renders += 1;
        self.max_render_time_ns = self.max_render_time_ns.max(time_ns);
        
        // Update rolling average
        self.avg_render_time_ns = 
            (self.avg_render_time_ns * (self.total_renders - 1) + time_ns) / self.total_renders;
    }
    
    /// Check if performance target is met (<100µs)
    pub fn meets_performance_target(&self) -> bool {
        self.avg_render_time_ns < 100_000 // 100µs in nanoseconds
    }
}
```

### 2. Event Loop Integration
```rust
// fsm-core/src/controller/event_loop.rs - Tab key handling integration
impl EventLoop {
    /// Handle Tab key for clipboard overlay toggle
    async fn handle_tab_key(&mut self, app: &mut MutexGuard<'_, AppState>) -> Result<Action, AppError> {
        // Toggle clipboard overlay state
        app.ui.clipboard_overlay_active = !app.ui.clipboard_overlay_active;
        
        // Reset selection when opening overlay
        if app.ui.clipboard_overlay_active {
            app.ui.selected_clipboard_item_index = 0;
            
            // Pre-warm overlay cache for immediate rendering
            if let Some(overlay) = &mut app.ui.clipboard_overlay {
                overlay.pre_warm_cache(app.ui.clipboard.len());
            }
        }
        
        Ok(Action::NoOp)
    }
    
    /// Handle clipboard overlay navigation
    async fn handle_clipboard_navigation(
        &mut self,
        app: &mut MutexGuard<'_, AppState>,
        key: KeyCode,
    ) -> Result<Action, AppError> {
        if !app.ui.clipboard_overlay_active {
            return Ok(Action::NoOp);
        }
        
        let clipboard_len = app.ui.clipboard.len();
        if clipboard_len == 0 {
            return Ok(Action::NoOp);
        }
        
        match key {
            KeyCode::Up => {
                app.ui.selected_clipboard_item_index = 
                    app.ui.selected_clipboard_item_index.saturating_sub(1);
            }
            KeyCode::Down => {
                app.ui.selected_clipboard_item_index = 
                    (app.ui.selected_clipboard_item_index + 1).min(clipboard_len - 1);
            }
            KeyCode::Enter => {
                // Paste selected item
                self.paste_selected_clipboard_item(app).await?;
                app.ui.clipboard_overlay_active = false;
            }
            KeyCode::Esc => {
                app.ui.clipboard_overlay_active = false;
            }
            _ => {}
        }
        
        Ok(Action::NoOp)
    }
    
    /// Paste specific clipboard item by index
    async fn paste_selected_clipboard_item(
        &mut self,
        app: &mut MutexGuard<'_, AppState>,
    ) -> Result<(), AppError> {
        let selected_index = app.ui.selected_clipboard_item_index;
        let clipboard = Arc::clone(&app.ui.clipboard);
        
        // Get selected item without allocation
        if let Some(item) = clipboard.get_item_by_index(selected_index) {
            let current_dir = app.fs.current_directory.clone();
            
            // Spawn paste operation
            self.spawn_paste_operation(item, current_dir).await?;
        }
        
        Ok(())
    }
}
```

### 3. UI State Extensions
```rust
// fsm-core/src/model/ui_state.rs - Clipboard overlay state integration
impl UIState {
    /// Initialize clipboard overlay components
    pub fn initialize_clipboard_overlay(&mut self) {
        self.clipboard_overlay = Some(ClipboardOverlay::new());
        self.clipboard_overlay_active = false;
        self.selected_clipboard_item_index = 0;
    }
    
    /// Get clipboard overlay rendering state
    pub fn get_clipboard_overlay_state(&self) -> Option<ClipboardOverlayState> {
        if self.clipboard_overlay_active {
            Some(ClipboardOverlayState {
                selected_index: self.selected_clipboard_item_index,
                total_items: self.clipboard.len(),
                is_empty: self.clipboard.is_empty(),
            })
        } else {
            None
        }
    }
}

// Add these fields to existing UIState struct:
pub struct UIState {
    // ... existing fields ...
    
    /// High-performance clipboard overlay component
    pub clipboard_overlay: Option<ClipboardOverlay>,
    
    /// Whether clipboard overlay is currently active
    pub clipboard_overlay_active: bool,
    
    /// Currently selected clipboard item index
    pub selected_clipboard_item_index: usize,
    
    // ... rest of existing fields ...
}

/// Clipboard overlay rendering state
#[derive(Debug, Clone)]
pub struct ClipboardOverlayState {
    pub selected_index: usize,
    pub total_items: usize,
    pub is_empty: bool,
}
```

### 4. Main UI Integration
```rust
// fsm-core/src/view/ui.rs - Clipboard overlay rendering integration
impl UI {
    /// Render clipboard overlay if active
    pub fn render_clipboard_overlay(
        &mut self,
        frame: &mut Frame,
        app_state: &AppState,
    ) -> Result<(), UIError> {
        if let Some(overlay_state) = app_state.ui.get_clipboard_overlay_state() {
            // Calculate overlay area (centered, 80% of screen)
            let overlay_area = self.calculate_centered_overlay(frame.area(), 80, 80);
            
            // Get mutable reference to overlay component
            if let Some(ref mut overlay) = app_state.ui.clipboard_overlay {
                overlay.render_zero_alloc(
                    frame,
                    overlay_area,
                    &app_state.ui.clipboard,
                    overlay_state.selected_index,
                )?;
            }
        }
        
        Ok(())
    }
    
    /// Calculate centered overlay area
    fn calculate_centered_overlay(&self, area: Rect, width_percent: u16, height_percent: u16) -> Rect {
        let overlay_width = (area.width * width_percent / 100).min(area.width);
        let overlay_height = (area.height * height_percent / 100).min(area.height);
        
        let x = (area.width.saturating_sub(overlay_width)) / 2;
        let y = (area.height.saturating_sub(overlay_height)) / 2;
        
        Rect {
            x: area.x + x,
            y: area.y + y,
            width: overlay_width,
            height: overlay_height,
        }
    }
}
```

## 6. Integration Requirements & Dependencies

### Required clipr Crate Methods
- `ClipBoard::get_all_items()` - Returns all clipboard items (✅ implemented)
- `ClipBoard::get_item_by_index(usize)` - Get specific item by index (may need implementation)
- `ClipBoard::len()` - Get total items count (✅ available)
- `ClipBoard::is_empty()` - Check if clipboard is empty (✅ available)

### UI State Integration Points
- `UIState::clipboard: Arc<ClipBoard>` - Shared clipboard reference (✅ available from Phase 3.2)
- `UIState::clipboard_overlay_active: bool` - Toggle state (needs addition)
- `UIState::selected_clipboard_item_index: usize` - Selection tracking (needs addition)

### Event Loop Integration Requirements
- Tab key handling for overlay toggle (needs implementation)
- Arrow key routing when overlay active (needs implementation)
- Enter key paste operation with overlay close (needs implementation)
- ESC key overlay close without paste (needs implementation)

## 7. Success Criteria Checklist
- [ ] **Tab Toggle**: Tab key opens/closes clipboard overlay smoothly
- [ ] **Item Display**: All clipboard items shown with proper formatting
- [ ] **Navigation**: Arrow keys navigate selection correctly
- [ ] **Selection**: Enter key pastes selected item and closes overlay
- [ ] **Performance**: <100µs render time measured and achieved
- [ ] **Zero Allocations**: Memory profiling confirms no heap allocations during UI updates
- [ ] **Responsive Layout**: Overlay adapts to different terminal sizes
- [ ] **Visual Polish**: Consistent styling with existing UI components

## 7. Risk Assessment
### Medium Risk
- **UI Complexity**: Complex layout calculations may introduce rendering bugs
  - *Mitigation*: Pre-computed layouts with extensive testing
  - *Detection*: Visual regression testing across terminal sizes

### Low Risk
- **Performance Regression**: UI rendering may not meet <100µs target
  - *Mitigation*: Built-in performance monitoring with alerts
  - *Detection*: Continuous benchmarking in development

## 8. Definition of Done
### Core Functionality
- [ ] Tab key toggles clipboard overlay with smooth animation
- [ ] All clipboard items displayed with metadata (path, type, operation)
- [ ] Arrow key navigation works correctly with visual selection indicator
- [ ] Enter key pastes selected item and closes overlay
- [ ] ESC key closes overlay without performing action
- [ ] Empty state shows helpful message when clipboard is empty

### Performance Requirements
- [ ] Sub-100µs render time achieved and monitored
- [ ] Zero heap allocations during UI updates verified
- [ ] Responsive layout adapts to terminal size changes
- [ ] Visual consistency maintained with existing UI components

### Integration & Quality
- [ ] All code passes `cargo clippy` without warnings
- [ ] Integration with existing overlay system works seamlessly
- [ ] Performance metrics integrated with status bar display
- [ ] Documentation updated in Design.md with implementation details

## 9. Phase 3.2 Foundation Reference

### Available Architecture Components
```rust
// From Phase 3.2 - Available for use in Phase 3.3

// Extreme performance key processor (in AppState)
pub struct EKeyProcessor {
    pub clipboard: Arc<ClipBoard>,              // Thread-safe clipboard access
    pub stats: EKeyStats,                       // Performance monitoring
    // ... other fields available
}

// Zero-allocation action system
pub enum ActionType {
    CopyToClipboard = 1,
    MoveToClipboard = 2, 
    PasteFromClipboard = 3,
    // Add: ToggleClipboardOverlay, NavigateClipboardUp, NavigateClipboardDown
}

// Heapless string pattern (from status_bar.rs)
let mut text_buffer: heapless::String<256> = heapless::String::new();
write!(&mut text_buffer, "Format: {}", value).unwrap_or_default();
```

### Integration Patterns Established
- **Arc<ClipBoard> Sharing**: Thread-safe clipboard access between UI and processor
- **Heapless String Construction**: Zero-allocation text formatting patterns
- **Cache-Aligned Structures**: 64-byte alignment for optimal performance
- **Lock-Free Statistics**: Atomic performance counters and monitoring
- **Dynamic Component Initialization**: Lazy loading patterns for UI components

### Performance Monitoring Available
- Cache hit rate tracking: `key_processor.stats.cache_hit_rate()`
- Latency monitoring: `key_processor.stats.update_latency(latency_ns)`
- Render time measurement: Built-in `std::time::Instant` patterns

---

**This implements a high-performance clipboard overlay UI completing the clipboard system with zero-allocation rendering and sub-100µs response times, building directly on the extreme performance foundation established in Phase 3.2.**