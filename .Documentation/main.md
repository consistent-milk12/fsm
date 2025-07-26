# Application Entry Point (`src/main.rs`)

This file contains the main entry point for the async TUI file manager application.

## Responsibilities
- **Terminal Initialization**: Sets up the terminal in raw mode and alternate screen using `crossterm` and `ratatui`.
- **Configuration and State**: Loads configuration, initializes cache, and sets up the main application state (`AppState`).
- **Event Loop**: Spawns the main `EventLoop` controller and starts the UI render loop.
- **Graceful Shutdown**: Handles signals (SIGINT, SIGTERM, Ctrl+C) for clean shutdown and terminal restoration.
- **Performance Monitoring**: Logs slow renders and memory usage warnings.
- **Panic Handling**: Installs a panic hook to restore the terminal on panic.

## Key Types
- `App`: Struct encapsulating the terminal, controller, state, and shutdown logic.
- `AppTerminal`: Type alias for the terminal backend.

## Usage
Run the application with `cargo run`. The main loop will process events, render the UI, and handle user actions until quit.

---

## Application Interaction Flow Map

This map details the entire application flow, from startup to shutdown, showing how different modules and components interact.

```text
[main.rs: main()]
  │
  ├─ setup_panic_handler()
  │
  ├─ App::new()
  │  ├─ Logger::init_tracing()
  │  ├─ setup_terminal() -> AppTerminal
  │  ├─ tokio::spawn(Config::load()) -> config
  │  ├─ tokio::spawn(tokio::fs::canonicalize(".")) -> current_dir
  │  ├─ ObjectInfoCache::with_config(config) -> cache
  │  ├─ FSState::default() -> fs_state
  │  ├─ UIState::default() -> ui_state
  │  ├─ mpsc::unbounded_channel() -> task_tx, task_rx
  │  ├─ mpsc::unbounded_channel() -> action_tx, action_rx
  │  ├─ AppState::new(config, cache, fs_state, ui_state, task_tx, action_tx) -> app_state
  │  ├─ EventLoop::new(app_state, task_rx, action_rx) -> controller
  │  └─ app_state.enter_directory(current_dir)
  │     └─ AppState::enter_directory_streaming()
  │        └─ dir_scanner::scan_dir_streaming_with_background_metadata()
  │           ├─ Spawns tokio task for directory scanning
  │           └─ Sends Action::DirectoryScanUpdate via action_tx
  │
  └─ App::run()
     ├─ setup_shutdown_handler() -> Spawns tokio task for signal handling
     │
     └─ Main Loop
        ├─ render()
        │  └─ if app.ui.needs_redraw()
        │     └─ terminal.draw(|frame| View::redraw(frame, &mut state))
        │        └─ [view/ui.rs: View::redraw()]
        │           ├─ ObjectTable::render()
        │           ├─ StatusBar::render()
        │           ├─ (if overlay) Renders corresponding overlay (Help, Search, etc.)
        │           └─ (if notifications) NotificationOverlay::render()
        │
        ├─ check_memory_usage()
        │
        ├─ tokio::select!
        │  ├─ on shutdown signal -> break loop
        │  └─ on controller.next_action() -> action
        │     └─ [controller/event_loop.rs: EventLoop::next_action()]
        │        ├─ Waits for terminal event, task result, or direct action
        │        └─ Returns Action enum
        │
        ├─ if action == Action::Quit -> break loop
        │
        └─ controller.dispatch_action(action)
           └─ [controller/event_loop.rs: EventLoop::dispatch_action(action)]
              ├─ Matches on Action enum
              ├─ Updates AppState (e.g., app.ui.toggle_help_overlay())
              ├─ Spawns background tasks (e.g., for file operations, search)
              │  └─ [tasks/*.rs]
              │     ├─ Perform work (e.g., copy, search)
              │     └─ Send TaskResult back to EventLoop via task_tx
              └─ Requests UI redraw (e.g., app.ui.request_redraw(RedrawFlag::All))
```

---

### Deep Dive: Tracing a File/Folder Copy Operation

This trace follows the sequence of events when a user copies a file or folder, showcasing how UI interaction, state management, and background tasks work together to provide a responsive experience with progress tracking and cancellation.

