# FSM Implementation Specification

**ACTIVE FEATURE:** Phase 3.5: Performance Telemetry Integration

## 1. Executive Summary
**Objective:** Integrate comprehensive performance monitoring with hardware counters, regression detection, and real-time telemetry dashboard  
**Priority:** High (builds on completed Phase 3.4 advanced clipboard features)  
**Complexity:** High (requires low-level hardware integration and monitoring infrastructure)  
**Dependencies:** Phase 3.4 Advanced Clipboard Features (✅ Complete)  
**Estimated Effort:** 3-4 development sessions focusing on performance monitoring and telemetry  
**Current Status:** 🚀 Ready for performance telemetry integration implementation

## 2. Context & Background
**Problem:** While extreme performance clipboard system is complete, there's no comprehensive monitoring to validate performance claims and detect regressions  
**Current State:** Phase 3.4 complete - advanced clipboard with persistence, enhanced metadata, and multi-selection  
**Architecture Foundation:** Production-ready extreme performance clipboard system with built-in basic metrics  
**Required:** Hardware performance counters, regression detection, real-time telemetry dashboard, performance alerting  
**Integration Point:** Existing AtomicStats + performance monitoring + clipr crate + zero-allocation UI components

### Phase 3.4 Foundation Delivered
- **Clipboard Persistence**: Atomic saves with <1ms target and backup recovery
- **Multi-Selection API**: Support for batch operations on clipboard items
- **Enhanced Error Handling**: Comprehensive persistence error types and recovery
- **Performance Validation**: Built-in timing validation for save/load operations
- **Configuration System**: User-configurable persistence and retention policies

## 3. Performance & UX Design Decisions

### ADR-010: Clipboard Persistence Architecture (2024-07-25)
**Status:** Proposed  
**Context:** Users lose clipboard contents when restarting application, reducing productivity  
**Decision:** Implement file-based persistence using memory-mapped storage for instant load/save  
**Performance Targets:**
- **Save Time**: <1ms for clipboard serialization
- **Load Time**: <500µs for clipboard restoration
- **File Size**: Efficient binary format minimizing disk usage
- **Atomic Operations**: Crash-safe saves with temporary file swapping

**Consequences:**
- ✅ Clipboard contents survive application restarts
- ✅ Zero performance impact on clipboard operations
- ✅ Crash-safe persistence with atomic file operations  
- ✅ Configurable persistence location and retention policies
- ⚠️ Additional complexity in error handling and recovery
- ⚠️ Disk space usage for large clipboard histories

### ADR-011: Enhanced Metadata Display (2024-07-25)
**Status:** Proposed  
**Context:** Current metadata display is basic, users need richer file information  
**Decision:** Expand metadata panel with file preview, permissions, timestamps, and size formatting  
**Alternatives Considered:**
- Separate metadata overlay (rejected: UI complexity)
- Tooltip-based metadata (rejected: poor accessibility)
- Status bar metadata (rejected: limited space)
**Consequences:**
- ✅ Professional file manager experience
- ✅ Better decision-making for paste operations
- ✅ Maintains <100µs render performance target
- ⚠️ Increased layout complexity
- ⚠️ More extensive file system calls

## 4. Success Criteria
### Must Have (P0) - Core Advanced Features
- [ ] **Clipboard Persistence**: Save/restore clipboard across application sessions
- [ ] **Enhanced Metadata Panel**: Rich file information display (permissions, timestamps, preview)
- [ ] **Multi-Selection Support**: Select multiple clipboard items for batch operations
- [ ] **Clipboard History**: Navigate through clipboard history with configurable retention
- [ ] **Performance Preservation**: Maintain <100µs render times with expanded features
- [ ] **Error Recovery**: Robust handling of corrupted persistence files

### Should Have (P1) - Enhanced UX
- [ ] **File Preview**: Quick preview of text files, images, and documents
- [ ] **Smart Sorting**: Sort clipboard items by date, size, type, or path
- [ ] **Search/Filter**: Quick search within clipboard items
- [ ] **Keyboard Shortcuts**: Efficient navigation and selection shortcuts
- [ ] **Visual Indicators**: Clear feedback for multi-selection state
- [ ] **Configuration**: User-configurable clipboard size and retention policies

