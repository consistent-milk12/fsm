# Search Results Overlay Component Documentation

## Overview
The `search_results_overlay` component provides the UI for displaying the results of a search operation (filename or content search) in an overlay. It allows users to browse, select, and act on search results.

## Features
- Displays a list of search results with metadata
- Supports keyboard navigation and selection
- Integrates with the main UI for overlay management
- Can trigger actions on selected results

## Usage
- Render the search results overlay when a search completes or as results stream in
- Update the results and selection as the user navigates

## Example
```rust
search_results_overlay::render(frame, &results_state, area);
```
