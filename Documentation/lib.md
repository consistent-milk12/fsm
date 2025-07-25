# FSM - File System Manager (TUI)

A high-performance, async Terminal User Interface (TUI) file manager built with Rust.
Uses the ratatui library for rendering and tokio for async operations.

## Architecture Overview

This application follows a modern **async MVC (Model-View-Controller)** architecture
with **actor-like patterns** using Rust's async/await and channels for communication.

```text
┌─────────────────────────────────────────────────────────────────┐
│                        APPLICATION FLOW                         │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐         │
│  │    VIEW     │    │ CONTROLLER  │    │    MODEL    │         │
│  │  (ratatui)  │◄───┤ (EventLoop) │────►│ (AppState)  │         │
│  │             │    │             │    │             │         │
│  │ • UI render │    │ • Actions   │    │ • Data      │         │
│  │ • Layouts   │    │ • Events    │    │ • State     │         │
│  │ • Overlays  │    │ • Handlers  │    │ • Logic     │         │
│  └─────────────┘    └─────────────┘    └─────────────┘         │
│         │                   │                   │              │
│         └───────────────────┼───────────────────┘              │
│                             │                                  │
│         ┌───────────────────▼───────────────────┐              │
│         │           BACKGROUND TASKS            │              │
│         │                                       │              │
│         │ • Directory scanning                  │              │
│         │ • File search (ripgrep)               │              │
│         │ • Metadata loading                    │              │
│         │ • Size calculations                   │              │
│         └───────────────────────────────────────┘              │
└─────────────────────────────────────────────────────────────────┘
```

## Core Concepts

### 1. **Event-Driven Architecture**
- Terminal events (keys, mouse, resize) → Actions
- Actions dispatched through centralized EventLoop
- Specialized handlers process different action categories
- Async channels connect components (mpsc for communication)

### 2. **State Management**
- **AppState**: Central application state (Arc<Mutex<T>> for thread safety)
- **FSState**: File system state (current directory, entries, caches)
- **UIState**: UI state (overlays, selections, modes, notifications)
- State updates trigger UI redraws via request_redraw() pattern

### 3. **Command-Driven Interface**
- Vim-style command mode (`:` prefix)
- Commands: `:nf` (new file), `:nd` (new directory), `:search`, `:grep`
- Auto-completion with Tab navigation
- Input prompts for user interaction

### 4. **Background Task System**
- Non-blocking operations via tokio::spawn
- Task results communicated via channels
- Progress tracking and cancellation support
- Examples: directory scanning, search operations, metadata loading

## Execution Flow

```text
1. main.rs
   ├── Initialize terminal, config, cache
   ├── Create AppState with channels (task_tx, action_tx)
   ├── Spawn EventLoop with handlers
   └── Start main render/event loop

2. Event Processing
   ├── EventLoop.next_action() receives:
   │   ├── Terminal events (keys/mouse) → Actions
   │   ├── Background task results → TaskResult
   │   └── Direct actions via action_rx
   └── dispatch_action() routes to specialized handlers

3. Action Handlers (Modular Design)
   ├── ui_handler: Overlays, modes, UI state
   ├── navigation_handler: Selection, directory traversal
   ├── file_ops_handler: File/directory operations
   ├── search_handler: Search functionality
   ├── input_handler: Input prompts and forms
   └── task_handler: Background task management

4. State Updates
   ├── Handlers modify AppState
   ├── UI redraws triggered via request_redraw()
   └── View renders current state
```

## Key Features

- **Async Directory Navigation**: Non-blocking directory scanning with incremental loading
- **Advanced Search**: Filename search (`/`) and content search (`:search`) with ripgrep integration
- **File Operations**: Create, delete, rename with input validation and history tracking
- **Smart Caching**: LRU cache (Moka) for metadata with background refresh
- **Command System**: Vim-style commands with auto-completion
- **Responsive UI**: 60fps target with smart redraw optimization
- **Error Handling**: Comprehensive error reporting with user-friendly messages

## Module Organization

Each module has a specific responsibility in the architecture:

### ERROR HANDLING - Unified error types and result handling

Application-wide error handling

Provides unified error types using `thiserror` for consistent error reporting.
All operations return `Result<T, AppError>` for proper error propagation.

### CONFIGURATION - App settings, keymaps, themes

Configuration management

Handles:
- User preferences (editor command, cache settings)
- Keybindings and hotkeys
- UI themes and styling
- Persistent settings via config files