### Could Have (P2) - Advanced Features
- [ ] **Cloud Sync**: Optional cloud synchronization of clipboard contents
- [ ] **Clipboard Sharing**: Share clipboard items between application instances
- [ ] **Export/Import**: Export clipboard to various formats (JSON, CSV, text)
- [ ] **Clipboard Templates**: Save and reuse common file operation patterns

## 5. Technical Approach
**Architecture:** Extend existing ClipboardOverlay with enhanced panels and persistence layer  
**Persistence:** Memory-mapped binary format with atomic file operations  
**Performance:** Maintain zero-allocation patterns with efficient file I/O  
**Integration:** Build on Phase 3.3 foundations without breaking changes  

## Implementation Specification

### 1. Enhanced Clipboard Persistence
```rust
// clipr/src/persistence.rs - New persistence module
use memmap2::{MmapMut, MmapOptions};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// High-performance clipboard persistence with memory mapping
pub struct ClipboardPersistence {
    /// Memory-mapped file for instant access
    mmap_file: Option<MmapMut>,
    
    /// Persistence file path
    file_path: PathBuf,
    
    /// Atomic save coordination
    temp_path: PathBuf,
    
    /// Persistence configuration
    config: PersistenceConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistenceConfig {
    /// Maximum clipboard items to persist
    pub max_items: usize,
    
    /// Maximum age of items to persist (days)
    pub max_age_days: u32,
    
    /// Enable compression for large items
    pub enable_compression: bool,
    
    /// Automatic cleanup interval
    pub cleanup_interval_hours: u32,
}

impl ClipboardPersistence {
    /// Initialize persistence with configuration
    pub fn new(file_path: PathBuf, config: PersistenceConfig) -> Result<Self, PersistenceError> {
        let temp_path = file_path.with_extension("tmp");
        
        Ok(Self {
            mmap_file: None,
            file_path,
            temp_path,
            config,
        })
    }
    
    /// Save clipboard with atomic operation (<1ms target)
    pub async fn save_clipboard(&mut self, clipboard: &ClipBoard) -> Result<(), PersistenceError> {
        let start_time = Instant::now();
        
        // Serialize clipboard data
        let data = self.serialize_clipboard(clipboard).await?;
        
        // Atomic save with temporary file
        self.atomic_save(&data).await?;
        
        // Verify performance target
        let save_time = start_time.elapsed();
        if save_time > Duration::from_millis(1) {
            warn!("Clipboard save exceeded 1ms target: {:?}", save_time);
        }
        
        Ok(())
    }
    
    /// Load clipboard with memory mapping (<500µs target)
    pub async fn load_clipboard(&mut self) -> Result<ClipBoard, PersistenceError> {
        let start_time = Instant::now();
        
        // Memory map the persistence file
        let mmap = self.create_memory_map().await?;
        
        // Deserialize clipboard data
        let clipboard = self.deserialize_clipboard(&mmap).await?;
        
        // Verify performance target
        let load_time = start_time.elapsed();
        if load_time > Duration::from_micros(500) {
            warn!("Clipboard load exceeded 500µs target: {:?}", load_time);
        }
        
        Ok(clipboard)
    }
    
    /// Perform atomic save operation
    async fn atomic_save(&self, data: &[u8]) -> Result<(), PersistenceError> {
        // Write to temporary file
        tokio::fs::write(&self.temp_path, data).await?;
        
        // Atomic rename to final location
        tokio::fs::rename(&self.temp_path, &self.file_path).await?;
        
        Ok(())
    }
    
    /// Serialize clipboard efficiently
    async fn serialize_clipboard(&self, clipboard: &ClipBoard) -> Result<Vec<u8>, PersistenceError> {
        let items = clipboard.get_all_items().await;
        
        // Apply retention policies
        let filtered_items = self.apply_retention_policy(items);
        
        // Serialize with compression if enabled
        if self.config.enable_compression {
            self.compress_serialize(&filtered_items).await
        } else {
            bincode::serialize(&filtered_items).map_err(PersistenceError::SerializationFailed)
        }
    }
    
    /// Apply retention policies to clipboard items
    fn apply_retention_policy(&self, items: Vec<ClipBoardItem>) -> Vec<ClipBoardItem> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let max_age_seconds = self.config.max_age_days as u64 * 24 * 60 * 60;
        
        items.into_iter()
            .filter(|item| now - item.added_at < max_age_seconds)
            .take(self.config.max_items)
            .collect()
    }
}
```

