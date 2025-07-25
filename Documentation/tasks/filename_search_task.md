# Filename Search Task Documentation

## Overview
The `filename_search_task` module provides an async background task for recursively searching for filenames using external tools (`fd` or `find`). It streams results to the UI, supports progress reporting, and includes robust error handling and logging.

## Features
- Uses `fd` or `find` for fast, recursive filename search
- Streams results as `ObjectInfo` to the UI
- Progress and error reporting
- Fallback strategy if preferred tool is unavailable
- Performance metrics and logging

## Main Types
- `filename_search_task`: Main async search function
- `TaskResultExt`: Helper trait for reporting task results

## Usage
- Call `filename_search_task` with a pattern, search path, and channels for task and action results
- Results are streamed to the UI for responsive updates

---

# Metadata Task Documentation

## Overview
The `metadata_task` module provides background tasks for loading file and directory metadata (size, modification time, item count) to improve UI responsiveness.

## Features
- Loads metadata asynchronously for individual or multiple entries
- Batch loading with configurable batch size
- Reports results to the UI via actions

## Main Types
- `load_metadata_task`: Loads metadata for a single entry
- `batch_load_metadata_task`: Loads metadata for multiple entries

## Usage
- Call `load_metadata_task` or `batch_load_metadata_task` with the relevant parameters
- Results are sent to the UI for updating file/directory info

---

# Search Task Documentation

## Overview
The `search_task` module provides an async background task for content searching using `ripgrep` (rg). It streams raw output to the UI for direct display.

## Features
- Uses `ripgrep` for fast, recursive content search
- Streams raw and parsed results to the UI
- Handles ANSI color codes for TUI display
- Parses file and line info from output

## Main Types
- `search_task`: Main async search function
- `RawSearchResult`: Struct for search results
- `TaskResultExt`: Helper trait for reporting task results

## Usage
- Call `search_task` with a pattern, path, and channels for task and action results
- Results are streamed to the UI for direct display

---

# Size Task Documentation

## Overview
The `size_task` module provides a background task for calculating the recursive size and direct item count of directories.

## Features
- Calculates total size and item count for directories
- Runs in the background to avoid blocking the UI
- Reports results to the UI via actions

## Main Types
- `calculate_size_task`: Main async size calculation function

## Usage
- Call `calculate_size_task` with the relevant parameters
- Results are sent to the UI for updating directory info
