# Content Search Overlay Component Documentation

## Overview
The `content_search_overlay` component provides the UI for the content search overlay, allowing users to enter search patterns and view live results from content searches (e.g., using ripgrep).

## Features
- Input field for entering content search patterns
- Displays live search results as they are streamed from the background task
- Keyboard navigation for result selection
- Integration with the main UI for overlay management

## Usage
- Render the content search overlay when the user initiates a content search
- Update the input and results as the search progresses
- Allow navigation and selection of search results

## Example
```rust
content_search_overlay::render(frame, &search_state, area);
```
