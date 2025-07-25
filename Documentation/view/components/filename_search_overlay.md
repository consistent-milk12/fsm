# Filename Search Overlay Component Documentation

## Overview
The `filename_search_overlay` component provides the UI for the filename search overlay, allowing users to enter filename patterns and view live search results as they are streamed from the background task.

## Features
- Input field for entering filename search patterns
- Displays live search results as they are found
- Keyboard navigation for result selection
- Integration with the main UI for overlay management

## Usage
- Render the filename search overlay when the user initiates a filename search
- Update the input and results as the search progresses
- Allow navigation and selection of search results

## Example
```rust
filename_search_overlay::render(frame, &search_state, area);
```
