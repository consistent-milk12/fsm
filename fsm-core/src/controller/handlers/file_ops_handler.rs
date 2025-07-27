// fsm-core/src/controller/handlers/file_ops_handler.rs

use crate::controller::{
    actions::{Action, InputPromptType, OperationId},
    event_processor::{Event, EventHandler},
};
use crate::error::AppError;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, trace, warn};

/// Specialized file operations handler for create, delete, rename, copy, move operations
/// Focused exclusively on file manipulation - navigation removed to avoid conflicts
pub struct FileOpsHandler {
    /// File operation key bindings (conflict-free)
    bindings: HashMap<KeyEvent, Action>,

    /// Current operation mode
    mode: FileOpsMode,

    /// Input buffer for file names and paths
    input_buffer: String,

    /// Pending operation context
    pending_operation: Option<PendingOperation>,

    /// Multi-key sequence buffer for advanced operations
    sequence_buffer: Vec<KeyEvent>,

    /// Operation statistics
    operation_count: u64,
}

/// File operations modes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOpsMode {
    /// Normal mode - handle file operation triggers
    Normal,

    /// Creating new file - accepting filename input
    CreatingFile,

    /// Creating new directory - accepting directory name input
    CreatingDirectory,

    /// Renaming existing item - accepting new name input
    Renaming,

    /// Copy/move operation - accepting destination path input
    AwaitingDestination,

    /// Batch operations mode - multiple selections
    BatchMode,
}

/// Pending operation details
#[derive(Debug, Clone)]
struct PendingOperation {
    operation_type: OperationType,
    source_path: PathBuf,
    operation_id: OperationId,

    #[allow(unused)]
    batch_items: Vec<PathBuf>,
}

#[allow(unused)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OperationType {
    Copy,

    Move,

    Delete,

    Rename,
}

impl Default for FileOpsHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl FileOpsHandler {
    /// Create new file operations handler
    pub fn new() -> Self {
        let mut bindings = HashMap::with_capacity(15);

        // ===== CORE FILE OPERATIONS =====
        // Remove navigation conflicts (h, l, Enter, Backspace) - handled by NavigationHandler
        // Remove clipboard conflicts (x, y) - handled by ClipboardHandler

        // File creation operations
        bindings.insert(key('n'), Action::CreateFile);
        bindings.insert(key('N'), Action::CreateFile); // Alternative
        bindings.insert(key('m'), Action::CreateDirectory);
        bindings.insert(key('M'), Action::CreateDirectory); // Alternative

        // File modification operations
        bindings.insert(key('d'), Action::Delete);
        bindings.insert(key('D'), Action::Delete); // Alternative
        bindings.insert(key('r'), Action::ShowInputPrompt(InputPromptType::Rename));
        bindings.insert(key('R'), Action::ShowInputPrompt(InputPromptType::Rename)); // Alternative

        // File attribute operations
        bindings.insert(key('.'), Action::ToggleShowHidden);

        // Copy/Move operations (using different keys to avoid clipboard conflicts)
        bindings.insert(
            key('C'),
            Action::StartCopy {
                source: PathBuf::new(),
            },
        ); // Capital C for copy
        bindings.insert(
            key('X'),
            Action::StartMove {
                source: PathBuf::new(),
            },
        ); // Capital X for move

        // Function key alternatives
        bindings.insert(
            function_key(2),
            Action::ShowInputPrompt(InputPromptType::Rename),
        ); // F2
        bindings.insert(function_key(7), Action::CreateDirectory); // F7
        bindings.insert(function_key(8), Action::Delete); // F8

        // Ctrl combinations for advanced operations
        bindings.insert(ctrl('n'), Action::CreateFile); // Ctrl+N
        bindings.insert(ctrl('d'), Action::Delete); // Ctrl+D

        Self {
            bindings,
            mode: FileOpsMode::Normal,
            input_buffer: String::with_capacity(128),
            pending_operation: None,
            sequence_buffer: Vec::with_capacity(3),
            operation_count: 0,
        }
    }

