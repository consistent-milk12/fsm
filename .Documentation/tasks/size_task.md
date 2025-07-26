# Size Task Module Documentation

## Overview
The `size_task` module provides a background task for calculating the recursive size and direct item count of directories. This allows the UI to display up-to-date size and item information without blocking the main thread.

## Features
- Asynchronous calculation of total directory size (recursively)
- Counts direct children (files and directories) in the immediate directory
- Sends results to the UI via action channels for live updates
- Error handling and logging for failed calculations

## Main Types
- `calculate_size_task`: Main async function for directory size and item count calculation

## Key Methods
- `calculate_size_task(parent_dir, object_info, action_tx)`: Spawns a background task to compute size and item count, then sends an update action

## Usage
- Use `calculate_size_task` to update size and item count for a directory in the background
- Results are sent to the UI for updating directory information

## Example
```rust
calculate_size_task(parent_dir, object_info, action_tx);
```
