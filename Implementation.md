# FSM Implementation Specification

**ACTIVE FEATURE:** Phase 3.1: Core Clipboard Infrastructure (`clipr` crate)

## 1. Executive Summary
**Objective:** Establish workspace architecture with `clipr` crate for advanced clipboard system  
**Priority:** High (foundation for intuitive copy/paste UX)  
**Complexity:** Medium (workspace setup + crate interface design)  
**Dependencies:** Phase 2.4 ESC cancellation (✅ Complete)  
**Estimated Effort:** 1-2 development sessions
**Current Status:** 🚧 Workspace structure created, core modules need completion  

## 2. Context & Background
**Problem:** Current copy operation requires typing full destination paths, extremely poor UX  
**Current State:** Copy requires `InputPromptType::CopyDestination` with manual path entry  
**Required:** Modern clipboard system with persistent copy/move operations and visual indicators  
**Integration Point:** Workspace architecture with clean crate separation and API design  

## 3. Success Criteria
### Must Have (P0)
- [ ] **Workspace Setup**: Convert to workspace with `fsm-core/` and `clipr/` crates
- [ ] **Core Clipboard API**: `Clipboard` struct with add/remove/paste operations
- [ ] **ClipboardItem Model**: Comprehensive item representation with metadata
- [ ] **Operation Types**: Support for Copy and Move operations
- [ ] **UIState Integration**: Embed clipboard instance in main app state
- [ ] **Error Handling**: Robust error types and propagation between crates

### Should Have (P1)  
- [ ] **Configuration**: Configurable clipboard limits and persistence options
- [ ] **Performance**: Optimized for large numbers of clipboard items
- [ ] **Memory Management**: Smart cleanup and item expiration policies
- [ ] **Serialization**: Persistence support for clipboard state across sessions

### Could Have (P2)
- [ ] **Advanced Metadata**: Extended file information and statistics
- [ ] **Clipboard History**: Track clipboard changes over time
- [ ] **Import/Export**: Clipboard state import/export functionality

## 4. Technical Approach
**Architecture:** Rust workspace with clean crate separation and well-defined APIs  
**Data Flow:** Main app → clipr API → clipboard operations → file system integration  
**Performance:** Zero-copy where possible, efficient item lookup with HashMap indexing  
**Error Handling:** Custom error types with `thiserror` for clean error propagation  

## Implementation Specification

### 1. Workspace Architecture Setup
```toml
# Root Cargo.toml - Workspace configuration
[workspace]
members = [
    "fsm-core",
    "clipr"
]
resolver = "2"

[workspace.dependencies]
tokio = { version = "1.0", features = ["full"] }
ratatui = "0.28"
crossterm = "0.28"
tracing = "0.1"
serde = { version = "1.0", features = ["derive"] }
thiserror = "1.0"
anyhow = "1.0"
uuid = { version = "1.0", features = ["v4"] }

# fsm-core/Cargo.toml - Main application
[package]
name = "fsm-core"
version = "0.1.0"
edition = "2021"

[dependencies]
clipr = { path = "../clipr" }
# ... existing dependencies moved from workspace

# clipr/Cargo.toml - Clipboard crate
[package]
name = "clipr"
version = "0.1.0"
edition = "2021"
description = "Advanced clipboard system for file managers"
license = "MIT"

[dependencies]
serde = { workspace = true }
thiserror = { workspace = true }
tokio = { workspace = true }
uuid = { workspace = true }
tracing = { workspace = true }
```

