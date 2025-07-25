# File Operations Overlay Component Documentation

## Overview
The `file_operations_overlay` component displays the progress and status of ongoing file operations (such as copy, move, or delete) in an overlay. It provides real-time feedback and allows users to monitor or cancel operations.

## Features
- Shows progress bars and status for each active file operation
- Displays operation type, current file, throughput, and estimated completion
- Supports multiple concurrent operations
- Allows cancellation of operations

## Usage
- Render the file operations overlay when there are active file operations
- Update progress and status as operations proceed
- Provide controls for cancelling operations if supported

## Example
```rust
file_operations_overlay::render(frame, area, &active_file_operations);
```
