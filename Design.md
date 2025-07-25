# FSM Design & Planning Document

## Major Improvement Analysis (2024-07-24)

### <� TIER 1: Critical Missing Features (High Impact)

**1. File Operations Suite (CRITICAL MISSING)** P **HIGHEST PRIORITY**

- **What's Missing:** Copy, move, rename, batch operations, undo/redo
- **Current State:** Only create/delete implemented
- **Value:** Essential for any file manager to be competitive
- **Implementation:** Add `Action::Copy`, `Action::Move`, `Action::Rename` with progress tracking

**2. Multi-Selection & Batch Operations**

- **What's Missing:** Visual selection, marking multiple files, batch delete/copy/move
- **Current State:** Single file selection only
- **Value:** Modern file managers require bulk operations
- **Implementation:** Extend `marked_indices` in UIState, add visual selection mode

**3. Plugin System (Stubbed but Empty)**

- **What's Missing:** Actual plugin loading, API implementation, example plugins
- **Current State:** `src/plugins/plugin_api.rs` is empty
- **Value:** Extensibility is mentioned in architecture but not implemented
- **Implementation:** Plugin trait system, dynamic loading, example plugins for git status, syntax highlighting

### <� TIER 2: UX & Productivity Enhancements (Medium-High Impact)

**4. Multi-Pane Support**

- **What's Missing:** Dual-pane view like orthodox file managers
- **Current State:** Single pane only, `active_pane` field exists but unused
- **Value:** Significantly improves file operations workflow
- **Implementation:** Split view rendering, pane switching, cross-pane operations

**5. Preview System**

- **What's Missing:** File preview pane (text, images, directories)
- **Current State:** No preview capability
- **Value:** Essential for modern file managers (lf, ranger have this)
- **Implementation:** Preview pane with syntax highlighting, image thumbnails, directory preview

**6. Advanced Navigation**

- **What's Missing:** Bookmarks, recent locations, path breadcrumbs, fuzzy path jumping
- **Current State:** Basic up/down navigation only
- **Value:** Significantly improves navigation efficiency
- **Implementation:** Bookmark system, history stack, fuzzy finder integration

**7. File Type Actions & Associations**

- **What's Missing:** Open with specific applications, custom actions per file type
- **Current State:** Basic "open with editor" only
- **Value:** Essential for productivity workflows
- **Implementation:** File association system, external command execution

### <� TIER 3: Polish & Advanced Features (Medium Impact)

**8. Themes & Customization**
**9. Sorting & Filtering**
**10. Progress & Status Indicators**

### <� TIER 4: Performance & Advanced Features (Lower Priority)

**11. Archive Support**
**12. Network Support**
**13. Git Integration**

---

## TIER 1 - STEP 1: Complete File Operations Suite

### Overview

Implement the missing core file operations (copy, move, rename) with progress tracking and error handling. This is the foundation that makes FSM a usable file manager.

### Current State Analysis

-  **Create**: `Action::CreateFile`, `Action::CreateDirectory` with input prompts
-  **Delete**: `Action::DeleteFile` with confirmation
- L **Copy**: Missing entirely
- L **Move**: Missing entirely
- L **Rename**: Missing entirely
- L **Progress Tracking**: No progress for long operations
- L **Undo/Redo**: No operation history

### Architecture Changes Required

#### 1. Action Enum Extensions (`src/controller/actions.rs`)

```rust
// Add to Action enum
Copy { source: PathBuf, dest: PathBuf },
Move { source: PathBuf, dest: PathBuf },
Rename { source: PathBuf, new_name: String },
BatchCopy { sources: Vec<PathBuf>, dest_dir: PathBuf },
BatchMove { sources: Vec<PathBuf>, dest_dir: PathBuf },
BatchDelete { paths: Vec<PathBuf> },

// Progress tracking
FileOperationProgress {
    operation_id: String,
    current: u64,
    total: u64,
    current_file: PathBuf
},
FileOperationComplete { operation_id: String, result: Result<(), AppError> },
FileOperationCancel { operation_id: String },
```

#### 2. New Task Types (`src/tasks/`)

