//! # FSM - File System Manager (TUI)
//!
//! A high-performance, async Terminal User Interface (TUI) file manager built with Rust.
//! Uses the ratatui library for rendering and tokio for async operations.
//!
//! ## Architecture Overview
//!
//! This application follows a modern **async MVC (Model-View-Controller)** architecture
//! with **actor-like patterns** using Rust's async/await and channels for communication.
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                        APPLICATION FLOW                         │
//! ├─────────────────────────────────────────────────────────────────┤
//! │                                                                 │
//! │  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐         │
//! │  │    VIEW     │    │ CONTROLLER  │    │    MODEL    │         │
//! │  │  (ratatui)  │◄───┤ (EventLoop) │────►│ (AppState)  │         │
//! │  │             │    │             │    │             │         │
//! │  │ • UI render │    │ • Actions   │    │ • Data      │         │
//! │  │ • Layouts   │    │ • Events    │    │ • State     │         │
//! │  │ • Overlays  │    │ • Handlers  │    │ • Logic     │         │
//! │  └─────────────┘    └─────────────┘    └─────────────┘         │
//! │         │                   │                   │              │
//! │         └───────────────────┼───────────────────┘              │
//! │                             │                                  │
//! │         ┌───────────────────▼───────────────────┐              │
//! │         │           BACKGROUND TASKS            │              │
//! │         │                                       │              │
//! │         │ • Directory scanning                  │              │
//! │         │ • File search (ripgrep)               │              │
//! │         │ • Metadata loading                    │              │
//! │         │ • Size calculations                   │              │
//! │         └───────────────────────────────────────┘              │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Core Concepts
//!
//! ### 1. **Event-Driven Architecture**
//! - Terminal events (keys, mouse, resize) → Actions
//! - Actions dispatched through centralized EventLoop
//! - Specialized handlers process different action categories
//! - Async channels connect components (mpsc for communication)
//!
//! ### 2. **State Management**
//! - **AppState**: Central application state (Arc<Mutex<T>> for thread safety)
//! - **FSState**: File system state (current directory, entries, caches)
//! - **UIState**: UI state (overlays, selections, modes, notifications)
//! - State updates trigger UI redraws via request_redraw() pattern
//!
//! ### 3. **Command-Driven Interface**
//! - Vim-style command mode (`:` prefix)
//! - Commands: `:nf` (new file), `:nd` (new directory), `:search`, `:grep`
//! - Auto-completion with Tab navigation
//! - Input prompts for user interaction
//!
//! ### 4. **Background Task System**
//! - Non-blocking operations via tokio::spawn
//! - Task results communicated via channels
//! - Progress tracking and cancellation support
//! - Examples: directory scanning, search operations, metadata loading
//!
//! ## Execution Flow
//!
//! ```text
//! 1. main.rs
//!    ├── Initialize terminal, config, cache
//!    ├── Create AppState with channels (task_tx, action_tx)
//!    ├── Spawn EventLoop with handlers
//!    └── Start main render/event loop
//!
//! 2. Event Processing
//!    ├── EventLoop.next_action() receives:
//!    │   ├── Terminal events (keys/mouse) → Actions
//!    │   ├── Background task results → TaskResult
//!    │   └── Direct actions via action_rx
//!    └── dispatch_action() routes to specialized handlers
//!
//! 3. Action Handlers (Modular Design)
//!    ├── ui_handler: Overlays, modes, UI state
//!    ├── navigation_handler: Selection, directory traversal
//!    ├── file_ops_handler: File/directory operations
//!    ├── search_handler: Search functionality
//!    ├── input_handler: Input prompts and forms
//!    └── task_handler: Background task management
//!
//! 4. State Updates
//!    ├── Handlers modify AppState
//!    ├── UI redraws triggered via request_redraw()
//!    └── View renders current state
//! ```
//!
//! ## Key Features
//!
//! - **Async Directory Navigation**: Non-blocking directory scanning with incremental loading
//! - **Advanced Search**: Filename search (`/`) and content search (`:search`) with ripgrep integration
//! - **File Operations**: Create, delete, rename with input validation and history tracking
//! - **Smart Caching**: LRU cache (Moka) for metadata with background refresh
//! - **Command System**: Vim-style commands with auto-completion
//! - **Responsive UI**: 60fps target with smart redraw optimization
//! - **Error Handling**: Comprehensive error reporting with user-friendly messages
//!
//! ## Module Organization
//!
//! Each module has a specific responsibility in the architecture:

// =============================================================================
// ERROR HANDLING - Unified error types and result handling
// =============================================================================

/// Application-wide error handling
///
/// Provides unified error types using `thiserror` for consistent error reporting.
/// All operations return `Result<T, AppError>` for proper error propagation.
pub mod error;

// =============================================================================
// CONFIGURATION - App settings, keymaps, themes
// =============================================================================

/// Configuration management
///
/// Handles:
/// - User preferences (editor command, cache settings)
/// - Keybindings and hotkeys
/// - UI themes and styling
/// - Persistent settings via config files
pub mod config;

// =============================================================================
// CACHING - High-performance object metadata cache
// =============================================================================

/// Async LRU cache for file metadata
///
/// Uses Moka for high-performance caching with:
/// - Time-based TTL (time-to-live) expiration
/// - Size-based eviction policies
/// - Background refresh mechanisms
/// - Thread-safe async operations
pub mod cache {
    /// Core cache manager implementation
    pub mod cache_manager;
}

// =============================================================================
// CONTROLLER - Event processing and action dispatch
// =============================================================================

/// **CONTROLLER LAYER** - The heart of the event-driven architecture
///
/// This is where all user interactions and system events are processed.
/// The controller acts as the central dispatcher, routing actions to
/// specialized handlers while maintaining clean separation of concerns.
///
/// ## Event Flow:
/// ```text
/// Terminal Event → Action → Handler → State Update → UI Redraw
/// ```
///
/// ## Handler Specialization:
/// Each handler focuses on a specific domain (UI, navigation, files, etc.)
/// preventing the monolithic controller anti-pattern.
pub mod controller {
    /// Action definitions - All possible user actions and system events
    ///
    /// The `Action` enum represents every possible operation in the application:
    /// - User input (keyboard, mouse)
    /// - UI state changes (overlays, modes)
    /// - File operations (create, delete, rename)
    /// - Navigation (selection, directory changes)
    /// - Search operations (filename, content)
    /// - Background task results
    /// Specialized action handlers (implemented as EventLoop methods)
    ///
    /// Instead of one massive dispatch function, actions are routed to
    /// focused handler methods based on their domain:
    /// - handle_ui_action: UI state management, overlays, modes
    /// - handle_navigation_action: Selection, directory traversal  
    /// - handle_file_operation: File/directory operations, validation
    /// - handle_search_action: Search functionality, result display
    /// - handle_task_action: Background task coordination
    /// - handle_input_action: Input prompts, form validation
    // Re-export core types for easy access
    pub mod actions;
    pub use actions::Action;

    /// Central event loop and action dispatcher
    ///
    /// The `EventLoop` struct:
    /// - Receives events from multiple sources (terminal, tasks, direct actions)
    /// - Routes actions to appropriate specialized handlers
    /// - Manages task communication channels
    /// - Coordinates state updates and UI redraws
    pub mod event_loop;
    pub use event_loop::{EventLoop, TaskResult};

    /// High-performance event processing pipeline
    pub mod event_processor;

    /// Lock-free state coordination
    pub mod state_coordinator;

    /// Action batching and optimization
    pub mod action_batcher;

    /// Event handler registry
    pub mod handler_registry;
    pub mod handlers {
        pub mod keyboard_handler;
    }

    /// Performance monitoring
    pub mod metrics;

