# Search Task Module Documentation

## Overview
The `search_task` module provides an asynchronous background task for content searching using `ripgrep` (rg). It streams raw output line-by-line to the UI, enabling fast, non-blocking content search and direct display of results in the TUI.

## Features
- Uses `ripgrep` for fast, recursive content search
- Streams both raw and parsed results to the UI
- Handles ANSI color codes for TUI display
- Parses file and line information from output
- Robust error handling for process spawning and completion

## Main Types
- `search_task`: Main async function for running a ripgrep search
- `RawSearchResult`: Struct for storing search results (lines, parsed lines, match count, base directory)
- `TaskResultExt`: Helper trait for reporting task results

## Key Methods
- `search_task(task_id, pattern, path, task_tx, action_tx)`: Spawns a background search and streams results
- `RawSearchResult::strip_ansi_codes(input)`: Utility to remove ANSI codes from output
- `RawSearchResult::parse_file_info(line)`: Parses file and line info from a ripgrep output line
- `RawSearchResult::parse_file_info_with_base(line, base_dir)`: Resolves relative paths
- `RawSearchResult::parse_heading_line_with_context(line, current_file, base_dir)`: Stateful parsing for heading output

## Usage
- Use `search_task` to perform a content search in the background and stream results to the UI
- Use parsing utilities to extract file and line information from search results

## Example
```rust
search_task(task_id, "pattern", path, task_tx, action_tx);
```
