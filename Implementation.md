# FSM Implementation Specification

**ACTIVE FEATURE:** Phase 3.2: Basic Copy/Move Operations with Key Bindings

## 1. Executive Summary
**Objective:** Implement intuitive c/x/v key bindings for copy/move/paste operations using the extreme performance clipr crate  
**Priority:** High (core UX improvement eliminating manual path entry)  
**Complexity:** Medium (key binding integration + clipboard operations)  
**Dependencies:** Phase 3.1 Extreme Performance Clipboard Infrastructure (✅ Complete)  
**Estimated Effort:** 1-2 development sessions
**Current Status:** 🚀 Ready to implement with production-ready clipr foundation

## 2. Context & Background
**Problem:** Current copy operation requires manual destination path entry, poor UX compared to modern file managers  
**Current State:** `InputPromptType::CopyDestination` requires typing full paths  
**Required:** Modern clipboard UX with c (copy), x (cut/move), v (paste) key bindings  
**Integration Point:** Event loop key handling + extreme performance clipr clipboard operations  

## 3. Success Criteria
### Must Have (P0)
- [ ] **Copy Key Binding**: 'c' key adds selected file to clipboard as copy operation
- [ ] **Move Key Binding**: 'x' key adds selected file to clipboard as move operation  
- [ ] **Paste Key Binding**: 'v' key pastes clipboard items to current directory
- [ ] **Visual Feedback**: Show confirmation when items added to clipboard
- [ ] **Error Handling**: Graceful handling of clipboard and paste operation failures
- [ ] **Multiple Items**: Support pasting multiple clipboard items in sequence

### Should Have (P1)  
- [ ] **Clipboard Indicators**: Visual indicators showing clipboard has items
- [ ] **Operation Type Display**: Show whether items are marked for copy or move
- [ ] **Duplicate Handling**: Smart handling when pasting to directories with existing files
- [ ] **Progress Integration**: Long paste operations show progress via existing progress system

### Could Have (P2)
- [ ] **Selective Paste**: Choose specific clipboard items to paste (future enhancement)
- [ ] **Clipboard Overlay**: View clipboard contents with 'cb' or similar key sequence

## 4. Technical Approach
**Architecture:** Event loop key binding integration with extreme performance clipr operations  
**Data Flow:** Key press → clipr API call → background file operation → progress tracking  
**Performance:** Leverages lock-free clipboard for zero-allocation item management  
**Error Handling:** Comprehensive error propagation from clipr to user notifications  

## Implementation Specification

### 1. Key Binding Integration
```rust
// fsm-core/src/controller/event_loop.rs - Add clipboard key bindings
impl EventLoop {
    async fn handle_key_event(&mut self, key: KeyEvent) -> Result<(), AppError> {
        match key.code {
            // ... existing key handling ...
            
            KeyCode::Char('c') if !self.is_input_mode().await => {
                self.dispatch_action(Action::CopyToClipboard).await?;
            }
            
            KeyCode::Char('x') if !self.is_input_mode().await => {
                self.dispatch_action(Action::MoveToClipboard).await?;
            }
            
            KeyCode::Char('v') if !self.is_input_mode().await => {
                self.dispatch_action(Action::PasteFromClipboard).await?;
            }
            
            // ... rest of key handling ...
        }
        
        Ok(())
    }
}
```

### 2. Action Enum Extensions
```rust
// fsm-core/src/controller/actions.rs - Add clipboard actions
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    // ... existing actions ...
    
    /// Copy selected file to clipboard
    CopyToClipboard,
    
    /// Move selected file to clipboard (cut operation)
    MoveToClipboard,
    
    /// Paste all clipboard items to current directory
    PasteFromClipboard,
    
    /// Paste specific clipboard item (for future selective paste)
    PasteClipboardItem { item_id: u64 },
}
```

