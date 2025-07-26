# File Operations Task Module Documentation

## Overview
The `file_ops_task` module is responsible for handling background file operations such as copy, move, rename, and delete. These operations are performed asynchronously to keep the TUI responsive and to provide real-time progress updates to the user.

## Features
- Asynchronous execution of file operations (copy, move, rename, delete)
- Progress tracking and reporting for long-running operations
- Error handling and user notifications for failed operations
- Integration with the UI for operation status and cancellation
- Supports batch operations on multiple files or directories

## Main Types
- File operation task functions (e.g., copy, move, delete)
- Progress and status reporting structures

## Usage
- Use the file operations task functions to perform background file actions
- Monitor progress and status via the UI overlays and notifications
- Handle errors and cancellations gracefully to ensure a robust user experience

## Example
```rust
// Example usage for copying files in the background:
file_ops_task::copy_files(src_paths, dest_dir, progress_tx, action_tx);
```