### 2. Core Clipboard API Design
```rust
// clipr/src/lib.rs - Main crate interface
pub mod clipboard;
pub mod item;
pub mod operations;
pub mod error;
pub mod config;

pub use clipboard::Clipboard;
pub use item::{ClipboardItem, ClipboardOperation, ItemStatus, FileMetadata};
pub use operations::{PasteOperation, FileOperation};
pub use error::{ClipError, ClipResult};
pub use config::ClipboardConfig;

// clipr/src/clipboard.rs - Core clipboard implementation
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;
use uuid::Uuid;

pub struct Clipboard {
    items: HashMap<String, ClipboardItem>,
    item_order: Vec<String>,
    config: ClipboardConfig,
    created_at: Instant,
}

impl Clipboard {
    pub fn new(config: ClipboardConfig) -> Self {
        Self {
            items: HashMap::new(),
            item_order: Vec::new(),
            config,
            created_at: Instant::now(),
        }
    }
    
    /// Add file to clipboard for copy operation
    pub fn add_copy(&mut self, path: PathBuf) -> ClipResult<String> {
        let item = ClipboardItem::new_copy(path)?;
        let id = item.id.clone();
        
        self.insert_item(item)?;
        Ok(id)
    }
    
    /// Add file to clipboard for move operation  
    pub fn add_move(&mut self, path: PathBuf) -> ClipResult<String> {
        let item = ClipboardItem::new_move(path)?;
        let id = item.id.clone();
        
        self.insert_item(item)?;
        Ok(id)
    }
    
    /// Remove item from clipboard
    pub fn remove_item(&mut self, id: &str) -> ClipResult<ClipboardItem> {
        let item = self.items.remove(id)
            .ok_or(ClipError::ItemNotFound(id.to_string()))?;
        
        self.item_order.retain(|item_id| item_id != id);
        Ok(item)
    }
    
    /// Get item for paste operation
    pub fn get_paste_operation(&self, id: &str, dest: PathBuf) -> ClipResult<PasteOperation> {
        let item = self.items.get(id)
            .ok_or(ClipError::ItemNotFound(id.to_string()))?;
            
        PasteOperation::new(item, dest)
    }
    
    /// Get all clipboard items in order
    pub fn items(&self) -> Vec<&ClipboardItem> {
        self.item_order.iter()
            .filter_map(|id| self.items.get(id))
            .collect()
    }
    
    /// Clear all clipboard items
    pub fn clear(&mut self) {
        self.items.clear();
        self.item_order.clear();
    }
    
    /// Get clipboard statistics
    pub fn stats(&self) -> ClipboardStats {
        ClipboardStats::new(&self.items, self.created_at)
    }
    
    fn insert_item(&mut self, item: ClipboardItem) -> ClipResult<()> {
        // Check for duplicates
        if self.items.values().any(|existing| existing.source_path == item.source_path) {
            return Err(ClipError::DuplicateItem(item.source_path));
        }
        
        // Enforce item limit
        while self.items.len() >= self.config.max_items {
            if let Some(oldest_id) = self.item_order.first().cloned() {
                self.remove_item(&oldest_id)?;
            }
        }
        
        let id = item.id.clone();
        self.items.insert(id.clone(), item);
        self.item_order.push(id);
        
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ClipboardStats {
    pub total_items: usize,
    pub copy_items: usize,
    pub move_items: usize,
    pub total_size: u64,
    pub created_at: Instant,
}
```

### 3. ClipboardItem Implementation
```rust
// clipr/src/item.rs - Clipboard item model
use std::path::PathBuf;
use std::time::Instant;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct ClipboardItem {
    pub id: String,
    pub source_path: PathBuf,
    pub operation: ClipboardOperation,
    pub metadata: FileMetadata,
    pub added_at: Instant,
    pub status: ItemStatus,
}

impl ClipboardItem {
    pub fn new_copy(path: PathBuf) -> ClipResult<Self> {
        let metadata = FileMetadata::from_path(&path)?;
        
        Ok(Self {
            id: Uuid::new_v4().to_string(),
            source_path: path,
            operation: ClipboardOperation::Copy,
            metadata,
            added_at: Instant::now(),
            status: ItemStatus::Ready,
        })
    }
    
    pub fn new_move(path: PathBuf) -> ClipResult<Self> {
        let metadata = FileMetadata::from_path(&path)?;
        
        Ok(Self {
            id: Uuid::new_v4().to_string(),
            source_path: path,
            operation: ClipboardOperation::Move,
            metadata,
            added_at: Instant::now(),
            status: ItemStatus::Ready,
        })
    }
    
    pub fn display_name(&self) -> &str {
        self.source_path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Unknown")
    }
    
    pub fn operation_tag(&self) -> &'static str {
        match self.operation {
            ClipboardOperation::Copy => "C",
            ClipboardOperation::Move => "M",
        }
    }
    
    pub fn is_expired(&self, max_age: std::time::Duration) -> bool {
        self.added_at.elapsed() > max_age
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardOperation {
    Copy,
    Move,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemStatus {
    Ready,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileMetadata {
    pub size: u64,
    pub is_dir: bool,
    pub modified: Option<Instant>,
    pub permissions: FilePermissions,
    pub file_type: FileType,
}

impl FileMetadata {
    pub fn from_path(path: &PathBuf) -> ClipResult<Self> {
        let metadata = std::fs::metadata(path)
            .map_err(|e| ClipError::MetadataError(path.clone(), e))?;
            
        Ok(Self {
            size: metadata.len(),
            is_dir: metadata.is_dir(),
            modified: metadata.modified().ok().map(|t| t.into()),
            permissions: FilePermissions::from_metadata(&metadata),
            file_type: FileType::from_path(path),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FilePermissions {
    pub readable: bool,
    pub writable: bool,
    pub executable: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FileType {
    RegularFile,
    Directory,
    Symlink,
    Other,
}
```

