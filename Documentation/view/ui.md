# UI Module Documentation

## Overview
The `ui` module provides the main rendering logic for the TUI, including layout, overlays, and notifications. It coordinates drawing the main object table, status bar, overlays, and notifications for each frame.

## Features
- Main UI redraw logic for each frame
- Overlay rendering for help, search, file operations, notifications, etc.
- Calculates overlay positions and manages stacking order

## Usage
- Use `View::redraw` to render the full UI for a frame
- Overlays and notifications are rendered on top of the main UI as needed