**1. User Input -> Show Prompt**
- The user presses the `c` key in navigation mode.
- `controller/event_loop.rs: EventLoop::handle_navigation_mode_keys()` matches `c` and returns `Action::ShowInputPrompt(InputPromptType::CopyDestination)`.

**2. Action Dispatch -> UI State Change**
- `EventLoop::dispatch_action()` receives the action.
- It calls `app.ui.show_input_prompt(InputPromptType::CopyDestination)`.
- This sets `app.ui.overlay = UIOverlay::Prompt` and `app.ui.input_prompt_type = Some(InputPromptType::CopyDestination)`.
- A redraw is requested.

**3. Render Loop -> Show Prompt**
- `view/ui.rs: View::redraw()` sees the `Prompt` overlay and calls `view/components/input_prompt_overlay.rs: InputPromptOverlay::render()`.
- A prompt is displayed asking the user for the destination path.

**4. User Input -> Submit Prompt**
- The user types the destination path (e.g., `/tmp/backup/`) and presses `Enter`.
- `EventLoop::handle_prompt_keys()` captures the input and, on `Enter`, returns `Action::SubmitInputPrompt(input)`.

**5. Action Dispatch -> File Operation Task**
- `EventLoop::dispatch_action()` receives `Action::SubmitInputPrompt`.
- It matches on the `input_prompt_type`, which is `CopyDestination`.
- It gets the currently selected file/folder path from `app.fs.active_pane()`.
- It creates a `dest` `PathBuf` from the user's input.
- It dispatches a new `Action::Copy { source, dest }`.
- The `dispatch_action` method matches `Action::Copy`.
- `tasks/file_ops_task.rs: FileOperationTask::new()` is called to create a new background task for the copy operation. This includes:
    - Generating a unique `operation_id`.
    - Creating a `CancellationToken` to allow for cancellation.
- The `CancellationToken` is stored in `app.ui.operations_cancel_tokens` with the `operation_id` as the key.
- `tokio::spawn()` is called to execute the `FileOperationTask::execute()` method in the background.

**6. Background Task Execution (`FileOperationTask::execute`)**
- The task first calls `calculate_operation_size()` to determine the total size and number of files to be copied. This is used for the progress bar.
- It sends an initial `TaskResult::FileOperationProgress` message to the `EventLoop` to show that the operation has started.
- It then calls `copy_file_with_progress()`.
- `copy_file_with_progress()`:
    - Opens the source and destination files.
    - Reads from the source and writes to the destination in chunks (64KB buffer).
    - After each chunk, it checks the `cancel_token`. If cancellation is requested, it aborts the operation.
    - Periodically, it sends `TaskResult::FileOperationProgress` updates with the number of bytes copied so far.
- Once the copy is complete (or has failed/been cancelled), it sends a final `TaskResult::FileOperationComplete` message with the result.

**7. Task Result -> UI Update**
- `EventLoop::next_action()` receives the `TaskResult` messages from the `task_rx` channel.
- `EventLoop::dispatch_action()` is called with `Action::TaskResult(result)`.
- It matches on the `TaskResult` enum:
    - For `FileOperationProgress`, it updates the `app.ui.active_file_operations` map with the latest progress information. This map is what the `FileOperationsOverlay` uses to render the progress bars.
    - For `FileOperationComplete`, it removes the operation from the `active_file_operations` and `operations_cancel_tokens` maps. It then shows a success or error notification to the user.
- A UI redraw is requested after each update.

**8. Render Loop -> Show Progress**
- `view/ui.rs: View::redraw()` checks if `app.ui.active_file_operations` is not empty.
- If it's not, it calls `view/components/file_operations_overlay.rs: FileOperationsOverlay::render()`.
- The overlay iterates through the active operations and renders a `Gauge` widget (a progress bar) for each one, showing the percentage complete, throughput, and ETA.

**9. Cancellation (Optional)**
- If the user presses `Esc` while the copy is in progress:
- `EventLoop::handle_escape_key()` is called.
- It sees that `app.ui.active_file_operations` is not empty.
- It calls `app.ui.cancel_all_operations()`.
- This iterates through the `operations_cancel_tokens` and calls `.cancel()` on each token.
- In the background `FileOperationTask`, the check `self.cancel_token.is_cancelled()` will now return `true`, causing the copy loop to terminate and return a `Cancelled` error.
- The `FileOperationComplete` message will be sent with the `Cancelled` error, and the UI will be updated accordingly (usually by hiding the progress bar and showing a "Cancelled" notification).

