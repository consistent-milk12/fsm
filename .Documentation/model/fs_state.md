# FSState Module Documentation

## Overview
The `FSState` module manages the advanced filesystem state for the application, supporting power-user workflows such as multi-pane navigation, directory entry management, sorting/filtering, batch operations, and session history. It is designed for high responsiveness and extensibility in a TUI file manager.

## Features
- Multi-pane support: Each pane tracks its own working directory, entries, selection, and UI state.
- Entry sorting and filtering: Flexible enums for sorting (by name, size, modified time, etc.) and filtering (files, dirs, patterns, extensions).
- Incremental loading: Supports streaming directory scans for large directories.
- Virtual scrolling: Efficiently handles large lists with viewport and scroll offset management.
- Batch operations: Tracks progress and status for bulk file actions.
- Favorites and recents: Maintains lists of favorite and recently visited directories.
- Extensible: Custom sort/filter modes and object types for plugins or user scripts.

## Main Types
- `FSState`: Persistent, session-wide filesystem state (multiple panes, batch status, recents, favorites).
- `PaneState`: State for a single pane, including directory contents, selection, scroll, and loading state.
- `EntrySort`, `EntryFilter`: Enums for sorting and filtering directory entries.
- `ObjectType`: Enum for file types (Dir, File, Symlink, Other).

## Key Methods
- `FSState::new(cwd)`: Initialize with a starting directory.
- `FSState::active_pane() / active_pane_mut()`: Access the currently focused pane.
- `FSState::set_active_pane(idx)`: Switch focus between panes.
- `FSState::add_recent_dir(path)`, `add_favorite(path)`, `remove_favorite(path)`: Manage recents and favorites.
- `PaneState::new(cwd)`: Create a new pane for a directory.
- `PaneState::set_entries(entries)`: Set directory contents and reset selection.
- `PaneState::move_selection_up/down`, `page_up/down`, `select_first/last`: Navigation and scrolling.
- `PaneState::start_incremental_loading`, `add_incremental_entry`, `complete_incremental_loading`: Streaming directory loading.
- `PaneState::sort_entries`: Sort entries in place by the current sort mode.

## Usage
- Use `FSState` to manage the overall filesystem state, including multiple panes and batch operations.
- Use `PaneState` for per-pane navigation, selection, and incremental loading.
- Integrate sorting/filtering and object type logic for advanced file management and plugin support.

## Example
```rust
let mut fs_state = FSState::new(PathBuf::from("~/projects"));
let pane = fs_state.active_pane_mut();
pane.set_entries(scan_directory("~/projects"));
pane.sort = EntrySort::SizeDesc;
pane.sort_entries();
```