### 2. Enhanced Metadata Display
```rust
// fsm-core/src/view/components/clipboard_overlay.rs - Enhanced metadata panel
impl ClipboardOverlay {
    /// Render enhanced metadata panel with rich file information
    fn render_enhanced_metadata_panel(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        item: &ClipBoardItem,
    ) -> Result<(), AppError> {
        // Split metadata area into sections
        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6), // File info
                Constraint::Length(4), // Permissions
                Constraint::Length(3), // Timestamps
                Constraint::Fill(1),   // Preview
            ])
            .split(area);
        
        // Render file information section
        self.render_file_info(frame, sections[0], item)?;
        
        // Render permissions section
        self.render_permissions(frame, sections[1], item)?;
        
        // Render timestamps section
        self.render_timestamps(frame, sections[2], item)?;
        
        // Render file preview if applicable
        self.render_file_preview(frame, sections[3], item)?;
        
        Ok(())
    }
    
    /// Render file information with smart formatting
    fn render_file_info(
        &self, 
        frame: &mut Frame<'_>, 
        area: Rect, 
        item: &ClipBoardItem
    ) -> Result<(), AppError> {
        let file_size = item.metadata.size;
        let file_type = item.metadata.file_type;
        
        let info_text = format!(
            "Path: {}\nType: {:?}\nSize: {}\nOperation: {:?}",
            item.source_path,
            file_type,
            format_file_size(file_size),
            item.operation
        );
        
        let paragraph = Paragraph::new(info_text)
            .block(Block::default().borders(Borders::ALL).title("File Info"))
            .wrap(Wrap { trim: true });
        
        frame.render_widget(paragraph, area);
        Ok(())
    }
    
    /// Render file permissions in human-readable format
    fn render_permissions(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        item: &ClipBoardItem,
    ) -> Result<(), AppError> {
        let permissions = item.metadata.permissions;
        let perm_string = format_permissions(permissions);
        
        let perm_text = format!(
            "Permissions: {}\nOwner: rwx\nGroup: r-x\nOther: r--",
            perm_string
        );
        
        let paragraph = Paragraph::new(perm_text)
            .block(Block::default().borders(Borders::ALL).title("Permissions"));
        
        frame.render_widget(paragraph, area);
        Ok(())
    }
    
    /// Render timestamps with relative formatting
    fn render_timestamps(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        item: &ClipBoardItem,
    ) -> Result<(), AppError> {
        let modified_time = format_timestamp_relative(item.metadata.modified);
        let added_time = format_timestamp_relative(item.added_at);
        
        let time_text = format!(
            "Modified: {}\nAdded: {}",
            modified_time,
            added_time
        );
        
        let paragraph = Paragraph::new(time_text)
            .block(Block::default().borders(Borders::ALL).title("Timestamps"));
        
        frame.render_widget(paragraph, area);
        Ok(())
    }
    
    /// Render file preview based on file type
    fn render_file_preview(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        item: &ClipBoardItem,
    ) -> Result<(), AppError> {
        let preview_text = match item.metadata.file_type {
            FileType::RegularFile => {
                if self.is_text_file(&item.source_path) {
                    self.load_text_preview(&item.source_path, 10)?
                } else {
                    format!("Binary file\n{} bytes", item.metadata.size)
                }
            }
            FileType::Directory => {
                format!("Directory\nContains {} items", self.count_directory_items(&item.source_path)?)
            }
            FileType::Symlink => {
                format!("Symbolic link\nTarget: {}", self.resolve_symlink(&item.source_path)?)
            }
            _ => "Special file".to_string(),
        };
        
        let paragraph = Paragraph::new(preview_text)
            .block(Block::default().borders(Borders::ALL).title("Preview"))
            .wrap(Wrap { trim: true });
        
        frame.render_widget(paragraph, area);
        Ok(())
    }
}

/// Format file size in human-readable format
fn format_file_size(size: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    
    if size == 0 {
        return "0 B".to_string();
    }
    
    let size_f = size as f64;
    let unit_index = (size_f.log10() / 3.0).floor() as usize;
    let unit_index = unit_index.min(UNITS.len() - 1);
    
    let size_in_unit = size_f / (1000_f64.powi(unit_index as i32));
    
    format!("{:.1} {}", size_in_unit, UNITS[unit_index])
}

/// Format permissions as rwx string
fn format_permissions(permissions: u16) -> String {
    let mut perm_string = String::with_capacity(9);
    
    // Owner permissions
    perm_string.push(if permissions & 0o400 != 0 { 'r' } else { '-' });
    perm_string.push(if permissions & 0o200 != 0 { 'w' } else { '-' });
    perm_string.push(if permissions & 0o100 != 0 { 'x' } else { '-' });
    
    // Group permissions
    perm_string.push(if permissions & 0o040 != 0 { 'r' } else { '-' });
    perm_string.push(if permissions & 0o020 != 0 { 'w' } else { '-' });
    perm_string.push(if permissions & 0o010 != 0 { 'x' } else { '-' });
    
    // Other permissions
    perm_string.push(if permissions & 0o004 != 0 { 'r' } else { '-' });
    perm_string.push(if permissions & 0o002 != 0 { 'w' } else { '-' });
    perm_string.push(if permissions & 0o001 != 0 { 'x' } else { '-' });
    
    perm_string
}
```

