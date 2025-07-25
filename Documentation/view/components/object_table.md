# Object Table Component Documentation

## Overview
The `object_table` component provides the main file/directory listing table in the TUI, displaying entries with icons, names, sizes, and other metadata. It supports selection, marking, sorting, and virtual scrolling for efficient navigation.

## Features
- Displays a table of files and directories with metadata
- Supports selection, multi-selection, and marking
- Integrates icons and color themes for better UX
- Virtual scrolling for large directories
- Sorting and filtering support

## Usage
- Render the object table as the main content area of the TUI
- Update entries, selection, and scroll offset as the user navigates

## Example
```rust
object_table::render(frame, &pane_state, area);
```
