# Input Prompt Overlay Component Documentation

## Overview
The `input_prompt_overlay` component provides the UI for input prompts, allowing users to enter text or confirm actions in a modal overlay. It is used for renaming, creating files/folders, or other user input scenarios.

## Features
- Modal input field for text entry
- Displays prompt messages and validation feedback
- Keyboard navigation and confirmation/cancellation
- Integrates with the main UI for overlay management

## Usage
- Render the input prompt overlay when user input is required
- Update the prompt message and input value as needed
- Handle confirmation and cancellation actions

## Example
```rust
input_prompt_overlay::render(frame, &prompt_state, area);
```