This detailed flow highlights the robust, non-blocking, and user-responsive nature of file operations in the application.

---

### Deep Dive: Tracing a Filename Search (`/`)

This flow demonstrates the application's asynchronous, message-passing architecture by tracing a common user action from start to finish.

**1. User Input -> Action**
- The user presses the `/` key.
- `main.rs`: The main loop is waiting in `controller.next_action().await`.
- `crossterm`: Captures the key press as a `TermEvent::Key`.
- `controller/event_loop.rs`: `EventLoop::next_action()` receives the `TermEvent`.
- `EventLoop::handle_terminal_event()` is called. Since no overlay is active, it routes the key to `handle_navigation_mode_keys()`.
- `handle_navigation_mode_keys()` matches `KeyCode::Char('/')` and returns `Action::ToggleFileNameSearch`.

**2. Action Dispatch -> UI State Change**
- `main.rs`: The main loop receives `Action::ToggleFileNameSearch`.
- `EventLoop::dispatch_action()` is called with this action.
- It matches `Action::ToggleFileNameSearch` and acquires a lock on `app_state`.
- It calls `app.ui.toggle_filename_search_overlay()`, which sets `app.ui.overlay = UIOverlay::FileNameSearch` and clears the input buffer.
- `app.ui.request_redraw(RedrawFlag::All)` is called to signal that the UI needs to be updated.

**3. Render Loop -> UI Update**
- `main.rs`: On the next iteration, `app.render()` sees the redraw flag.
- `terminal.draw()` is called, which in turn calls `view/ui.rs: View::redraw()`.
- `View::redraw()` sees that `app.ui.overlay` is `UIOverlay::FileNameSearch` and calls `view/components/filename_search_overlay.rs: FileNameSearchOverlay::render()`.
- The search overlay is drawn on the screen, and the cursor is positioned in the input box.

**4. User Input -> Search Task**
- The user types a search pattern, e.g., "gemini".
- `EventLoop::handle_terminal_event()` receives each key press. It now routes them to `handle_filename_search_keys()` because the `FileNameSearch` overlay is active.
- For each character, `handle_filename_search_keys()` appends it to `app.ui.input` and returns `Action::FileNameSearch(pattern)`.
- `EventLoop::dispatch_action()` receives `Action::FileNameSearch(pattern)`.
- It calls `app_state.filename_search(pattern)`.
- `model/app_state.rs: AppState::filename_search()`:
    - It generates a unique `task_id`.
    - It creates a `TaskInfo` struct to track the search.
    - It calls `tasks/filename_search_task.rs: filename_search_task()`, spawning a new Tokio task.

**5. Background Task Execution**
- `tasks/filename_search_task.rs`: The spawned task begins execution.
- It determines whether to use `fd` or `find` as the underlying search command.
- It spawns the chosen command as a child process with piped stdout/stderr.
- It reads the output from the child process line by line, asynchronously.
- For each line (a file path), it creates an `ObjectInfo` struct.
- Periodically, it sends intermediate results back to the main event loop by sending an `Action::ShowFilenameSearchResults(results)` via the `action_tx` channel.

**6. Task Result -> Data State Change**
- `controller/event_loop.rs`: `EventLoop::next_action()` receives the `Action::ShowFilenameSearchResults` from the `action_tx` channel.
- `EventLoop::dispatch_action()` is called.
- It locks `app_state` and updates `app.ui.filename_search_results` with the new data.
- It requests a redraw.

**7. Render Loop -> Final UI**
- `main.rs`: The `render()` method is called again.
- `FileNameSearchOverlay::render()` is called.
- It now finds results in `app.ui.filename_search_results` and renders them as a list for the user to navigate.

**8. Cleanup**
- When the user presses `Esc`, `handle_escape_key()` returns `Action::CloseOverlay`.
- `dispatch_action()` calls `app.ui.close_all_overlays()`, which sets `app.ui.overlay = UIOverlay::None`.
- The search results in `app.ui.filename_search_results` are kept until the next search is initiated.