    // Performance-optimized modules
    pub mod eactions;
    pub mod ekey_processor;
    pub mod esimd_matcher;
}

// =============================================================================
// MODEL - Application state and data structures
// =============================================================================

/// **MODEL LAYER** - Application state and business logic
///
/// Following MVC patterns, the model layer contains all application state
/// and business logic, separated from UI concerns.
///
/// ## State Architecture:
/// ```text
/// AppState (Arc<Mutex<T>>)
/// ├── FSState - File system state
/// ├── UIState - User interface state  
/// ├── Config  - User preferences
/// ├── Cache   - Metadata cache
/// └── Channels - Communication (task_tx, action_tx)
/// ```
pub mod model {
    /// **Central Application State** - The single source of truth
    ///
    /// `AppState` contains all application data and provides methods for:
    /// - Directory navigation and file operations
    /// - Task management and communication
    /// - History tracking for undo operations
    /// - Error state management
    ///
    /// Thread-safe via Arc<Mutex<T>> for concurrent access from UI and background tasks.
    pub mod app_state;
    pub use app_state::{AppHistoryEvent, AppState, PluginInfo};

    /// **Vim-style Command System** - Command parsing and execution
    ///
    /// Handles the `:command` interface with:
    /// - Command parsing and validation
    /// - Auto-completion suggestions
    /// - Command history
    /// - Argument processing
    pub mod command_palette;
    pub use command_palette::{Command, CommandAction, CommandPaletteState};

    /// **File System State** - Directory and file management
    ///
    /// Manages:
    /// - Current directory and navigation history
    /// - File/directory entries with metadata
    /// - Sorting and filtering options
    /// - Multi-pane support (future expansion)
    pub mod fs_state;
    pub use fs_state::{EntryFilter, EntrySort, FSState, ObjectType, PaneState};

    /// **User Interface State** - UI-specific state management
    ///
    /// Tracks:
    /// - Current overlay and UI mode
    /// - Selection state and navigation
    /// - Search results and input buffers
    /// - Notification and loading states
    /// - Redraw flags and optimization
    pub mod ui_state;
    pub use ui_state::{
        FileOperationProgress, LoadingState, Notification, NotificationLevel, RedrawFlag,
        SearchType, UIMode, UIOverlay, UIState,
    };
}

// =============================================================================
// VIEW - User interface rendering and components
// =============================================================================

/// **VIEW LAYER** - Terminal UI rendering with ratatui
///
/// Responsible for all visual presentation using the ratatui library.
/// Follows component-based architecture for maintainability.
///
/// ## Rendering Pipeline:
/// ```text
/// App State → UI Components → ratatui Widgets → Terminal Output
/// ```
///
/// ## Component Hierarchy:
/// ```text
/// UI (main orchestrator)
/// ├── ObjectTable (file/directory list)
/// ├── StatusBar (current path, stats)
/// ├── Overlays (search, help, prompts)
/// │   ├── HelpOverlay
/// │   ├── SearchOverlay
/// │   ├── InputPromptOverlay
/// │   └── LoadingOverlay
/// └── CommandBar (vim-style command input)
/// ```
pub mod view {
    /// Icon system for file types and UI elements
    pub mod icons;

    /// Color themes and styling configuration
    pub mod theme;

    /// **Generic UI Traits** - Common interfaces for UI components
    ///
    /// Provides consistent interfaces for:
    /// - Overlay behavior and lifecycle management
    /// - Focus handling for interactive components
    /// - Scrollable content patterns
    /// - Redraw optimization hooks
    pub mod traits;

    /// **Main UI orchestrator** - Coordinates all rendering
    ///
    /// The central `render()` function that:
    /// - Determines current UI mode and overlays
    /// - Coordinates component rendering
    /// - Handles layout and responsive design
    /// - Manages redraw optimization
    pub mod ui;
    pub use ui::View;