### 4. Error Handling System
```rust
// clipr/src/error.rs - Comprehensive error handling
use std::path::PathBuf;
use thiserror::Error;

pub type ClipResult<T> = Result<T, ClipError>;

#[derive(Error, Debug, Clone)]
pub enum ClipError {
    #[error("Clipboard item not found: {0}")]
    ItemNotFound(String),
    
    #[error("Duplicate item already in clipboard: {0}")]
    DuplicateItem(PathBuf),
    
    #[error("Clipboard is full (max {max} items)")]
    ClipboardFull { max: usize },
    
    #[error("Failed to read metadata for {0}: {1}")]
    MetadataError(PathBuf, std::io::Error),
    
    #[error("Invalid file path: {0}")]
    InvalidPath(PathBuf),
    
    #[error("Operation not supported: {0}")]
    UnsupportedOperation(String),
    
    #[error("Clipboard configuration error: {0}")]
    ConfigError(String),
    
    #[error("File system error: {0}")]
    FileSystemError(#[from] std::io::Error),
}

impl ClipError {
    pub fn is_recoverable(&self) -> bool {
        matches!(self, 
            ClipError::ItemNotFound(_) |
            ClipError::DuplicateItem(_) |
            ClipError::ClipboardFull { .. }
        )
    }
}
```

### 5. UIState Integration
```rust
// fsm-core/src/model/ui_state.rs - Integration points
use clipr::Clipboard;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct UIState {
    // ... existing fields ...
    
    /// Integrated clipboard system
    pub clipboard: Clipboard,
    
    /// Clipboard overlay state
    pub clipboard_overlay_active: bool,
    pub selected_clipboard_item: Option<String>,
    pub clipboard_view_mode: ClipboardViewMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardViewMode {
    List,       // Standard list view
    Details,    // Detailed view with metadata  
    Grid,       // Grid layout for many items
}

impl UIState {
    pub fn toggle_clipboard_overlay(&mut self) {
        self.clipboard_overlay_active = !self.clipboard_overlay_active;
        if !self.clipboard_overlay_active {
            self.selected_clipboard_item = None;
        }
    }
    
    pub fn show_clipboard_overlay(&mut self) {
        self.clipboard_overlay_active = true;
        self.request_redraw(RedrawFlag::Overlay);
    }
    
    pub fn close_clipboard_overlay(&mut self) {
        self.clipboard_overlay_active = false;
        self.selected_clipboard_item = None;
        self.request_redraw(RedrawFlag::Overlay);
    }
}
```

### 6. Configuration System
```rust
// clipr/src/config.rs - Clipboard configuration
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardConfig {
    /// Maximum number of items in clipboard
    pub max_items: usize,
    
    /// Auto-expire items after duration (None = never expire)
    pub item_expiry: Option<Duration>,
    
    /// Enable clipboard persistence across sessions
    pub persist_clipboard: bool,
    
    /// Clipboard file location for persistence
    pub clipboard_file: Option<PathBuf>,
    
    /// Show visual indicators in main UI
    pub show_clipboard_indicators: bool,
    
    /// Confirm destructive operations
    pub confirm_destructive_ops: bool,
}

impl Default for ClipboardConfig {
    fn default() -> Self {
        Self {
            max_items: 50,
            item_expiry: Some(Duration::from_secs(3600)), // 1 hour
            persist_clipboard: true,
            clipboard_file: None, // Will use default location
            show_clipboard_indicators: true,
            confirm_destructive_ops: true,
        }
    }
}

impl ClipboardConfig {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> ClipResult<Self> {
        // Implementation for loading config from file
    }
    
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> ClipResult<()> {
        // Implementation for saving config to file
    }
}
```

