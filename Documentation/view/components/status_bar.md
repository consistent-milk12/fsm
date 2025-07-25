# Status Bar Component Documentation

## Overview
The `status_bar` component provides the UI for the status bar at the bottom of the TUI, displaying information such as the current directory, selection status, active mode, and quick hints.

## Features
- Displays current working directory and selection info
- Shows active mode and quick action hints
- Integrates with the main UI for persistent feedback

## Usage
- Render the status bar as part of the main UI layout
- Update the displayed information as the application state changes

## Example
```rust
status_bar::render(frame, &status_state, area);
```