    /// Handle file operations key events
    fn handle_key(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        trace!(
            "FileOpsHandler: processing key {:?} in mode {:?}",
            key_event, self.mode
        );

        match self.mode {
            FileOpsMode::Normal => self.handle_normal_mode(key_event),

            FileOpsMode::CreatingFile => self.handle_file_creation_mode(key_event),

            FileOpsMode::CreatingDirectory => self.handle_directory_creation_mode(key_event),

            FileOpsMode::Renaming => self.handle_rename_mode(key_event),

            FileOpsMode::AwaitingDestination => self.handle_destination_mode(key_event),

            FileOpsMode::BatchMode => self.handle_batch_mode(key_event),
        }
    }

    /// Handle keys in normal file operations mode
    fn handle_normal_mode(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        // Check for multi-key sequences first
        if let Some(action) = self.check_sequences(key_event) {
            return Ok(vec![action]);
        }

        // Direct binding lookup
        if let Some(action) = self.bindings.get(&key_event).cloned() {
            debug!("FileOpsHandler: matched key to action {:?}", action);

            // Handle mode transitions and operation tracking
            match &action {
                Action::CreateFile => {
                    self.mode = FileOpsMode::CreatingFile;
                    self.input_buffer.clear();
                    debug!("FileOpsHandler: entering file creation mode");
                }
                Action::CreateDirectory => {
                    self.mode = FileOpsMode::CreatingDirectory;
                    self.input_buffer.clear();
                    debug!(
                        "FileOpsHandler: entering directory creation
  mode"
                    );
                }
                Action::ShowInputPrompt(InputPromptType::Rename) => {
                    self.mode = FileOpsMode::Renaming;
                    self.input_buffer.clear();
                    debug!("FileOpsHandler: entering rename mode");
                }
                Action::StartCopy { .. } => {
                    self.mode = FileOpsMode::AwaitingDestination;
                    self.pending_operation = Some(PendingOperation {
                        operation_type: OperationType::Copy,
                        source_path: PathBuf::new(), // Will be set from current selection
                        operation_id: OperationId::new(),
                        batch_items: Vec::new(),
                    });
                    self.input_buffer.clear();
                    debug!(
                        "FileOpsHandler: entering copy destination
  mode"
                    );
                }
                Action::StartMove { .. } => {
                    self.mode = FileOpsMode::AwaitingDestination;
                    self.pending_operation = Some(PendingOperation {
                        operation_type: OperationType::Move,
                        source_path: PathBuf::new(), // Will be set from current selection
                        operation_id: OperationId::new(),
                        batch_items: Vec::new(),
                    });
                    self.input_buffer.clear();
                    debug!(
                        "FileOpsHandler: entering move destination
  mode"
                    );
                }
                Action::Delete => {
                    self.operation_count += 1;
                    debug!("FileOpsHandler: delete operation #{}", self.operation_count);
                }
                _ => {}
            }

            Ok(vec![action])
        } else {
            Ok(vec![])
        }
    }

    /// Handle keys during file creation
    fn handle_file_creation_mode(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        match key_event.code {
            KeyCode::Esc => {
                debug!("FileOpsHandler: cancelled file creation");
                self.mode = FileOpsMode::Normal;
                self.input_buffer.clear();

                Ok(vec![Action::CloseOverlay])
            }
            KeyCode::Enter => {
                if self.input_buffer.trim().is_empty() {
                    warn!("FileOpsHandler: empty filename provided");
                    Ok(vec![]) // Don't create file with empty name
                } else {
                    debug!("FileOpsHandler: creating file '{}'", self.input_buffer);
                    let filename = self.input_buffer.clone();
                    self.mode = FileOpsMode::Normal;
                    self.input_buffer.clear();
                    self.operation_count += 1;

                    Ok(vec![Action::CreateFileWithName(filename)])
                }
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();

                Ok(vec![Action::Key(key_event)]) // Forward for UI update
            }
            KeyCode::Char(c) => {
                // Validate filename characters
                if is_valid_filename_char(c) {
                    self.input_buffer.push(c);

                    Ok(vec![Action::Key(key_event)]) // Forward for UI update
                } else {
                    debug!(
                        "FileOpsHandler: invalid filename character
  '{}'",
                        c
                    );

                    Ok(vec![]) // Ignore invalid characters
                }
            }
            KeyCode::Tab => {
                // Auto-complete filename (future enhancement)
                debug!("FileOpsHandler: filename auto-complete requested");

                Ok(vec![Action::Key(key_event)])
            }

            _ => Ok(vec![Action::Key(key_event)]), // Forward other keys
        }
    }

