<!-- src/controller/actions.rs -->

# Actions: Centralized Application Commands

This file defines the `Action` enum, which is the central pillar of the application's event-driven architecture. It represents every possible user input, internal event, and background task result that the application can respond to. This provides a single, clear, and strongly-typed interface for the main `EventLoop` to process.

## `InputPromptType` (Enum)

Represents the different types of input prompts that can be shown to the user. This allows the UI to display the correct context and the controller to handle the submitted input appropriately.

- `CreateFile`: Prompt for creating a new file.
- `CreateDirectory`: Prompt for creating a new directory.
- `Rename`: Prompt for renaming the selected entry.
- `Search`: Prompt for a search query.
- `GoToPath`: Prompt for a path to navigate to.
- `Custom(String)`: A generic prompt with a custom message.
- `CopyDestination`: Prompt for the destination path for a copy operation.
- `MoveDestination`: Prompt for the destination path for a move operation.
- `RenameFile`: Prompt for the new name when renaming a file.

## `Action` (Enum)

This enum represents a high-level, abstract command that the application can perform. It decouples the raw terminal events (like key presses) from the actual operations, making the controller logic cleaner and more maintainable.

### Core & System Actions
- `Key(KeyEvent)`: A raw keyboard event.
- `Mouse(MouseEvent)`: A raw mouse event.
- `Resize(u16, u16)`: A terminal resize event.
- `Tick`: An internal event for periodic updates (e.g., animations, notifications).
- `Quit`: Signals the application to quit.
- `NoOp`: A no-operation action, used when an event is consumed but no state change is needed.

### UI & Mode Actions
- `ToggleHelp`: Toggles the visibility of the help overlay.
- `EnterCommandMode`: Enters the Vim-style command mode.
- `ExitCommandMode`: Exits the command mode.
- `ToggleShowHidden`: Toggles the visibility of hidden files.
- `CloseOverlay`: Closes the currently active overlay.
- `SimulateLoading`: A debug action to simulate a loading state.

### Navigation Actions
- `MoveSelectionUp`: Moves the selection up by one entry.
- `MoveSelectionDown`: Moves the selection down by one entry.
- `PageUp`: Moves the selection up by one viewport height.
- `PageDown`: Moves the selection down by one viewport height.
- `SelectFirst`: Jumps to the first entry in the list.
- `SelectLast`: Jumps to the last entry in the list.
- `EnterSelected`: Enters the selected directory or opens the selected file.
- `GoToParent`: Navigates to the parent directory.
- `GoToPath(String)`: Navigates to a specified absolute or relative path.

### File Operation Actions
- `Delete`: Deletes the selected entry.
- `CreateFile`: Initiates the creation of a new file.
- `CreateDirectory`: Initiates the creation of a new directory.
- `CreateFileWithName(String)`: Creates a new file with the given name.
- `CreateDirectoryWithName(String)`: Creates a new directory with the given name.
- `RenameEntry(String)`: Renames the selected entry to the given name.
- `Copy { source, dest }`: Copies a file or directory.
- `Move { source, dest }`: Moves a file or directory.
- `Rename { source, new_name }`: Renames a file or directory.
- `CancelFileOperation { operation_id }`: Cancels an ongoing file operation.
- `ReloadDirectory`: Reloads the contents of the current directory.
- `OpenFile(PathBuf, Option<u32>)`: Opens a file with an external editor, optionally at a specific line.

### Search Actions
- `ToggleFileNameSearch`: Toggles the file name search overlay.
- `ToggleContentSearch`: Toggles the content search (ripgrep) overlay.
- `FileNameSearch(String)`: Performs an instant file name search with the given pattern.
- `ContentSearch(String)`: Performs a content search with the given pattern.
- `DirectContentSearch(String)`: Performs a content search directly without showing the overlay first.

### Result & State Update Actions
- `ShowSearchResults(...)`: Displays a list of search results.
- `ShowFilenameSearchResults(...)`: Displays a list of filename search results.
- `ShowRichSearchResults(...)`: (Deprecated) Displays rich content search results.
- `ShowRawSearchResults(...)`: Displays raw, colored search results from ripgrep.
- `TaskResult(...)`: Represents a result from a background task.
- `UpdateObjectInfo { parent_dir, info }`: Updates the metadata of a specific file or directory in the UI.
- `DirectoryScanUpdate { path, update }`: Handles a streaming update from the directory scanner.

### Input Prompt Actions
- `ShowInputPrompt(InputPromptType)`: Shows an input prompt of a specific type.
- `SubmitInputPrompt(String)`: Submits the input from the active prompt.

### Sorting & Filtering Actions
- `Sort(String)`: Sorts the entries based on a given criteria.
- `Filter(String)`: Filters the entries based on a given criteria.
