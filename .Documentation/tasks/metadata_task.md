# Metadata Task Module Documentation

## Overview
The `metadata_task` module provides background tasks for loading file and directory metadata (such as size, modification time, and item count) asynchronously. This improves UI responsiveness by offloading expensive metadata operations from the main thread.

## Features
- Asynchronous loading of full metadata for individual or multiple entries
- Batch loading with configurable batch size to avoid blocking
- Reports results to the UI via action channels
- Error handling and logging for failed metadata loads

## Main Types
- `load_metadata_task`: Spawns a background task to load metadata for a single entry
- `batch_load_metadata_task`: Loads metadata for multiple entries in batches

## Key Methods
- `load_metadata_task(parent_dir, light_info, action_tx)`: Loads full metadata for a single entry and sends an update action
- `batch_load_metadata_task(parent_dir, light_entries, action_tx, batch_size)`: Loads metadata for multiple entries, yielding control periodically

## Usage
- Use `load_metadata_task` to update metadata for a single file or directory
- Use `batch_load_metadata_task` for efficient batch updates of directory listings
- Results are sent to the UI for updating file/directory information

## Example
```rust
load_metadata_task(parent_dir, light_info, action_tx);
batch_load_metadata_task(parent_dir, entries, action_tx, 16);
```
