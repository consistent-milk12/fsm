# Loading Overlay Component Documentation

## Overview
The `loading_overlay` component provides the UI for displaying loading indicators and progress spinners during background operations, such as directory scans or metadata loading.

## Features
- Animated spinner or progress bar for ongoing operations
- Displays operation messages and current item being processed
- Can be shown as a modal overlay to block input

## Usage
- Render the loading overlay when a background operation is in progress
- Update the message and progress as the operation proceeds

## Example
```rust
loading_overlay::render(frame, &loading_state, area);
```
