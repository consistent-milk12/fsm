# Notification Overlay Component Documentation

## Overview
The `notification_overlay` component provides the UI for displaying notifications, such as info, warning, error, or success messages, in an overlay at the top or bottom of the TUI.

## Features
- Displays notifications with different severity levels
- Auto-dismisses notifications after a timeout (except errors)
- Can be stacked or replaced as new notifications arrive
- Integrates with the main UI for overlay management

## Usage
- Render the notification overlay when a notification is active
- Update the message and severity as notifications change

## Example
```rust
notification_overlay::render(frame, &notification_state, area);
```