    /// Handle keys during directory creation
    fn handle_directory_creation_mode(
        &mut self,
        key_event: KeyEvent,
    ) -> Result<Vec<Action>, AppError> {
        match key_event.code {
            KeyCode::Esc => {
                debug!("FileOpsHandler: cancelled directory creation");
                self.mode = FileOpsMode::Normal;
                self.input_buffer.clear();

                Ok(vec![Action::CloseOverlay])
            }
            KeyCode::Enter => {
                if self.input_buffer.trim().is_empty() {
                    warn!("FileOpsHandler: empty directory name provided");
                    Ok(vec![])
                } else {
                    debug!("FileOpsHandler: creating directory '{}'", self.input_buffer);
                    let dirname = self.input_buffer.clone();
                    self.mode = FileOpsMode::Normal;
                    self.input_buffer.clear();
                    self.operation_count += 1;

                    Ok(vec![Action::CreateDirectoryWithName(dirname)])
                }
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();

                Ok(vec![Action::Key(key_event)])
            }
            KeyCode::Char(c) => {
                if is_valid_filename_char(c) {
                    self.input_buffer.push(c);

                    Ok(vec![Action::Key(key_event)])
                } else {
                    debug!(
                        "FileOpsHandler: invalid directory name
  character '{}'",
                        c
                    );

                    Ok(vec![])
                }
            }
            KeyCode::Tab => {
                debug!(
                    "FileOpsHandler: directory name auto-complete
  requested"
                );

                Ok(vec![Action::Key(key_event)])
            }

            _ => Ok(vec![Action::Key(key_event)]),
        }
    }

    /// Handle keys during rename operation
    fn handle_rename_mode(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        match key_event.code {
            KeyCode::Esc => {
                debug!("FileOpsHandler: cancelled rename");
                self.mode = FileOpsMode::Normal;
                self.input_buffer.clear();

                Ok(vec![Action::CloseOverlay])
            }
            KeyCode::Enter => {
                if self.input_buffer.trim().is_empty() {
                    warn!("FileOpsHandler: empty new name provided");

                    Ok(vec![])
                } else {
                    debug!("FileOpsHandler: renaming to '{}'", self.input_buffer);
                    let new_name = self.input_buffer.clone();
                    self.mode = FileOpsMode::Normal;
                    self.input_buffer.clear();
                    self.operation_count += 1;

                    Ok(vec![Action::RenameEntry(new_name)])
                }
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();

                Ok(vec![Action::Key(key_event)])
            }
            KeyCode::Char(c) => {
                if is_valid_filename_char(c) {
                    self.input_buffer.push(c);

                    Ok(vec![Action::Key(key_event)])
                } else {
                    debug!("FileOpsHandler: invalid rename character '{}'", c);

                    Ok(vec![])
                }
            }
            KeyCode::Tab => {
                debug!("FileOpsHandler: rename auto-complete requested");

                Ok(vec![Action::Key(key_event)])
            }

            _ => Ok(vec![Action::Key(key_event)]),
        }
    }