### 3. Multi-Selection Support
```rust
// fsm-core/src/model/ui_state.rs - Multi-selection state
pub struct UIState {
    // ... existing fields ...
    
    /// Selected clipboard item indices for multi-selection
    pub selected_clipboard_items: HashSet<usize>,
    
    /// Multi-selection mode active
    pub clipboard_multi_select_mode: bool,
    
    // ... rest of fields ...
}

impl UIState {
    /// Toggle multi-selection for clipboard item at index
    pub fn toggle_clipboard_item_selection(&mut self, index: usize) {
        if self.selected_clipboard_items.contains(&index) {
            self.selected_clipboard_items.remove(&index);
        } else {
            self.selected_clipboard_items.insert(index);
        }
        
        // Exit multi-select mode if no items selected
        if self.selected_clipboard_items.is_empty() {
            self.clipboard_multi_select_mode = false;
        }
    }
    
    /// Select all clipboard items
    pub fn select_all_clipboard_items(&mut self, total_items: usize) {
        self.selected_clipboard_items.clear();
        for i in 0..total_items {
            self.selected_clipboard_items.insert(i);
        }
        self.clipboard_multi_select_mode = true;
    }
    
    /// Clear all clipboard selections
    pub fn clear_clipboard_selections(&mut self) {
        self.selected_clipboard_items.clear();
        self.clipboard_multi_select_mode = false;
    }
}

// fsm-core/src/controller/event_loop.rs - Multi-selection key handling
impl EventLoop {
    /// Handle multi-selection keys in clipboard overlay
    async fn handle_clipboard_multi_selection(
        &mut self,
        app: &mut MutexGuard<'_, AppState>,
        key: KeyCode,
        modifiers: KeyModifiers,
    ) -> Result<Action, AppError> {
        if !app.ui.clipboard_overlay_active {
            return Ok(Action::NoOp);
        }
        
        match (key, modifiers) {
            // Space: Toggle selection of current item
            (KeyCode::Char(' '), KeyModifiers::NONE) => {
                let current_index = app.ui.selected_clipboard_item_index;
                app.ui.toggle_clipboard_item_selection(current_index);
                app.ui.clipboard_multi_select_mode = true;
            }
            
            // Ctrl+A: Select all items
            (KeyCode::Char('a'), KeyModifiers::CONTROL) => {
                let total_items = app.ui.clipboard.len();
                app.ui.select_all_clipboard_items(total_items);
            }
            
            // Escape: Clear selections
            (KeyCode::Esc, _) => {
                if app.ui.clipboard_multi_select_mode {
                    app.ui.clear_clipboard_selections();
                } else {
                    app.ui.clipboard_overlay_active = false;
                }
            }
            
            // Enter: Paste selected items or current item
            (KeyCode::Enter, _) => {
                if app.ui.clipboard_multi_select_mode {
                    self.paste_multiple_clipboard_items(app).await?;
                } else {
                    self.paste_selected_clipboard_item(app).await?;
                }
                app.ui.clipboard_overlay_active = false;
            }
            
            _ => return Ok(Action::NoOp),
        }
        
        app.ui.request_redraw(RedrawFlag::All);
        Ok(Action::NoOp)
    }
    
    /// Paste multiple selected clipboard items
    async fn paste_multiple_clipboard_items(
        &mut self,
        app: &mut MutexGuard<'_, AppState>,
    ) -> Result<(), AppError> {
        let selected_indices: Vec<usize> = app.ui.selected_clipboard_items.iter().copied().collect();
        let clipboard = Arc::clone(&app.ui.clipboard);
        let current_dir = app.fs.current_directory.clone();
        
        // Spawn paste operations for all selected items
        for index in selected_indices {
            if let Some(items) = clipboard.get_all_items().await.get(index) {
                let item = items.clone();
                self.spawn_paste_operation(item, current_dir.clone()).await?;
            }
        }
        
        // Clear selections after paste
        app.ui.clear_clipboard_selections();
        
        Ok(())
    }
}
```