### 3. Action Handler Implementation
```rust
// fsm-core/src/controller/event_loop.rs - Clipboard action handlers
impl EventLoop {
    async fn handle_copy_to_clipboard(&mut self) -> Result<(), AppError> {
        let mut app = self.app.lock().await;
        
        let selected_path = app.fs.get_selected_path()
            .ok_or_else(|| AppError::user_error("No file selected"))?;
        
        match app.ui.clipboard.add_copy(selected_path.clone()).await {
            Ok(id) => {
                let filename = selected_path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Unknown");
                    
                app.ui.show_success(format!("Copied '{}' to clipboard", filename));
                tracing::info!("Added file to clipboard: {} (ID: {})", selected_path.display(), id);
            }
            Err(e) => {
                app.ui.show_error(format!("Failed to copy to clipboard: {}", e));
                tracing::error!("Clipboard copy failed: {} - {}", selected_path.display(), e);
            }
        }
        
        Ok(())
    }
    
    async fn handle_move_to_clipboard(&mut self) -> Result<(), AppError> {
        let mut app = self.app.lock().await;
        
        let selected_path = app.fs.get_selected_path()
            .ok_or_else(|| AppError::user_error("No file selected"))?;
        
        match app.ui.clipboard.add_move(selected_path.clone()).await {
            Ok(id) => {
                let filename = selected_path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Unknown");
                    
                app.ui.show_success(format!("Cut '{}' to clipboard", filename));
                tracing::info!("Added file to clipboard for move: {} (ID: {})", selected_path.display(), id);
            }
            Err(e) => {
                app.ui.show_error(format!("Failed to cut to clipboard: {}", e));
                tracing::error!("Clipboard cut failed: {} - {}", selected_path.display(), e);
            }
        }
        
        Ok(())
    }
    
    async fn handle_paste_from_clipboard(&mut self) -> Result<(), AppError> {
        let app = self.app.lock().await;
        
        let current_dir = app.fs.current_directory().clone();
        let clipboard_items = app.ui.clipboard.items().await;
        
        if clipboard_items.is_empty() {
            app.ui.show_info("Clipboard is empty");
            return Ok(());
        }
        
        drop(app); // Release lock before spawning tasks
        
        // Process each clipboard item
        for item in clipboard_items {
            let paste_op = {
                let app = self.app.lock().await;
                app.ui.clipboard.get_paste_operation(item.id, current_dir.clone()).await
            };
            
            match paste_op {
                Ok(operation) => {
                    // Spawn background file operation task
                    self.spawn_paste_operation(operation).await?;
                }
                Err(e) => {
                    let app = self.app.lock().await;
                    app.ui.show_error(format!("Failed to create paste operation: {}", e));
                    tracing::error!("Paste operation creation failed: {}", e);
                }
            }
        }
        
        let app = self.app.lock().await;
        app.ui.show_info(format!("Started pasting {} item(s)", clipboard_items.len()));
        
        Ok(())
    }
    
    async fn spawn_paste_operation(&mut self, operation: clipr::PasteOperation) -> Result<(), AppError> {
        use crate::tasks::file_ops_task::{FileOperationTask, FileOperation};
        
        let file_operation = match operation.operation_type {
            clipr::ClipBoardOperation::Copy => {
                FileOperation::Copy {
                    source: PathBuf::from(operation.source_path.as_str()),
                    dest: PathBuf::from(operation.destination_path.as_str()),
                }
            }
            clipr::ClipBoardOperation::Move => {
                FileOperation::Move {
                    source: PathBuf::from(operation.source_path.as_str()),
                    dest: PathBuf::from(operation.destination_path.as_str()),
                }
            }
        };
        
        let task = FileOperationTask {
            operation_id: operation.operation_id.to_string(),
            operation: file_operation,
            app: Arc::clone(&self.app),
            task_tx: self.task_tx.clone(),
            cancel_token: CancellationToken::new(),
        };
        
        // Store cancellation token for ESC key handling
        {
            let app = self.app.lock().await;
            app.ui.store_cancel_token(task.operation_id.clone(), task.cancel_token.clone());
        }
        
        tokio::spawn(async move {
            if let Err(e) = task.execute().await {
                tracing::error!("Paste operation failed: {}", e);
            }
        });
        
        Ok(())
    }
}
```

### 4. Clipboard Status Integration
```rust
// fsm-core/src/model/ui_state.rs - Add clipboard status methods
impl UIState {
    /// Get clipboard status for status bar display
    pub fn clipboard_status(&self) -> Option<String> {
        let stats = self.clipboard.stats();
        
        if stats.total_items > 0 {
            Some(format!("CB:{}", stats.total_items))
        } else {
            None
        }
    }
    
    /// Show clipboard contents summary  
    pub fn clipboard_summary(&self) -> String {
        let stats = self.clipboard.stats();
        
        if stats.total_items == 0 {
            "Clipboard is empty".to_string()
        } else {
            format!(
                "Clipboard: {} items ({} copy, {} move)",
                stats.total_items,
                stats.copy_items, 
                stats.move_items
            )
        }
    }
}
```

### 5. Status Bar Integration
```rust
// fsm-core/src/view/ui.rs - Add clipboard status to status bar
impl UI {
    fn render_status_bar(&self, frame: &mut Frame, area: Rect, app_state: &AppState) {
        let mut status_parts = Vec::new();
        
        // ... existing status parts ...
        
        // Add clipboard status
        if let Some(clipboard_status) = app_state.ui.clipboard_status() {
            status_parts.push(format!(" {} ", clipboard_status));
        }
        
        // ... render status parts ...
    }
}
```