```rust
// src/tasks/file_ops_task.rs (NEW FILE)
pub struct FileOperationTask {
    pub operation_id: String,
    pub operation: FileOperation,
    pub task_tx: mpsc::UnboundedSender<TaskResult>,
}

pub enum FileOperation {
    Copy { source: PathBuf, dest: PathBuf },
    Move { source: PathBuf, dest: PathBuf },
    Rename { source: PathBuf, new_name: String },
    BatchCopy { sources: Vec<PathBuf>, dest_dir: PathBuf },
    BatchMove { sources: Vec<PathBuf>, dest_dir: PathBuf },
}

impl FileOperationTask {
    pub async fn execute(&self) -> Result<(), AppError> {
        // Implementation with progress reporting
    }
}
```

#### 3. UI State Extensions (`src/model/ui_state.rs`)

```rust
// Add to UIState
pub file_operations: HashMap<String, FileOperationProgress>,
pub operation_history: Vec<CompletedOperation>, // For undo

pub struct FileOperationProgress {
    pub operation_type: String,
    pub current: u64,
    pub total: u64,
    pub current_file: PathBuf,
    pub start_time: std::time::Instant,
}

pub struct CompletedOperation {
    pub operation: String,
    pub timestamp: std::time::SystemTime,
    pub undo_action: Option<Action>, // For undo functionality
}
```

#### 4. New UI Components

```rust
// src/view/components/file_operations_overlay.rs (NEW FILE)
pub struct FileOperationsOverlay {
    operations: HashMap<String, FileOperationProgress>,
}

impl FileOperationsOverlay {
    pub fn render_progress_bars(&self, f: &mut Frame, area: Rect) {
        // Multi-operation progress display
        // Cancel buttons for active operations
        // ETA calculations
    }
}
```

#### 5. Handler Extensions (`src/controller/event_loop.rs`)

```rust
impl EventLoop {
    // Add to existing handlers
    pub async fn handle_file_operation(&mut self, action: Action) -> Result<(), AppError> {
        match action {
            Action::Copy { source, dest } => {
                self.start_file_operation(FileOperation::Copy { source, dest }).await?;
            }
            Action::Move { source, dest } => {
                self.start_file_operation(FileOperation::Move { source, dest }).await?;
            }
            Action::Rename { source, new_name } => {
                self.start_file_operation(FileOperation::Rename { source, new_name }).await?;
            }
            // ... batch operations
        }
        Ok(())
    }

    async fn start_file_operation(&mut self, operation: FileOperation) -> Result<(), AppError> {
        let operation_id = uuid::Uuid::new_v4().to_string();
        let task = FileOperationTask {
            operation_id: operation_id.clone(),
            operation,
            task_tx: self.task_tx.clone(),
        };

        // Update UI state to show operation started
        {
            let mut state = self.app_state.lock().unwrap();
            state.ui_state.file_operations.insert(operation_id, /* progress */);
        }

        // Spawn background task
        tokio::spawn(async move { task.execute().await });
        Ok(())
    }
}
```

### Key Bindings Extensions (`src/config.rs`)

```rust
// Add new keybindings
'c' | 'C' => Action::ShowInputPrompt(InputPromptType::CopyDestination),
'r' | 'R' => Action::ShowInputPrompt(InputPromptType::RenameFile),
'm' | 'M' => Action::ShowInputPrompt(InputPromptType::MoveDestination),

// For marked files (future multi-selection)
'C' => Action::BatchCopy { /* from marked files */ },
'M' => Action::BatchMove { /* from marked files */ },
```

### Implementation Phases

#### Phase 1: Basic Operations

1. Add Action enum variants
2. Implement basic copy/move/rename without progress
3. Add input prompts for destinations
4. Basic error handling

#### Phase 2: Progress Tracking

1. Add FileOperationTask with progress reporting
2. Implement progress UI overlay
3. Add cancellation support
4. Real-time progress updates

#### Phase 3: Batch Operations

1. Multi-selection implementation (prerequisite)
2. Batch operation support
3. Advanced progress tracking for multiple files
4. Conflict resolution (overwrite prompts)

#### Phase 4: Polish

1. Operation history for undo
2. Better error messages and recovery
3. Performance optimizations
4. Integration testing

