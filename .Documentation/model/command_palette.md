# CommandPaletteState Module Documentation

## Overview
The `CommandPaletteState` module manages the state and logic for the command palette overlay in the TUI file manager. It provides filtering, auto-completion, and execution of user commands, supporting both built-in and custom actions.

## Features
- Real-time filtering and auto-completion of commands based on user input
- Built-in and custom command support with descriptions
- Command parsing for arguments and aliases
- Integration with UI overlays and command mode
- Completion cycling and application

## Main Types
- `CommandPaletteState`: Holds input, filtered commands, completions, and selection state
- `Command`: Represents a user-invokable command (title and action)
- `CommandAction`: Enum of possible command actions (open config, reload, new file/folder, search, custom)

## Key Methods
- `CommandPaletteState::new(commands)`: Initialize with available commands
- `update_filter()`: Filter commands based on current input
- `update_completions()`: Update auto-completion suggestions
- `apply_completion()`, `next_completion()`, `prev_completion()`: Manage and apply completions
- `parse_command()`: Parse current input into a command action
- `show_completions_if_available()`, `hide_completions()`: Control completion visibility

## Usage
- Use `CommandPaletteState` to manage the command palette overlay and user input
- Call `update_filter` and `update_completions` on input changes
- Use `parse_command` to interpret user input and trigger actions
- Integrate with the UI for command mode and overlays

## Example
```rust
let mut palette = CommandPaletteState::new(get_all_commands());
palette.input = "nf myfile.txt".to_string();
palette.update_filter();
if let Some(action) = palette.parse_command() {
    // Execute the parsed command action
}
```