## Success Criteria Checklist
- [x] **Workspace Setup**: Cargo.toml workspace configuration with proper dependency management
- [ ] **Core Clipboard API**: Complete Clipboard struct with all essential operations
- [ ] **ClipboardItem Model**: Rich item representation with metadata and status tracking
- [ ] **Operation Types**: Copy and Move operation support with proper distinction
- [ ] **UIState Integration**: Clean integration of clipboard into main application state
- [ ] **Error Handling**: Comprehensive error types with proper propagation and recovery
- [ ] **Configuration**: Flexible configuration system with sensible defaults
- [ ] **Performance**: Efficient HashMap-based item lookup and management
- [ ] **Memory Management**: Smart cleanup policies and item limits
- [ ] **Documentation**: Complete API documentation and usage examples

## Implementation Progress Status
### ✅ Completed
1. **Workspace Configuration**: Root Cargo.toml with proper workspace setup
2. **Directory Structure**: `fsm-core/` and `clipr/` directories created
3. **Crate Setup**: Basic Cargo.toml files for both crates
4. **Code Migration**: Existing source moved to `fsm-core/src/`

### 🚧 In Progress  
1. **clipr Core Modules**: Need to complete error.rs, item.rs, clipboard.rs, config.rs, operations.rs
2. **UIState Integration**: Need to add clipr::Clipboard to UIState in fsm-core
3. **Build Verification**: Test workspace compilation and integration

### ⏳ Next Steps
1. Complete clipr crate implementation
2. Integrate clipr into fsm-core UIState 
3. Test workspace build and functionality
4. Move to Phase 3.2 (Basic Copy/Move Operations)

## Technical Requirements
- **Performance**: O(1) item lookup, O(n) for ordered iteration where n = clipboard size
- **Memory**: Configurable limits with automatic cleanup of expired items
- **Thread Safety**: All public APIs safe for concurrent access (Arc<Mutex<>> wrapper in main app)
- **Integration**: Clean separation with well-defined API boundary between crates
- **Error Handling**: Comprehensive error coverage with recovery strategies

## Testing Strategy
1. **Unit Tests**: Individual clipboard operations (add, remove, paste)
2. **Integration Tests**: Workspace setup and inter-crate communication
3. **Performance Tests**: Large clipboard operations and memory usage
4. **API Tests**: Public interface contract validation
5. **Error Handling Tests**: All error conditions and recovery paths

## Risk Assessment
### High Risk (Project Impact)
- **Workspace Migration Complexity**
  - *Analysis*: Converting existing single-crate to workspace requires careful dependency management
  - *Mitigation*: Incremental migration with thorough testing at each step
  - *Detection*: Compilation failures and integration test failures

### Medium Risk (Integration)  
- **Inter-crate Communication Overhead**
  - *Analysis*: API boundaries might introduce performance overhead
  - *Mitigation*: Zero-copy APIs where possible, efficient serialization
  - *Detection*: Performance benchmarking and profiling

### Low Risk (Feature Scope)
- **Configuration Complexity**
  - *Analysis*: Too many configuration options might confuse users
  - *Mitigation*: Sensible defaults with minimal required configuration
  - *Detection*: User testing and feedback

## Rollback Plan
**Failure Condition:** Workspace setup fails or causes major integration issues  
**Rollback Steps:**
1. Revert to single-crate structure by moving clipr code into main app
2. Remove workspace Cargo.toml and restore original dependency structure
3. Update import paths to use internal modules instead of external crate
4. Verify all functionality works in single-crate mode
5. Plan alternative approach for clipboard separation

