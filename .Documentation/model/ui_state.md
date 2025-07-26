# UIState Module Documentation

## Overview
The `UIState` module manages all ephemeral and persistent UI state for the TUI file manager. It tracks selection, overlays, modes, panes, themes, quick actions, command palette, notifications, search results, and file operation progress. The design supports robust overlay/mode patterns, extensibility for plugins, and immediate-mode TUI responsiveness.

## Features
- Overlay and mode management (browse, search, command, scripting, batch, etc.)
- Selection, multi-selection, and visual range for batch operations
- Notification and status management with auto-dismiss and severity levels
- Search and batch operation tracking, including raw and rich search results
- Theme and pane management for multi-pane UIs
- Quick actions and command palette integration
- File operation progress and cancellation tracking

## Main Types
- `UIState`: Main struct for all UI state
- `RedrawFlag`: Enum for granular UI redraw control
- `UIMode`, `UIOverlay`: Enums for UI modes and overlays
- `Notification`, `LoadingState`, `FileOperationProgress`: Feedback and progress types
- `CommandPaletteState`: State for the command palette overlay

## Key Methods
- `UIState::new()`: Construct a new UI state with default values
- Selection: `set_selected`, `mark_index`, `unmark_index`, `clear_marks`, `set_visual_range`, `move_selection_up/down`
- Overlay/mode: `set_mode`, `set_overlay`, `toggle_help_overlay`, `enter_command_mode`, `exit_command_mode`, `toggle_filename_search_overlay`, `toggle_content_search_overlay`, `close_all_overlays`
- Notification: `show_notification`, `show_info`, `show_warning`, `show_error`, `show_success`, `dismiss_notification`, `update_notification`
- Redraw: `request_redraw`, `request_redraw_all`, `needs_redraw`, `clear_redraw`, `clear_redraw_for`
- Search: `clear_search_results`, `show_input_prompt`, `hide_input_prompt`, `set_input`, `set_last_query`
- Theme/pane: `toggle_show_hidden`, `set_theme`, `set_active_pane`
- Quick actions: `push_action`, `store_cancel_token`, `cancel_all_operations`, `remove_operation`

## Usage
- Use `UIState` to manage all aspects of the TUI's state, including overlays, notifications, and search results.
- Integrate with the main event loop and async tasks for responsive UI updates.
- Use granular redraw flags to optimize rendering performance.

## Example
```rust
let mut ui_state = UIState::new();
ui_state.set_mode(UIMode::Browse);
ui_state.show_info("Welcome to the file manager!");
ui_state.toggle_help_overlay();
```
