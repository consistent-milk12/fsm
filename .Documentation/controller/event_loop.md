# Event Loop Controller (`src/controller/event_loop.rs`)

This module implements the core event loop and action dispatcher for the file manager, handling all user input, background task results, and system events.

## Features
- **Async/Await Architecture**: Uses async/await and channels for non-blocking event processing.
- **Performance Monitoring**: Tracks event processing times and logs slow events.
- **Command Palette Integration**: Full support for command mode with auto-completion and history.
- **Input Prompt System**: Handles all input prompt types for file operations, search, and navigation.
- **Advanced Search**: Integrates filename and content search with real-time result streaming.
- **Robust Error Handling**: Gracefully handles errors, notifications, and cancellation.
- **Extensive Logging**: Logs all actions, events, and performance metrics for debugging.

## Main Types
- `EventLoop`: The main event loop struct, holding references to app state and communication channels.
- `TaskResult`: Enum representing results from background tasks (file ops, search, etc.).

## Responsibilities
- Polls terminal events, background task results, and direct actions.
- Maps raw events to high-level `Action` enums.
- Dispatches actions to specialized handlers (navigation, search, prompt, etc.).
- Updates application state and triggers UI redraws as needed.

---

## Detailed Logic Flow Map

This map provides a comprehensive breakdown of the event and action processing logic within the `EventLoop`.

```text
EventLoop::next_action()
  ├─ Waits for: terminal event, task result, or direct action (async select)
  │
  ├─ If terminal event -> handle_terminal_event(event)
  │  ├─ If Key event:
  │  │  ├─ If ESC -> handle_escape_key()
  │  │  │  ├─ If active file operations -> Cancel all, show notification, return NoOp
  │  │  │  ├─ If notification visible -> Dismiss notification, return NoOp
  │  │  │  ├─ If overlay visible -> return Action::CloseOverlay
  │  │  │  ├─ If command mode & completions visible -> Hide completions, return NoOp
  │  │  │  ├─ If command mode -> return Action::ExitCommandMode
  │  │  │  └─ Else -> return Action::Quit
  │  │  │
  │  │  ├─ If notification visible -> Dismiss notification, continue processing
  │  │  │
  │  │  ├─ Route by UIMode/UIOverlay:
  │  │  │  ├─ UIMode::Command -> handle_command_mode_keys()
  │  │  │  │  ├─ Char -> Append to input, update completions
  │  │  │  │  ├─ Backspace -> Pop from input, update completions
  │  │  │  │  ├─ Up/Down -> Navigate history or completions
  │  │  │  │  ├─ Tab -> Apply completion
  │  │  │  │  └─ Enter -> Execute command -> map_command_action_to_action()
  │  │  │  │
  │  │  │  ├─ UIOverlay::None (Navigation) -> handle_navigation_mode_keys()
  │  │  │  │  ├─ ':' -> Action::EnterCommandMode
  │  │  │  │  ├─ '/' -> Action::ToggleFileNameSearch
  │  │  │  │  ├─ 'c', 'm', 'r' -> Show input prompts for Copy, Move, Rename
  │  │  │  │  ├─ Arrows/PageUp/Down/Home/End -> Navigation actions
  │  │  │  │  ├─ Enter -> Action::EnterSelected
  │  │  │  │  └─ Backspace -> Action::GoToParent
  │  │  │  │
  │  │  │  ├─ UIOverlay::FileNameSearch -> handle_filename_search_keys()
  │  │  │  │  ├─ Char/Backspace -> Update search pattern -> Action::FileNameSearch
  │  │  │  │  ├─ Up/Down -> Navigate results
  │  │  │  │  └─ Enter -> Open selected file or trigger search
  │  │  │  │
  │  │  │  ├─ UIOverlay::ContentSearch -> handle_content_search_keys()
  │  │  │  │  ├─ Char/Backspace -> Update input, clear results
  │  │  │  │  ├─ Up/Down -> Navigate results
  │  │  │  │  └─ Enter -> Open selected result or trigger Action::ContentSearch
  │  │  │  │
  │  │  │  ├─ UIOverlay::Prompt -> handle_prompt_keys()
  │  │  │  │  ├─ Char/Backspace -> Update input
  │  │  │  │  └─ Enter -> Action::SubmitInputPrompt(input)
  │  │  │  │
  │  │  │  └─ UIOverlay::SearchResults -> handle_search_results_keys()
  │  │  │     ├─ Up/Down -> Navigate results
  │  │  │     └─ Enter -> Open selected file
  │  │  │
  │  ├─ If Mouse event -> Action::Mouse
  │  ├─ If Resize event -> Action::Resize
  │  └─ Other -> Action::Tick
  │
  ├─ If task result -> Action::TaskResult(result)
  │
  ├─ If direct action -> Action (as received)
  │
  └─ update_performance_metrics()

EventLoop::dispatch_action(action)
  ├─ Match on Action enum:
  │    ├─ Quit: Handled in main loop
  │    ├─ ToggleHelp, ToggleFileNameSearch, ToggleContentSearch: Show/hide overlays
  │    ├─ EnterCommandMode, ExitCommandMode: Change app.ui.mode
  │    ├─ Navigation (MoveSelection*, Page*, Select*, EnterSelected, GoToParent): Update app.fs.active_pane, redraw
  │    ├─ Search (FileNameSearch, ContentSearch, DirectContentSearch): Start search tasks, update state
  │    ├─ Show*SearchResults (Simple, Rich, Raw, Filename): Update app.ui with results, redraw
  │    ├─ OpenFile(path, line): Spawn editor process (e.g., 'code --goto')
  │    ├─ CloseOverlay: Close any active overlay
  │    ├─ File Ops (CreateFile, CreateDirectory, Create*WithName): Call app state methods to perform fs operations
  │    ├─ File Ops (Copy, Move, Rename): Spawn FileOperationTask, store cancellation token
  │    ├─ ShowInputPrompt(type): Show prompt overlay with specific type
  │    ├─ SubmitInputPrompt(input):
  │    │  └─ Match on prompt type:
  │    │     ├─ CreateFile -> dispatch Action::CreateFileWithName(input)
  │    │     ├─ Rename -> dispatch Action::RenameEntry(input)
  │    │     ├─ CopyDestination -> dispatch Action::Copy{source, dest}
  │    │     └─ ... (and others)
  │    │
  │    ├─ TaskResult(result):
  │    │  ├─ Legacy: Update loading state
  │    │  ├─ FileOperationComplete: Remove operation from UI, show notification
  │    │  └─ FileOperationProgress: Update progress in app.ui.active_file_operations
  │    │
  │    ├─ DirectoryScanUpdate:
  │    │  ├─ Entry -> Add entry to pane incrementally
  │    │  ├─ Completed -> Finalize loading, spawn size calculation tasks
  │    │  └─ Error -> Show error message
  │    │
  │    ├─ UpdateObjectInfo: Update a single file's info in the current pane
  │    ├─ Tick: Update notifications, perform periodic cleanup
  │    └─ ... (other actions like Sort, Filter, etc.)
  │
  └─ Log execution time if slow
```

This map summarizes the main event and action flow, showing how the event loop receives, maps, and dispatches events and actions throughout the application.