    /// Handle keys when awaiting destination for copy/move
    fn handle_destination_mode(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        match key_event.code {
            KeyCode::Esc => {
                debug!("FileOpsHandler: cancelled copy/move operation");
                self.mode = FileOpsMode::Normal;
                self.input_buffer.clear();
                self.pending_operation = None;

                Ok(vec![Action::CloseOverlay])
            }
            KeyCode::Enter => {
                if self.input_buffer.trim().is_empty() {
                    warn!(
                        "FileOpsHandler: empty destination path
  provided"
                    );

                    Ok(vec![])
                } else {
                    debug!(
                        "FileOpsHandler: executing operation to '{}'",
                        self.input_buffer
                    );
                    let destination: PathBuf = PathBuf::from(self.input_buffer.clone());
                    let result: Vec<Action> = if let Some(pending) = &self.pending_operation {
                        match pending.operation_type {
                            OperationType::Copy => vec![Action::ExecuteCopy {
                                operation_id: pending.operation_id.clone(),
                                source: pending.source_path.clone(),
                                destination,
                            }],
                            OperationType::Move => vec![Action::ExecuteMove {
                                operation_id: pending.operation_id.clone(),
                                source: pending.source_path.clone(),
                                destination,
                            }],
                            _ => vec![Action::NoOp],
                        }
                    } else {
                        vec![Action::NoOp]
                    };

                    self.mode = FileOpsMode::Normal;
                    self.input_buffer.clear();
                    self.pending_operation = None;
                    self.operation_count += 1;

                    Ok(result)
                }
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
                Ok(vec![Action::Key(key_event)])
            }
            KeyCode::Char(c) => {
                // Allow path characters
                if is_valid_path_char(c) {
                    self.input_buffer.push(c);

                    Ok(vec![Action::Key(key_event)])
                } else {
                    debug!("FileOpsHandler: invalid path character '{}'", c);

                    Ok(vec![])
                }
            }
            KeyCode::Tab => {
                debug!("FileOpsHandler: path auto-complete requested");

                Ok(vec![Action::Key(key_event)])
            }

            _ => Ok(vec![Action::Key(key_event)]),
        }
    }

    /// Handle keys in batch operations mode
    fn handle_batch_mode(&mut self, key_event: KeyEvent) -> Result<Vec<Action>, AppError> {
        match key_event.code {
            KeyCode::Esc => {
                debug!("FileOpsHandler: exiting batch mode");
                self.mode = FileOpsMode::Normal;
                self.pending_operation = None;

                Ok(vec![Action::CloseOverlay])
            }
            KeyCode::Enter => {
                debug!("FileOpsHandler: executing batch operation");
                // Execute batch operation
                self.mode = FileOpsMode::Normal;
                self.pending_operation = None;
                self.operation_count += 1;

                Ok(vec![Action::NoOp]) // Placeholder for batch execution
            }

            _ => Ok(vec![Action::Key(key_event)]),
        }
    }

    /// Check for multi-key file operation sequences
    fn check_sequences(&mut self, key_event: KeyEvent) -> Option<Action> {
        // Add key to sequence buffer for certain operations
        if matches!(key_event.code, KeyCode::Char('d' | 'g' | 'y')) {
            self.sequence_buffer.push(key_event);
        }

        let action: Option<Action> = match self.sequence_buffer.as_slice() {
            // 'dd' - delete (vim-style)
            [
                KeyEvent {
                    code: KeyCode::Char('d'),
                    ..
                },
                KeyEvent {
                    code: KeyCode::Char('d'),
                    ..
                },
            ] => Some(Action::Delete),

            // 'gg' - go to first (handled by NavigationHandler, but might be used for batch selection)
            [
                KeyEvent {
                    code: KeyCode::Char('g'),
                    ..
                },
                KeyEvent {
                    code: KeyCode::Char('g'),
                    ..
                },
            ] => Some(Action::SelectFirst),

            _ => None,
        };

        // Clear buffer if sequence matched or buffer full
        if action.is_some() || self.sequence_buffer.len() >= 2 {
            self.sequence_buffer.clear();
        }

        if action.is_some() {
            debug!("FileOpsHandler: multi-key sequence matched: {:?}", action);
        }

        action
    }

    /// Reset file operations handler state
    pub fn reset(&mut self) {
        debug!("FileOpsHandler: resetting state");
        self.mode = FileOpsMode::Normal;
        self.input_buffer.clear();
        self.pending_operation = None;
        self.sequence_buffer.clear();
    }

    /// Get current operation mode
    pub fn current_mode(&self) -> FileOpsMode {
        self.mode
    }

    /// Get current input buffer
    pub fn input_buffer(&self) -> &str {
        &self.input_buffer
    }

    /// Get operation statistics
    pub fn operation_count(&self) -> u64 {
        self.operation_count
    }

    /// Reset operation statistics
    pub fn reset_stats(&mut self) {
        self.operation_count = 0;
    }