### Success Criteria

- [x] Copy single file with `c` key + destination prompt
- [x] Move single file with `m` key + destination prompt
- [x] Rename file with `r` key + new name prompt
- [ ] Progress bars for long operations (Phase 2)
- [ ] Cancel ongoing operations with Escape (Phase 2)
- [x] Error handling with user-friendly messages
- [x] All operations work across different file systems
- [x] Memory efficient for large file operations

### Dependencies & Considerations

- **Tokio fs operations**: Use `tokio::fs` for async file I/O
- **Progress calculation**: File size-based progress for copies
- **Cross-platform**: Handle Windows/Unix path differences
- **Permissions**: Proper error handling for permission issues
- **Symlinks**: Preserve symlink behavior in copy/move
- **Large files**: Streaming copy for memory efficiency
- **Concurrent ops**: Multiple operations can run simultaneously

---

## ✅ **PHASE 1 IMPLEMENTATION COMPLETE** (2024-07-24)

### Implementation Workflow Summary

**Phase 1 successfully implemented basic file operations (copy/move/rename) with the following workflow:**

#### 1. **Action System Extension**

```rust
// Added to src/controller/actions.rs
pub enum InputPromptType {
    CopyDestination,    // ← NEW: For copy operations
    MoveDestination,    // ← NEW: For move operations
    RenameFile,         // ← NEW: For rename operations
}

pub enum Action {
    Copy { source: PathBuf, dest: PathBuf },      // ← NEW
    Move { source: PathBuf, dest: PathBuf },      // ← NEW
    Rename { source: PathBuf, new_name: String }, // ← NEW
}
```

#### 2. **Background Task System**

```rust
// Created src/tasks/file_ops_task.rs
pub struct FileOperationTask {
    pub operation_id: String,           // UUID for tracking
    pub operation: FileOperation,       // Copy/Move/Rename
    pub task_tx: mpsc::UnboundedSender<TaskResult>,
}

pub enum FileOperation {
    Copy { source: PathBuf, dest: PathBuf },
    Move { source: PathBuf, dest: PathBuf },
    Rename { source: PathBuf, new_name: String },
}
```

#### 3. **TaskResult Enum Extension**

```rust
// Extended src/controller/event_loop.rs TaskResult
pub enum TaskResult {
    Legacy { /* existing fields */ },
    FileOperationComplete {              // ← NEW
        operation_id: String,
        result: Result<(), AppError>,
    },
}
```

#### 4. **Key Bindings Implementation**

```rust
// Added to event_loop.rs navigation mode
(KeyCode::Char('c'), _) => Action::ShowInputPrompt(InputPromptType::CopyDestination),
(KeyCode::Char('m'), _) => Action::ShowInputPrompt(InputPromptType::MoveDestination),
(KeyCode::Char('r'), _) => Action::ShowInputPrompt(InputPromptType::RenameFile),
```

#### 5. **Input Prompt Processing**

```rust
// Added to event_loop.rs SubmitInputPrompt handler
Some(InputPromptType::CopyDestination) => {
    // Get selected file path → create Copy action → dispatch
}
Some(InputPromptType::MoveDestination) => {
    // Get selected file path → create Move action → dispatch
}
Some(InputPromptType::RenameFile) => {
    // Get selected file path → create Rename action → dispatch
}
```

#### 6. **File Operation Handlers**

```rust
// Added to event_loop.rs dispatch_action
Action::Copy { source, dest } => {
    let task = FileOperationTask::new(FileOperation::Copy { source, dest }, task_tx);
    tokio::spawn(async move { task.execute().await });
}
// Similar for Move and Rename
```

#### 7. **Async File Operations**

- **Copy**: Recursive directory copying with `tokio::fs::copy`
- **Move**: Efficient rename with fallback to copy+delete
- **Rename**: Simple rename within same directory
- **Boxing**: Used `Box::pin()` for recursive async functions

#### 8. **Task Completion Handling**

```rust
// Added to event_loop.rs task result processing
TaskResult::FileOperationComplete { operation_id, result } => {
    match result {
        Ok(()) => app.ui.show_info("File operation completed"),
        Err(e) => app.ui.show_error(format!("File operation failed: {e}")),
    }
}
```

