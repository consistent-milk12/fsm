# Prompt Bar Component Documentation

## Overview
The `prompt_bar` component provides a compact UI element for displaying prompts, status messages, or quick actions at the bottom or top of the TUI. It is used for transient feedback and user guidance.

## Features
- Displays prompt messages or status updates
- Can show quick action hints or shortcuts
- Integrates with overlays and main UI for context-sensitive feedback

## Usage
- Render the prompt bar as part of the main UI or overlays
- Update the message and actions as the application state changes

## Example
```rust
prompt_bar::render(frame, &prompt_state, area);
```