    /// **Reusable UI Components** - Modular widget system
    ///
    /// Each component handles its own rendering logic and state:
    pub mod components {
        /// Command auto-completion widget with Tab navigation
        pub mod command_completion;
        pub use command_completion::CommandCompletion;

        /// Vim-style command palette interface
        pub mod command_palette;
        pub use command_palette::CommandPalette;

        /// High-performance clipboard overlay with zero-allocation rendering
        pub mod clipboard_overlay;
        pub use clipboard_overlay::ClipboardOverlay;

        /// Content search overlay with ripgrep integration
        pub mod content_search_overlay;
        pub use content_search_overlay::ContentSearchOverlay;

        /// File operations progress overlay with real-time progress tracking
        pub mod file_operations_overlay;
        pub use file_operations_overlay::FileOperationsOverlay;

        /// Filename search overlay with real-time results
        pub mod filename_search_overlay;
        pub use filename_search_overlay::FileNameSearchOverlay;

        /// Help system showing keybindings and commands
        pub mod help_overlay;
        pub use help_overlay::HelpOverlay;

        /// Modal input prompts for user text input
        pub mod input_prompt_overlay;
        pub use input_prompt_overlay::InputPromptOverlay;

        /// Loading indicators with progress bars
        pub mod loading_overlay;
        pub use loading_overlay::LoadingOverlay;

        /// Toast-style notifications for user feedback
        pub mod notification_overlay;
        pub use notification_overlay::NotificationOverlay;

        /// **Main file/directory table** - Core UI component
        ///
        /// Features:
        /// - Virtual scrolling for performance
        /// - Column-based layout (name, size, modified)
        /// - Selection highlighting
        /// - Sorting indicators
        pub mod object_table;
        pub use object_table::ObjectTable;

        /// Bottom prompt bar for status and commands
        pub mod prompt_bar;
        pub use prompt_bar::PromptBar;

        /// Generic search overlay base component
        pub mod search_overlay;
        pub use search_overlay::SearchOverlay;

        /// Search results display with navigation
        pub mod search_results_overlay;
        pub use search_results_overlay::SearchResultsOverlay;

        /// Status bar showing current directory and file counts
        pub mod status_bar;
        pub use status_bar::StatusBar;
    }

    pub use components::*;
}

pub use view::*;

// =============================================================================
// FILESYSTEM - File system abstraction and operations
// =============================================================================

/// **File System Abstraction** - Cross-platform file operations
///
/// Provides async file system operations with comprehensive metadata support.
/// Abstracts platform differences and provides consistent interfaces.
pub mod fs {
    /// **Async Directory Scanner** - Non-blocking directory traversal
    ///
    /// Features:
    /// - Incremental loading for large directories
    /// - Background metadata collection
    /// - Error handling for permission issues
    /// - Progress reporting via channels
    ///
    /// Uses tokio for non-blocking I/O operations.
    pub mod dir_scanner;

    /// **Rich File Metadata** - Comprehensive file information
    ///
    /// `ObjectInfo` struct contains:
    /// - Basic metadata (size, modified time, permissions)
    /// - File type detection and icons
    /// - Symlink resolution
    /// - Platform-specific attributes
    ///
    /// Optimized for caching and UI display.
    pub mod object_info;
}

// =============================================================================
// BACKGROUND TASKS - Async operation system
// =============================================================================

/// **Background Task System** - Non-blocking async operations
///
/// All potentially slow operations run as background tasks to maintain
/// UI responsiveness. Tasks communicate results via channels.
///
/// ## Task Lifecycle:
/// ```text
/// Spawn Task → Process → Report Progress → Complete → Cleanup
/// ```
///
/// ## Communication Pattern:
/// ```text
/// Task ──(TaskResult)──► EventLoop ──(Action)──► Handler ──► AppState
/// ```
pub mod tasks {
    /// **Filename Search Task** - Fast file finding
    ///
    /// Uses `fd` (preferred) or `find` for filename pattern matching:
    /// - Incremental result reporting
    /// - Regex and glob pattern support
    /// - Respects .gitignore and hidden file settings
    /// - Cancellation support
    pub mod filename_search_task;