### **Technical Challenges Resolved**

1. **Async Recursion**: Boxed recursive `copy_directory` function with proper lifetimes
2. **TaskResult Migration**: Changed from struct to enum, fixed all usage sites
3. **AppError Clone**: Manual Clone implementation for non-Clone fields (io::Error)
4. **UUID Dependency**: Added `uuid` crate for operation tracking
5. **Compilation Issues**: Fixed lifetime issues, enum variants, recursive boxing

### **Architecture Integration**

- **Non-blocking**: All operations run in background tasks via `tokio::spawn`
- **Progress Ready**: Task system ready for Phase 2 progress tracking
- **Error Propagation**: Full error chain from file ops → tasks → UI
- **State Management**: Operations tracked with unique IDs
- **UI Integration**: Seamless prompt → action → execution → feedback loop

### **User Experience**

**Current workflow:**

1. Navigate to file/directory
2. Press `c` (copy), `m` (move), or `r` (rename)
3. Enter destination/new name in prompt
4. Operation executes in background
5. Success/error notification displayed

**Next Phase:** Add progress bars and batch operations for multiple files.

---

## TIER 1 - STEP 2: Phase 2 Progress Tracking Implementation

### Overview

Implement real-time progress tracking for file operations (copy/move/rename) with visual progress bars, cancellation support, and performance monitoring. This enhances the user experience for large file operations by providing feedback and control.

### Current State Analysis

**✅ Completed (Phase 1):**
- `FileOperationTask` with unique operation IDs
- `TaskResult::FileOperationComplete` variant
- Background async processing via `tokio::spawn`
- Basic error handling and completion notification

**❌ Missing (Phase 2):**
- Real-time progress reporting during operations
- Visual progress indicators in UI
- Operation cancellation support
- ETA calculations and performance metrics
- Multi-operation progress tracking

### Architecture Changes Required

#### 1. TaskResult Extensions (`src/controller/event_loop.rs`)

```rust
// Add new TaskResult variant for progress updates
pub enum TaskResult {
    Legacy { /* existing fields */ },
    FileOperationComplete { /* existing fields */ },
    
    // NEW: Real-time progress reporting
    FileOperationProgress {
        operation_id: String,
        operation_type: String,        // "copy", "move", "rename"
        current_bytes: u64,           // Bytes processed so far
        total_bytes: u64,             // Total bytes to process
        current_file: PathBuf,        // Currently processing file
        files_completed: u32,         // Files completed
        total_files: u32,             // Total files to process
        start_time: std::time::Instant, // For ETA calculation
        throughput_bps: Option<u64>,  // Bytes per second
    },
}
```

#### 2. Action Extensions (`src/controller/actions.rs`)

```rust
// Add cancellation support
pub enum Action {
    // ... existing variants
    
    // NEW: Cancel ongoing file operation
    CancelFileOperation { operation_id: String },
}
```

#### 3. UI State Extensions (`src/model/ui_state.rs`)

```rust
// Add progress tracking to UIState
pub struct UIState {
    // ... existing fields
    
    // NEW: Track active file operations
    pub active_file_operations: HashMap<String, FileOperationProgress>,
}

// NEW: Progress tracking structure
#[derive(Debug, Clone)]
pub struct FileOperationProgress {
    pub operation_type: String,       // "copy", "move", "rename"
    pub current_bytes: u64,
    pub total_bytes: u64,
    pub current_file: PathBuf,
    pub files_completed: u32,
    pub total_files: u32,
    pub start_time: std::time::Instant,
    pub throughput_bps: Option<u64>,
    pub estimated_completion: Option<std::time::Instant>,
}
```

#### 4. FileOperationTask Enhancement (`src/tasks/file_ops_task.rs`)

```rust
impl FileOperationTask {
    // NEW: Progress calculation and reporting
    async fn calculate_operation_size(&self) -> Result<(u64, u32), AppError> {
        // Calculate total bytes and file count for progress tracking
    }
    
    async fn report_progress(&self, 
        current_bytes: u64, 
        total_bytes: u64,
        current_file: &Path,
        files_completed: u32,
        total_files: u32
    ) -> Result<(), AppError> {
        // Send progress update to UI
    }
    
    // Enhanced copy with progress reporting
    async fn copy_file_with_progress(&self, source: &Path, dest: &Path) -> Result<(), AppError> {
        // Stream copy with periodic progress updates
    }
}
```

