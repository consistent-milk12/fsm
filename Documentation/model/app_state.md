# AppState Module Documentation

## Overview
The `AppState` struct is the central state container for the application, unifying persistent and transient state for the async TUI file manager. It is designed for power users, supporting advanced workflows such as concurrent tasks, undo/redo, multi-selection, plugins, and responsive UI overlays.

## Features
- Manages configuration, cache, UI, filesystem, plugins, and tasks
- Supports batch operations, history, and undo/redo
- Integrates with async tasks and background operations
- Extensible for plugins and scripting
- Robust error, status, and notification management

## Main Types
- `AppState`: Core application state
- `TaskInfo`: Metadata for background/async tasks
- `AppHistoryEvent`: Undo/redo event types
- `PluginInfo`: Registered plugin metadata

## Usage
- Use `AppState::new` to construct a new state
- Use methods like `mark_entry`, `add_task`, `set_error`, `enter_directory` to manage state and trigger UI updates
- Integrates with async tasks and UI overlays for a responsive experience

---

# CommandPaletteState Module Documentation

## Overview
The `CommandPaletteState` struct manages the state of the command palette overlay, including input, filtering, completions, and command execution.

## Features
- Filters and auto-completes commands based on user input
- Supports built-in and custom commands
- Provides command descriptions and help
- Integrates with the UI for command mode

## Main Types
- `CommandPaletteState`: State for the command palette
- `Command`: User-invokable command
- `CommandAction`: Enum of possible command actions

## Usage
- Use `CommandPaletteState::new` to initialize with available commands
- Call `update_filter`, `update_completions`, and `parse_command` to manage input and completions
- Integrates with UI overlays and command mode

---

# FSState Module Documentation

## Overview
The `FSState` struct manages the state of the filesystem, including multi-pane navigation, directory entries, filters, sorts, and batch operation status.

## Features
- Multi-pane support for advanced navigation
- Entry sorting and filtering
- Batch operation tracking
- Recent and favorite directories

## Main Types
- `FSState`: Persistent filesystem state
- `PaneState`: State for a single pane
- `EntrySort`, `EntryFilter`: Sorting and filtering enums
- `ObjectType`: Enum for file types

## Usage
- Use `FSState::new` to initialize with a working directory
- Use methods to manage panes, recents, favorites, and sorting/filtering

---

# UIState Module Documentation

## Overview
The `UIState` struct tracks all UI-related state, including selection, overlays, modes, themes, search results, notifications, and file operation progress.

## Features
- Overlay and mode management (browse, search, command, scripting, etc.)
- Selection, multi-selection, and visual range
- Notification and status management
- Search and batch operation tracking
- Theme and pane management

## Main Types
- `UIState`: Main UI state
- `RedrawFlag`, `UIMode`, `UIOverlay`: Enums for UI state
- `Notification`, `LoadingState`, `FileOperationProgress`: Feedback and progress types

## Usage
- Use `UIState::new` to construct a new UI state
- Use methods to manage selection, overlays, notifications, and redraws
- Integrates with the main event loop and async tasks
