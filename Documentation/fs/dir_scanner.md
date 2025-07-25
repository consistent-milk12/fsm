# Directory Scanner (`src/fs/dir_scanner.rs`)

This module provides asynchronous directory scanning utilities for the file manager.

## Features
- **Async Directory Listing**: Scans directories using async I/O for non-blocking UI updates.
- **Streaming Updates**: Supports streaming scan results and progress updates to the UI.
- **Batch Metadata Loading**: Integrates with background metadata tasks for fast initial display and later enrichment.
- **Error Handling**: Reports errors for inaccessible entries but continues scanning.
- **Sorting**: Ensures directories are listed before files, sorted alphabetically.

## Main Types
- `scan_dir`: Async function to scan a directory and return a sorted list of `ObjectInfo`.
- `scan_dir_streaming_with_background_metadata`: Streams entries and triggers background metadata loading.
- `ScanUpdate`: Enum representing streaming scan progress (entry, completed, error).

## Usage
Use these functions to populate directory listings in the UI, with support for incremental loading and responsive updates.