## Definition of Done
### Code Quality
- [ ] All P0 success criteria implemented and tested
- [ ] Workspace builds successfully with `cargo build --workspace`
- [ ] All tests pass with `cargo test --workspace`
- [ ] Code passes cargo fmt + check + clippy for both crates
- [ ] Public APIs fully documented with rustdoc

### Integration Testing  
- [ ] Manual testing of clipboard basic operations
- [ ] Workspace dependency resolution verified
- [ ] Inter-crate communication tested under various scenarios
- [ ] Error propagation validated across crate boundaries

### Documentation & Continuity
- [ ] Design.md updated with Phase 3.1 completion details
- [ ] Implementation.md prepared for Phase 3.2 (basic copy/move operations)
- [ ] CLAUDE.md updated with workspace-specific development patterns
- [ ] ADR created for workspace architecture decisions

---

**This establishes the foundation for the comprehensive clipboard system with clean architecture and extensible design.**

## 1. Executive Summary
**Objective:** Enable user-initiated cancellation of file operations via ESC key  
**Priority:** High (essential UX for long-running operations)  
**Complexity:** Medium (key handling + cancellation token cleanup)  
**Dependencies:** Phase 2.3 FileOperationsOverlay (✅ Complete)  
**Estimated Effort:** 1 development session  

## 2. Context & Background
**Problem:** Users cannot cancel long-running file operations once started  
**Current State:** CancellationToken integrated but no user-facing cancellation trigger  
**Required:** ESC key handling + proper cleanup of cancelled operations  
**Integration Point:** Event loop key handling + UIState operation management  

## 3. Success Criteria
### Must Have (P0)
- [ ] **ESC Key Detection**: Detect ESC key during active file operations
- [ ] **Token Cancellation**: Cancel active operations via CancellationToken
- [ ] **UI Cleanup**: Remove cancelled operations from active_file_operations HashMap  
- [ ] **User Feedback**: Show cancellation confirmation message
- [ ] **Graceful Degradation**: Handle partial operation completion properly

### Should Have (P1)
- [ ] **Multi-operation Cancel**: Cancel all active operations with single ESC press
- [ ] **Cancel Confirmation**: Brief notification confirming cancellation
- [ ] **Resource Cleanup**: Ensure proper cleanup of temporary files/handles
- [ ] **Progress Preservation**: Don't lose completed work before cancellation

### Could Have (P2)
- [ ] **Selective Cancellation**: Choose which operation to cancel (future enhancement)
- [ ] **Cancel with Confirmation**: Optional confirmation dialog before cancellation

## 4. Technical Approach
**Architecture:** Event loop key handling with cancellation token integration  
**Data Flow:** ESC key → event_loop → cancel tokens → cleanup UIState  
**Performance:** Minimal overhead, only active during operations  
**Error Handling:** Robust cleanup even if cancellation fails

## Implementation Specification

### 1. Cancellation Token Management
```rust
// src/model/ui_state.rs - Add cancellation token tracking
use tokio_util::CancellationToken;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct UIState {
    // ... existing fields ...
    
    /// Track cancellation tokens for active operations
    pub operation_cancel_tokens: HashMap<String, CancellationToken>,
}

impl UIState {
    /// Store cancellation token for operation
    pub fn store_cancel_token(&mut self, operation_id: String, token: CancellationToken) {
        self.operation_cancel_tokens.insert(operation_id, token);
    }
    
    /// Cancel all active file operations
    pub fn cancel_all_operations(&mut self) -> usize {
        let count = self.operation_cancel_tokens.len();
        
        // Cancel all tokens
        for token in self.operation_cancel_tokens.values() {
            token.cancel();
        }
        
        // Clear tracking data
        self.operation_cancel_tokens.clear();
        self.active_file_operations.clear();
        
        count
    }
    
    /// Remove completed/cancelled operation
    pub fn remove_operation(&mut self, operation_id: &str) {
        self.operation_cancel_tokens.remove(operation_id);
        self.active_file_operations.remove(operation_id);
    }
}
```