    /// **Metadata Loading Task** - Background file info collection
    ///
    /// Collects detailed file metadata without blocking UI:
    /// - File size calculation for directories
    /// - Permission and ownership information
    /// - File type detection
    /// - Batched updates for performance
    pub mod metadata_task;

    /// **Content Search Task** - Full-text search with ripgrep
    ///
    /// Integrates with ripgrep for fast content searching:
    /// - ANSI color output preservation
    /// - Context line extraction
    /// - Multiple file format support
    /// - Real-time result streaming
    pub mod search_task;

    /// **Size Calculation Task** - Directory size computation
    ///
    /// Recursively calculates directory sizes:
    /// - Async traversal for large directories
    /// - Progress reporting
    /// - Memory-efficient streaming
    /// - Error resilience for inaccessible files
    pub mod size_task;

    /// **File Operations Task** - Background file operations
    ///
    /// Handles file operations asynchronously to prevent UI blocking:
    /// - Copy, move, rename operations
    /// - Progress tracking for large files
    /// - Cross-filesystem compatibility
    /// - Error handling and recovery
    pub mod file_ops_task;
}

// =============================================================================
// UTILITY MODULES - Supporting functionality
// =============================================================================

/// **Logging System** - Structured logging with tracing
///
/// Provides comprehensive logging for debugging and monitoring:
/// - Multiple log levels (trace, debug, info, warn, error)
/// - Structured logging with context
/// - Performance monitoring
/// - File and console output options
pub mod logging;
pub use logging::Logger;

/// **Plugin System** - Extensibility framework (future)
///
/// Designed for future extensibility:
/// - Custom file operations
/// - UI theme plugins
/// - Search provider plugins
/// - Integration with external tools
pub mod plugins;

/// **Utility Functions** - Helper functions and common operations
///
/// Contains shared utilities:
/// - Path manipulation helpers
/// - String formatting functions
/// - Performance measurement tools
/// - Cross-platform compatibility functions
pub mod util;

// =============================================================================
// PUBLIC API - Main library exports
// =============================================================================

/// **Primary Error Type** - Main error enum for the application
///
/// All public APIs return `Result<T, AppError>` for consistent error handling.
pub use error::AppError;

/// **Core State Types** - Main application state structures
///
/// The three pillars of application state:
/// - `AppState`: Central coordinating state
/// - `FSState`: File system specific state
/// - `UIState`: User interface specific state
pub use model::{app_state::AppState, fs_state::FSState, ui_state::UIState};

// =============================================================================
// DEVELOPMENT AND DEBUGGING
// =============================================================================

//  **Performance Characteristics**
//
//  The application is designed for high performance:
//  - Target: 60fps UI rendering
//  - Memory: LRU caching prevents unbounded memory growth
//  - I/O: All file operations are async and non-blocking
//  - CPU: Background tasks prevent UI thread blocking
//
//  **Threading Model**
//
//  - Main thread: UI rendering and event processing
//  - Tokio runtime: Background tasks and async I/O
//  - Shared state: Arc<Mutex<T>> for thread-safe access
//
//  **Memory Management**
//
//  - Rust's ownership system prevents memory leaks
//  - LRU cache bounds metadata memory usage
//  - Streaming operations for large directory traversals
//  - Careful clone() usage to minimize allocations

//  **Testing Strategy**
//
//  ```bash
//  # Run all tests
//  cargo test
//
//  # Run with logging
//  RUST_LOG=debug cargo test
//
//  # Check formatting and linting
//  cargo fmt && cargo clippy
//
//  # Build optimized release
//  cargo build --release
//  ```
//  After ANY source edit:
//  1. `cargo fmt` - Format code
//  2. `cargo check` - Check compilation
//  3. `cargo clippy` - Lint code
//  4. `cargo build` - Build (for significant changes)
