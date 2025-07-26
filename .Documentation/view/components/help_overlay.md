# Help Overlay Component Documentation

## Overview
The `help_overlay` component provides the UI for the help overlay, displaying keybindings, command descriptions, and usage tips to assist users in navigating and using the TUI file manager.

## Features
- Displays a list of keybindings and their actions
- Shows command descriptions and usage hints
- Scrollable content for large help sections
- Can be toggled on/off as an overlay

## Usage
- Render the help overlay when the user requests help (e.g., by pressing `?`)
- Update content as keybindings or commands change
- Allow scrolling if the help content exceeds the visible area

## Example
```rust
help_overlay::render(frame, &help_content, area);
```