    /// Check if currently in input mode
    pub fn is_input_mode(&self) -> bool {
        matches!(
            self.mode,
            FileOpsMode::CreatingFile
                | FileOpsMode::CreatingDirectory
                | FileOpsMode::Renaming
                | FileOpsMode::AwaitingDestination
        )
    }
}

impl EventHandler for FileOpsHandler {
    fn can_handle(&self, event: &Event) -> bool {
        if let Event::Key {
            event: key_event, ..
        } = event
        {
            match self.mode {
                FileOpsMode::Normal => {
                    // Handle file operation keys (conflict-free)
                    matches!(
                        key_event.code,
                        // Core file operations (no navigation conflicts)
                        KeyCode::Char('n' | 'N' | 'm' | 'M' | 'd' | 'D' |
  'r' | 'R' | '.' | 'C' | 'X') |
                          // Function keys
                          KeyCode::F(2 | 7 | 8)
                    ) || (key_event.modifiers.contains(KeyModifiers::CONTROL)
                        && matches!(key_event.code, KeyCode::Char('n' | 'd')))
                }
                _ => {
                    // In input modes, handle all keys for input processing
                    true
                }
            }
        } else {
            false
        }
    }

    fn handle(&mut self, event: Event) -> Result<Vec<Action>, AppError> {
        if let Event::Key {
            event: key_event, ..
        } = event
        {
            self.handle_key(key_event)
        } else {
            Ok(vec![])
        }
    }

    fn priority(&self) -> u8 {
        match self.mode {
            FileOpsMode::Normal => 50, // Medium priority in normal mode
            _ => 3,                    // Very high priority when in input mode
        }
    }

    fn name(&self) -> &'static str {
        "FileOpsHandler"
    }
}

// Helper functions

/// Create simple character key event
fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

/// Create function key event
fn function_key(n: u8) -> KeyEvent {
    KeyEvent::new(KeyCode::F(n), KeyModifiers::NONE)
}

/// Create Ctrl+key event
fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

/// Check if character is valid for filenames
fn is_valid_filename_char(c: char) -> bool {
    // Basic filename validation - exclude problematic characters
    !matches!(
        c,
        '\0' | '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
    )
}

/// Check if character is valid for file paths
fn is_valid_path_char(c: char) -> bool {
    // More permissive for paths (allow path separators)
    !matches!(c, '\0' | '*' | '?' | '"' | '<' | '>' | '|')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_ops_handler_creation() {
        let handler = FileOpsHandler::new();
        assert_eq!(handler.mode, FileOpsMode::Normal);
        assert!(handler.input_buffer.is_empty());
        assert_eq!(handler.operation_count, 0);
    }

    #[test]
    fn test_mode_transitions() {
        let mut handler = FileOpsHandler::new();

        // Test file creation mode
        assert_eq!(handler.mode, FileOpsMode::Normal);
        // Simulate 'n' key press would trigger mode change in real usage

        handler.reset();
        assert_eq!(handler.mode, FileOpsMode::Normal);
    }

    #[test]
    fn test_filename_validation() {
        assert!(is_valid_filename_char('a'));
        assert!(is_valid_filename_char('1'));
        assert!(is_valid_filename_char('.'));
        assert!(is_valid_filename_char('-'));

        assert!(!is_valid_filename_char('/'));
        assert!(!is_valid_filename_char('\\'));
        assert!(!is_valid_filename_char(':'));
        assert!(!is_valid_filename_char('*'));
    }

    #[test]
    fn test_priority_system() {
        let handler = FileOpsHandler::new();
        assert_eq!(handler.priority(), 50); // Normal mode

        let mut input_handler = FileOpsHandler::new();
        input_handler.mode = FileOpsMode::CreatingFile;
        assert_eq!(input_handler.priority(), 3); // Input mode
    }

    #[test]
    fn test_operation_statistics() {
        let mut handler = FileOpsHandler::new();
        assert_eq!(handler.operation_count(), 0);

        handler.operation_count += 1;
        assert_eq!(handler.operation_count(), 1);

        handler.reset_stats();
        assert_eq!(handler.operation_count(), 0);
    }
}