### 2. Event Loop ESC Key Handling
```rust
// src/controller/event_loop.rs - Add ESC key cancellation
use crossterm::event::{KeyCode, KeyEvent};

impl EventLoop {
    async fn handle_key_event(&mut self, key: KeyEvent) -> Result<(), AppError> {
        // ESC key handling - highest priority for operation cancellation
        if key.code == KeyCode::Esc {
            return self.handle_escape_key().await;
        }
        
        // ... existing key handling ...
    }
    
    async fn handle_escape_key(&mut self) -> Result<(), AppError> {
        let mut app = self.app.lock().await;
        
        // Cancel file operations if any are active
        if !app.ui.active_file_operations.is_empty() {
            let cancelled_count = app.ui.cancel_all_operations();
            
            if cancelled_count > 0 {
                app.ui.show_info(format!("Cancelled {} file operation(s)", cancelled_count));
                tracing::info!("User cancelled {} file operations via ESC key", cancelled_count);
                return Ok(());
            }
        }
        
        // Handle other overlay closures
        match app.ui.overlay {
            UIOverlay::Help | UIOverlay::Search | UIOverlay::FileNameSearch 
            | UIOverlay::ContentSearch | UIOverlay::SearchResults 
            | UIOverlay::Prompt => {
                app.ui.close_all_overlays();
            }
            _ => {
                // ESC in browse mode - could add additional behavior here
            }
        }
        
        Ok(())
    }
}
```

### 3. File Operation Task Integration
```rust
// src/tasks/file_ops_task.rs - Integrate token storage
impl FileOperationTask {
    pub async fn execute(mut self) -> Result<(), AppError> {
        let operation_id = self.operation_id.clone();
        let cancel_token = self.cancel_token.clone();
        
        // Store cancellation token in UI state for ESC key access
        {
            let app = self.app.lock().await;
            app.ui.store_cancel_token(operation_id.clone(), cancel_token.clone());
        }
        
        // ... existing task execution logic ...
        
        // Cleanup on completion or error
        let app = self.app.lock().await;
        app.ui.remove_operation(&operation_id);
        
        Ok(())
    }
}
```

### 4. Enhanced Task Result Handling
```rust
// src/controller/event_loop.rs - Handle cancellation in task results
async fn handle_task_result(&mut self, result: TaskResult) -> Result<(), AppError> {
    match result {
        TaskResult::FileOperationComplete { operation_id, result } => {
            let mut app = self.app.lock().await;
            
            // Remove from tracking regardless of success/failure
            app.ui.remove_operation(&operation_id);
            
            match result {
                Ok(()) => {
                    app.ui.show_success("File operation completed successfully");
                }
                Err(e) => {
                    // Check if error was due to cancellation
                    if e.to_string().contains("cancelled") || e.to_string().contains("canceled") {
                        // Don't show error for user-initiated cancellation
                        tracing::debug!("Operation {} was cancelled by user", operation_id);
                    } else {
                        app.ui.show_error(format!("File operation failed: {}", e));
                    }
                }
            }
        }
        TaskResult::FileOperationProgress { operation_id, .. } => {
            // Progress updates don't need special cancellation handling
            // The cancellation will be handled by the task itself
        }
        // ... other task results ...
    }
    
    Ok(())
}
```

## Success Criteria
- [ ] **ESC Key Detection**: Detect ESC key during active file operations
- [ ] **Token Cancellation**: Cancel active operations via CancellationToken
- [ ] **UI Cleanup**: Remove cancelled operations from active_file_operations HashMap  
- [ ] **User Feedback**: Show cancellation confirmation message
- [ ] **Graceful Degradation**: Handle partial operation completion properly
- [ ] **Multi-operation Cancel**: Cancel all active operations with single ESC press
- [ ] **Cancel Confirmation**: Brief notification confirming cancellation
- [ ] **Resource Cleanup**: Ensure proper cleanup of temporary files/handles
- [ ] **Progress Preservation**: Don't lose completed work before cancellation

## Technical Requirements
- **Performance**: Minimal overhead, only active during operations
- **Memory**: Efficient HashMap cleanup of cancelled operations
- **Thread Safety**: Proper async/await handling during cancellation
- **Integration**: Seamless addition to existing event loop
- **Error Handling**: Robust cleanup even if cancellation fails