## 6. Integration Requirements & Dependencies

### Enhanced clipr Crate Methods
- `ClipBoard::save_to_file(path)` - Persistence save operation (needs implementation)
- `ClipBoard::load_from_file(path)` - Persistence load operation (needs implementation)
- `ClipBoard::get_items_by_indices(indices)` - Multi-selection support (needs implementation)
- Enhanced metadata collection for file previews and extended information

### UI State Extensions
- Multi-selection tracking with `HashSet<usize>`
- Persistence configuration and file path management
- Enhanced metadata display state
- Search/filter state for clipboard items

### Performance Monitoring Extensions
- Persistence operation timing (save/load performance)
- Metadata collection performance tracking  
- Multi-selection operation metrics
- Memory usage monitoring for enhanced features

## 7. Success Criteria Checklist
- [ ] **Persistence**: Clipboard survives application restart with <1ms save, <500µs load
- [ ] **Enhanced Metadata**: Rich file information display with preview capabilities
- [ ] **Multi-Selection**: Space key toggles, Ctrl+A selects all, Enter pastes multiple
- [ ] **Performance**: Maintain <100µs render times with all enhancements
- [ ] **Configuration**: User-configurable persistence and retention policies
- [ ] **Error Handling**: Robust recovery from corrupted persistence files
- [ ] **UI Polish**: Smooth animations and visual feedback for all operations

## 8. Risk Assessment
### High Risk
- **Performance Regression**: Enhanced features may impact <100µs render target
  - *Mitigation*: Careful profiling and optimization of new features
  - *Detection*: Continuous performance monitoring with alerts

### Medium Risk
- **Persistence Corruption**: File corruption could cause data loss
  - *Mitigation*: Atomic saves with backup files and validation
  - *Detection*: Checksum validation and recovery procedures

### Low Risk
- **UI Complexity**: Enhanced metadata display may clutter interface
  - *Mitigation*: Progressive disclosure and configurable detail levels
  - *Detection*: User testing and feedback collection

## 9. Definition of Done
### Core Advanced Features
- [ ] Clipboard persistence implemented with atomic file operations
- [ ] Enhanced metadata panel with permissions, timestamps, and preview
- [ ] Multi-selection support with keyboard shortcuts and visual indicators
- [ ] Performance targets maintained (<100µs render, <1ms save, <500µs load)
- [ ] Configuration system for persistence and retention policies
- [ ] Comprehensive error handling and recovery mechanisms

### Quality & Integration
- [ ] All code passes cargo clippy without warnings
- [ ] Full test coverage for persistence and multi-selection operations
- [ ] Performance regression tests for enhanced features
- [ ] Documentation updated with new features and usage examples
- [ ] User experience testing confirms intuitive operation

---

**This implements advanced clipboard features building on the solid foundation of Phase 3.3, delivering professional file manager capabilities while maintaining extreme performance standards.**