#### 5. New UI Component (`src/view/components/file_operations_overlay.rs`)

```rust
// NEW FILE: Progress overlay component
pub struct FileOperationsOverlay {
    operations: HashMap<String, FileOperationProgress>,
}

impl FileOperationsOverlay {
    pub fn render_progress_bars(&self, f: &mut Frame, area: Rect) {
        // Multi-operation progress display with:
        // - Progress bars with percentage
        // - Current file indicator
        // - ETA and throughput display
        // - Cancel buttons/instructions
    }
    
    pub fn render_single_operation(&self, f: &mut Frame, area: Rect, progress: &FileOperationProgress) {
        // Single operation progress bar
        // Format: "Copying file.txt... [████████████████████████████████] 75% (2.1 MB/s, 30s remaining)"
    }
}
```

#### 6. Cancellation Token System

```rust
// Add to FileOperationTask
use tokio_util::sync::CancellationToken;

pub struct FileOperationTask {
    pub operation_id: String,
    pub operation: FileOperation,
    pub task_tx: mpsc::UnboundedSender<TaskResult>,
    pub cancel_token: CancellationToken, // NEW: Cancellation support
}
```

### Implementation Phases

#### Phase 2.1: Progress Infrastructure
1. ✅ Add `TaskResult::FileOperationProgress` variant
2. ✅ Add `Action::CancelFileOperation` variant  
3. ✅ Extend UIState with progress tracking
4. ✅ Update event loop to handle progress updates

#### Phase 2.2: Task Progress Reporting
1. ✅ Enhance FileOperationTask with size calculation
2. ✅ Implement streaming copy with progress updates
3. ✅ Add periodic progress reporting during operations
4. ✅ Handle cancellation token integration

#### Phase 2.3: UI Progress Display
1. ✅ Create FileOperationsOverlay component
2. ✅ Implement progress bar rendering
3. ✅ Add ETA and throughput calculations
4. ✅ Integrate overlay with main UI

#### Phase 2.4: Cancellation & Polish
1. ✅ Add Escape key cancellation handling
2. ✅ Implement graceful operation cleanup
3. ✅ Error handling for cancelled operations
4. ✅ Performance optimization and testing

### Success Criteria

- [ ] **Real-time Progress**: Visual progress bars update during long operations
- [ ] **Multiple Operations**: Support concurrent operations with separate progress bars
- [ ] **Cancellation**: Escape key cancels current operation gracefully
- [ ] **Performance Metrics**: Display throughput (MB/s) and ETA
- [ ] **File-level Progress**: Show current file being processed
- [ ] **Error Resilience**: Handle cancellation and errors without UI corruption
- [ ] **Memory Efficient**: Progress tracking doesn't significantly increase memory usage
- [ ] **Responsive UI**: Progress updates don't block UI rendering (60fps target)

### Technical Considerations

#### Performance Optimizations
- **Batched Updates**: Limit progress updates to ~10Hz to avoid UI spam
- **Streaming Copy**: Use buffered copying to avoid loading entire files into memory
- **Background Calculation**: Pre-calculate operation size in background thread
- **Efficient Rendering**: Only redraw progress areas when values change

#### Cross-Platform Compatibility
- **File Size Calculation**: Handle symlinks and special files correctly
- **Cancellation**: Ensure proper cleanup on Windows/Unix systems
- **Path Display**: Truncate long paths for UI display

#### Error Handling
- **Partial Operations**: Handle cancellation mid-file gracefully
- **Permission Errors**: Continue operation on individual file failures
- **Disk Space**: Detect and handle insufficient disk space
- **Network Operations**: Handle slow network filesystem operations

#### Memory Management  
- **Progress Buffer**: Limit active operation tracking to prevent memory growth
- **Cleanup**: Automatic cleanup of completed operation progress data
- **Efficient Updates**: Minimize allocations during progress reporting

---