### 6. Enhanced Error Handling
```rust
// fsm-core/src/controller/event_loop.rs - Clipboard-specific error handling
impl EventLoop {
    /// Handle clipboard-related errors with user-friendly messages
    fn handle_clipboard_error(&mut self, error: clipr::ClipError, operation: &str) {
        let user_message = match error {
            clipr::ClipError::ItemNotFound(_) => {
                "Clipboard item no longer exists".to_string()
            }
            clipr::ClipError::DuplicateItem { path } => {
                format!("'{}' is already in clipboard", path.as_str())
            }
            clipr::ClipError::ClipboardFull { max, current } => {
                format!("Clipboard is full ({}/{} items)", current, max)
            }
            clipr::ClipError::MetadataError { path, .. } => {
                format!("Cannot access file metadata for '{}'", path.as_str())
            }
            _ => {
                format!("Clipboard {} operation failed: {}", operation, error)
            }
        };
        
        let app = self.app.lock().await;
        app.ui.show_error(user_message);
    }
}
```

## Success Criteria Checklist
- [ ] **Copy Key Binding**: 'c' key adds selected file to clipboard as copy operation
- [ ] **Move Key Binding**: 'x' key adds selected file to clipboard as move operation  
- [ ] **Paste Key Binding**: 'v' key pastes clipboard items to current directory
- [ ] **Visual Feedback**: Show confirmation when items added to clipboard
- [ ] **Error Handling**: Graceful handling of clipboard and paste operation failures
- [ ] **Multiple Items**: Support pasting multiple clipboard items in sequence
- [ ] **Clipboard Indicators**: Visual indicators showing clipboard has items
- [ ] **Operation Type Display**: Show whether items are marked for copy or move
- [ ] **Duplicate Handling**: Smart handling when pasting to directories with existing files
- [ ] **Progress Integration**: Long paste operations show progress via existing progress system

## Technical Requirements
- **Performance**: Leverage lock-free clipboard for zero-allocation operations
- **Memory**: Efficient clipboard item management with automatic cleanup
- **Thread Safety**: All clipboard operations safe for concurrent access
- **Integration**: Seamless integration with existing progress tracking system
- **Error Handling**: Comprehensive error coverage with user-friendly messages

## Testing Strategy
1. **Basic Operations**: Copy file with 'c', paste with 'v', verify file copied
2. **Move Operations**: Cut file with 'x', paste with 'v', verify file moved
3. **Multiple Items**: Add multiple items to clipboard, paste all at once
4. **Error Cases**: Test clipboard full, invalid files, permission errors
5. **Progress Integration**: Test paste operations with progress overlay
6. **Cancellation**: Test ESC key cancellation during paste operations

## Risk Assessment
### High Risk (Integration)
- **Event Loop Complexity**: Adding key bindings may conflict with existing handlers  
  - *Mitigation*: Careful key binding precedence and comprehensive testing
  - *Detection*: Manual testing of all key combinations and input modes

### Medium Risk (User Experience)  
- **Clipboard State Confusion**: Users may forget what's in clipboard  
  - *Mitigation*: Clear visual indicators and status bar integration
  - *Detection*: User testing and feedback on clipboard visibility

### Low Risk (Performance)
- **Clipboard Operation Overhead**: Frequent clipboard access may impact performance  
  - *Mitigation*: Leverage extreme performance clipr implementation
  - *Detection*: Performance monitoring and benchmarking

## Rollback Plan
**Failure Condition:** Key bindings cause conflicts or clipboard integration fails  
**Rollback Steps:**
1. Remove new key bindings from event loop
2. Revert Action enum to remove clipboard actions
3. Remove clipboard integration from UIState display
4. Fall back to existing copy operation with manual path entry
5. Plan alternative approach for clipboard UX

## Definition of Done
### Code Quality
- [ ] All P0 success criteria implemented and tested
- [ ] Code passes cargo fmt + check + clippy + build for workspace
- [ ] Key bindings work in all UI modes without conflicts
- [ ] Clipboard operations integrate seamlessly with progress system
- [ ] Comprehensive error handling with user-friendly messages

### Integration Testing  
- [ ] Manual testing of all copy/move/paste scenarios
- [ ] Key binding integration verified across different UI states
- [ ] Progress tracking confirmed for long paste operations
- [ ] ESC key cancellation works with clipboard paste operations
- [ ] Status bar clipboard indicators display correctly

### Documentation & Continuity
- [ ] Design.md updated with Phase 3.2 completion details
- [ ] Implementation.md prepared for Phase 3.3 (Clipboard Overlay UI)
- [ ] CLAUDE.md updated with new clipboard workflow patterns
- [ ] Performance metrics documented for clipboard operations

---

**This implements the core copy/paste UX that transforms the file manager from manual path entry to modern clipboard operations, leveraging the extreme performance foundation established in Phase 3.1.**