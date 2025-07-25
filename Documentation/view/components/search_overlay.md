# Search Overlay Component Documentation

## Overview
The `search_overlay` component provides the UI for the search overlay, allowing users to enter search queries and view results for filename or content searches.

## Features
- Input field for entering search queries
- Displays search results as they are found
- Keyboard navigation for result selection
- Integration with the main UI for overlay management

## Usage
- Render the search overlay when the user initiates a search
- Update the input and results as the search progresses
- Allow navigation and selection of search results

## Example
```rust
search_overlay::render(frame, &search_state, area);
```