## Testing Approach
1. Start large file copy operation
2. Press ESC key during operation
3. Verify operation cancels and UI clears
4. Test with multiple concurrent operations
5. Confirm cancellation notification appears
6. Validate no resource leaks after cancellation

## 5. Testing Strategy
**Unit Tests:** Cancellation token behavior and UI state cleanup  
**Integration Tests:** Full ESC key → cancellation → UI update pipeline  
**Performance Tests:** Memory and resource usage during cancellation  
**User Acceptance:** Manual testing with various operation types  

## 6. Risk Assessment (Claude-Enhanced)
### High Risk (Project Impact)
- **Incomplete cancellation leaving zombie operations**
  - *Claude Analysis*: Partial cleanup could cause UI inconsistencies
  - *Mitigation*: Comprehensive cleanup in all code paths
  - *Detection*: Monitor active operations count after cancellation

### Medium Risk (User Experience)  
- **Cancellation not responsive during heavy I/O**
  - *Claude Analysis*: Long-running I/O operations may delay cancellation response
  - *Mitigation*: Check cancellation token in I/O loops
  - *Detection*: Manual testing with large file operations

### Low Risk (Edge Cases)
- **Race conditions between completion and cancellation**
  - *Claude Analysis*: Operation could complete just as user presses ESC
  - *Mitigation*: Proper synchronization and state checking
  - *Detection*: Stress testing with rapid operations

### Edge Cases (Claude-Identified)
- **ESC key conflict with other UI overlays**
- **Multiple rapid ESC key presses**
- **Cancellation during error conditions**
- **Network operations that can't be cancelled immediately**

## 7. Rollback Plan
**Failure Condition:** Cancellation causes crashes or incomplete cleanup  
**Rollback Steps:**
1. Remove ESC key cancellation handling
2. Revert UIState cancellation token methods
3. Remove operation cleanup from event loop
4. Operations continue without user cancellation option

## 8. Definition of Done (Claude-Enhanced)
### Code Quality
- [ ] All P0 success criteria met and tested
- [ ] Code passes cargo fmt + check + clippy + build
- [ ] Claude-identified edge cases handled
- [ ] Error scenarios analyzed and mitigated

### Integration Testing  
- [ ] Manual testing completed with various operation types
- [ ] ESC key integration with existing overlays verified
- [ ] Cancellation responsiveness measured and acceptable
- [ ] Resource cleanup validated with monitoring tools

### Documentation & Continuity
- [ ] Documentation updated in Design.md with technical details
- [ ] ADR created for cancellation architecture decisions
- [ ] Next phase specification prepared
- [ ] CLAUDE.md updated with any workflow improvements discovered

### Claude-Specific Validation
- [ ] Implementation leverages Claude's reasoning for error prevention
- [ ] Code generated via terminal diffs (not direct editing)
- [ ] Complex tasks tracked via TodoWrite throughout development
- [ ] Context window efficiently utilized during implementation

---

**This completes user-initiated cancellation for the robust file operations system.**

---

## Implementation Template (For Future Phases)

```markdown
# FSM Implementation Specification

**ACTIVE FEATURE:** Phase X.X: [Feature Name]

## 1. Executive Summary
**Objective:** [Clear objective statement]
**Priority:** [High/Medium/Low + business justification]
**Complexity:** [Low/Medium/High + technical factors]
**Dependencies:** [List with completion status]
**Estimated Effort:** [Time estimate]

## 2. Context & Background
**Problem:** [What problem are we solving]
**Current State:** [What exists today]
**Required:** [What needs to be built]
**Integration Point:** [Where this fits in the system]

## 3. Success Criteria
### Must Have (P0)
- [ ] [Critical functionality]

### Should Have (P1)  
- [ ] [Important but not blocking]

### Could Have (P2)
- [ ] [Nice to have features]

## 4. Technical Approach
**Architecture:** [High-level design approach]
**Data Flow:** [How data moves through the system]
**Performance:** [Performance considerations]
**Error Handling:** [Error handling strategy]

## 5. Testing Strategy
[Testing approach and requirements]

## 6. Risk Assessment
[Risks and mitigation strategies]

## 7. Rollback Plan
[How to rollback if things go wrong]

## 8. Definition of Done
[Checklist for completion]
```