### CACHING - High-performance object metadata cache

Async LRU cache for file metadata

Uses Moka for high-performance caching with:
- Time-based TTL (time-to-live) expiration
- Size-based eviction policies
- Background refresh mechanisms
- Thread-safe async operations

### CONTROLLER - Event processing and action dispatch

**CONTROLLER LAYER** - The heart of the event-driven architecture

This is where all user interactions and system events are processed.
The controller acts as the central dispatcher, routing actions to
specialized handlers while maintaining clean separation of concerns.

## Event Flow:
```text
Terminal Event → Action → Handler → State Update → UI Redraw
```

## Handler Specialization:
Each handler focuses on a specific domain (UI, navigation, files, etc.)
preventing the monolithic controller anti-pattern.

### MODEL - Application state and data structures

**MODEL LAYER** - Application state and business logic

Following MVC patterns, the model layer contains all application state
and business logic, separated from UI concerns.

## State Architecture:
```text
AppState (Arc<Mutex<T>>)
├── FSState - File system state
├── UIState - User interface state  
├── Config  - User preferences
├── Cache   - Metadata cache
└── Channels - Communication (task_tx, action_tx)
```

### VIEW - User interface rendering and components

**VIEW LAYER** - Terminal UI rendering with ratatui

Responsible for all visual presentation using the ratatui library.
Follows component-based architecture for maintainability.

## Rendering Pipeline:
```text
App State → UI Components → ratatui Widgets → Terminal Output
```

## Component Hierarchy:
```text
UI (main orchestrator)
├── ObjectTable (file/directory list)
├── StatusBar (current path, stats)
├── Overlays (search, help, prompts)
│   ├── HelpOverlay
│   ├── SearchOverlay
│   ├── InputPromptOverlay
│   └── LoadingOverlay
└── CommandBar (vim-style command input)
```

### FILESYSTEM - File system abstraction and operations

**File System Abstraction** - Cross-platform file operations

Provides async file system operations with comprehensive metadata support.
Abstracts platform differences and provides consistent interfaces.

### BACKGROUND TASKS - Async operation system

**Background Task System** - Non-blocking async operations

All potentially slow operations run as background tasks to maintain
UI responsiveness. Tasks communicate results via channels.

## Task Lifecycle:
```text
Spawn Task → Process → Report Progress → Complete → Cleanup
```

## Communication Pattern:
```text
Task ──(TaskResult)──► EventLoop ──(Action)──► Handler ──► AppState
```

### UTILITY MODULES - Supporting functionality

**Logging System** - Structured logging with tracing

Provides comprehensive logging for debugging and monitoring:
- Multiple log levels (trace, debug, info, warn, error)
- Structured logging with context
- Performance monitoring
- File and console output options

**Plugin System** - Extensibility framework (future)

Designed for future extensibility:
- Custom file operations
- UI theme plugins
- Search provider plugins
- Integration with external tools

**Utility Functions** - Helper functions and common operations

Contains shared utilities:
- Path manipulation helpers
- String formatting functions
- Performance measurement tools
- Cross-platform compatibility functions

### PUBLIC API - Main library exports

**Primary Error Type** - Main error enum for the application

All public APIs return `Result<T, AppError>` for consistent error handling.

**Core State Types** - Main application state structures

The three pillars of application state:
- `AppState`: Central coordinating state
- `FSState`: File system specific state
- `UIState`: User interface specific state

### DEVELOPMENT AND DEBUGGING

**Performance Characteristics**

The application is designed for high performance:
- Target: 60fps UI rendering
- Memory: LRU caching prevents unbounded memory growth
- I/O: All file operations are async and non-blocking
- CPU: Background tasks prevent UI thread blocking

**Threading Model**

- Main thread: UI rendering and event processing
- Tokio runtime: Background tasks and async I/O
- Shared state: Arc<Mutex<T>> for thread-safe access

**Memory Management**

- Rust's ownership system prevents memory leaks
- LRU cache bounds metadata memory usage
- Streaming operations for large directory traversals
- Careful clone() usage to minimize allocations

**Testing Strategy**

```bash
# Run all tests
cargo test

# Run with logging
RUST_LOG=debug cargo test

# Check formatting and linting
cargo fmt && cargo clippy

# Build optimized release
cargo build --release
```
After ANY source edit:
1. `cargo fmt` - Format code
2. `cargo check` - Check compilation
3. `cargo clippy` - Lint code
4. `cargo build` - Build (for significant changes)
