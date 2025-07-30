# FSM TSV-Based Systematic Tracing Guide

**Goal**: Leverage Rust's `tracing` crate with TSV (Tab-Separated Values) logging to create AI-optimized, token-efficient logs for rapid diagnosis of complex issues using CLI tools and AI analysis.

## Core Principles

1. **TSV-First Structure**: Maximum token efficiency for AI model analysis
2. **Fixed Column Schema**: Consistent field positions for reliable parsing
3. **Context Preservation**: Critical state in every trace event
4. **AI-Optimized**: 6x more token efficient than JSON for AI processing
5. **CLI-Friendly**: Works seamlessly with cut, awk, sort, and modern tools

## TSV Log Architecture

### Output Formats
- **Primary**: `logs/fsm-core.tsv` - AI-optimized structured data
- **Secondary**: `logs/fsm-core.json` - Complex CLI queries (optional)
- **Fallback**: `logs/fsm-core.log` - Human debugging (optional)
- **Errors**: `logs/errors.tsv` - Critical events only

### TSV Schema (Fixed Columns)
```
timestamp	level	target	marker	operation_type	current_path	target_path	entries_count	selected_index	duration_us	cache_hit	area_width	area_height	message
2025-07-29T14:08:33.123Z	INFO	fs_dispatcher	ENTER_START	directory_entry	/home/user/project	/home/user/project/sub	19	2	NULL	false	NULL	NULL	Navigation initiated
2025-07-29T14:08:33.145Z	INFO	fs_dispatcher	ENTER_COMPLETE	directory_entry	/home/user/project/sub	NULL	2	0	1250	false	NULL	NULL	Navigation completed
2025-07-29T14:08:33.167Z	INFO	object_table	UI_RENDER_START	file_table_render	/home/user/project/sub	NULL	2	0	NULL	NULL	94	44	UI render initiated
2025-07-29T14:08:33.170Z	INFO	object_table	UI_RENDER_COMPLETE	file_table_render	/home/user/project/sub	NULL	2	0	3420	NULL	94	44	UI render completed
```

**Schema Benefits:**
- **Predictable Positions**: Column 4 = marker, Column 8 = entries_count
- **NULL Handling**: Empty fields use "NULL" for clarity
- **AI Parsing**: Simple split('\t') operations
- **Backward Compatible**: Easy to add columns at end

## Tracing Architecture

### Level Strategy
```rust
// ERROR: System failures, panics, corrupted state
// WARN:  Recoverable issues, performance degradation  
// INFO:  Key operations, state changes, user actions
// DEBUG: Detailed flow, intermediate results
// TRACE: Verbose internals, loops, frequent operations
```

### TSV Instrumentation Patterns

**Function Entry/Exit with TSV:**
```rust
#[instrument(
    level = "info",
    skip(self, complex_param),
    fields(
        marker = "ENTER_START",
        operation_type = "directory_entry",
        current_path = %self.current_path().display(),
        target_path = %target_path.display(),
        entries_count = self.entry_count(),
        selected_index = self.selected_index(),
        duration_us = tracing::field::Empty,
        cache_hit = false,
        result = tracing::field::Empty
    )
)]
async fn navigate_to(&self, target_path: PathBuf) -> Result<DispatchResult> {
    let start = std::time::Instant::now();
    let span = tracing::Span::current();
    
    // Update for completion
    span.record("marker", "ENTER_COMPLETE");
    span.record("entries_count", entries.len());
    span.record("duration_us", start.elapsed().as_micros());
    span.record("result", "success");
    
    info!("Directory navigation operation completed");
    Ok(DispatchResult::Continue)
}
```

**State Change Events:**
```rust
// Before state change
info!(
    marker = "STATE_BEFORE",
    operation_type = "navigate_to_parent",
    current_path = %current_path.display(),
    entries_count = current_entries,
    selected_index = current_selected,
    "State snapshot before operation"
);

// After state change
info!(
    marker = "STATE_AFTER", 
    operation_type = "navigate_to_parent",
    current_path = %new_path.display(),
    entries_count = new_entries,
    selected_index = new_selected,
    duration_us = duration.as_micros(),
    cache_hit = cache_was_used,
    "State snapshot after operation"
);
```

## TSV Marker System

### Standardized Markers (Column 4)
```rust
// Application Lifecycle
"APP_START" | "APP_STARTUP_COMPLETE" | "APP_EXIT_CLEAN" | "APPLICATION_PANIC"

// Configuration Management
"CONFIG_LOAD_START" | "CONFIG_PARSE_ERROR" | "CONFIG_NOT_FOUND" | "CONFIG_SAVE_START" | "CONFIG_LOAD_SUCCESS" | "CONFIG_LOAD_FAILED"

// Cache Management
"CACHE_INIT" | "CACHE_LOAD_FAILED" | "CACHE_STATS_DISABLED" | "CACHE_CLEARED" | "CACHE_LOW_HIT_RATE" | "CACHE_HIGH_MEMORY_USAGE" | "CACHE_HIGH_EXCEPTION_RATE" | "CACHE_HEALTH_CHECK"

// Event Loop & Dispatcher
"EVENT_LOOP_STARTED" | "TERMINAL_EVENT_ERROR" | "TERMINAL_STREAM_CLOSED" | "KEY_MAPPED_TO_ACTION" | "TERMINAL_RESIZE" | "MOUSE_EVENT_UNHANDLED" | "TERMINAL_FOCUS_GAINED" | "TERMINAL_FOCUS_LOST" | "PASTE_EVENT_UNIMPLEMENTED" | "KEY_NOT_MAPPED" | "KEY_MAPPED_SUCCESS" | "NO_ENTRY_AT_INDEX" | "ACTION_DISPATCHING" | "ACTION_DISPATCH_SUCCESS" | "ACTION_DISPATCH_FAILED" | "TASK_SEND_FAILED" | "TASK_RESULT_RECEIVED_LEGACY" | "NO_TASK_RESULT_LEGACY" | "SHUTDOWN_HANDLE_RETRIEVED" | "SHUTDOWN_SIGNAL_SENT" | "METRICS_SNAPSHOT_CREATED"
"DISPATCHER_HANDLED" | "DISPATCHER_FAILED" | "DISPATCHER_ENTRY_CREATED" | "DISPATCHER_HANDLER_REGISTERED" | "DISPATCHER_INITIALIZED" | "ACTION_DISPATCH_START" | "QUIT_ACTION_PROCESSED" | "NO_HANDLER_FOUND" | "ACTION_DISPATCH_FAILED" | "ACTION_DISPATCH_COMPLETE" | "ACTION_DISPATCH_ERROR" | "HANDLER_SELECTED" | "HANDLER_EXECUTED_SUCCESS" | "HANDLER_EXECUTION_FAILED" | "ACTION_PRIORITY_DETERMINED" | "DISPATCHER_STATS_COLLECTED" | "HANDLER_STATE_CHANGED" | "HANDLER_NOT_FOUND_OR_DISABLED"

// Navigation Flow
"ENTER_START" | "ENTER_COMPLETE" | "ENTER_ERROR" | "ENTER_SELECTED_START"
"BACKSPACE_START" | "BACKSPACE_COMPLETE" | "BACKSPACE_ERROR"
"SELECTION_MOVED" | "SELECTION_MOVED_UP" | "SELECTION_MOVE_UP_BLOCKED" | "SELECTION_MOVED_DOWN" | "SELECTION_MOVE_DOWN_BLOCKED" | "SELECTION_FIRST" | "SELECTION_FIRST_BLOCKED" | "SELECTION_LAST" | "SELECTION_LAST_BLOCKED" | "PAGE_UP" | "PAGE_DOWN"
"NAVIGATE_TO_START" | "NAVIGATE_TO_COMPLETE" | "NAVIGATE_PARENT_START" | "NAVIGATE_PARENT_COMPLETE" | "NAVIGATE_PARENT_BLOCKED" | "NAVIGATE_BACK" | "NAVIGATE_BACK_BLOCKED" | "NAVIGATE_FORWARD" | "NAVIGATE_FORWARD_BLOCKED" | "ADD_TO_HISTORY" | "HISTORY_CAPACITY_EXCEEDED" | "HISTORY_UPDATED"

// File System Operations
"RELOAD_DIRECTORY_START" | "FILE_CREATED" | "DIRECTORY_CREATED" | "RENAME_COMPLETE" | "FILE_DELETED" | "DIRECTORY_DELETED"
"DIRECTORY_SCAN_START" | "FAST_SCAN_COMPLETE" | "STREAMING_SCAN_COMPLETE" | "TWO_PHASE_SCAN_COMPLETE" | "FAST_SCAN_START" | "FAST_SCAN_CANCELLED" | "DIRECTORY_ENTRIES_SORTED" | "SEND_SCAN_RESULT_FAILED" | "FAST_SCAN_FINAL" | "STREAMING_SCAN_START" | "STREAMING_SCAN_CANCELLED" | "READ_DIRECTORY_ENTRY_FAILED" | "SEND_SCAN_PROGRESS_FAILED" | "SEND_SCAN_COMPLETION_FAILED" | "STREAMING_SCAN_FINAL" | "TWO_PHASE_SCAN_START" | "SPAWN_METADATA_TASK" | "TWO_PHASE_SCAN_FINAL" | "LIGHT_METADATA_SCAN_CANCELLED" | "LIGHT_METADATA_SCAN_COMPLETE" | "SPAWN_FILE_OPERATION" | "FILE_OPERATION_TASK_FAILED" | "SPAWN_DIRECTORY_SCAN" | "SPAWN_FAST_DIRECTORY_SCAN" | "STREAMING_SCAN_WITH_UPDATES_START" | "STREAMING_SCAN_WITH_UPDATES_CANCELLED" | "BATCH_PROGRESS_UPDATE_SENT" | "STREAMING_SCAN_WITH_UPDATES_COMPLETE" | "TWO_PHASE_SCAN_WITH_UPDATES_START" | "TWO_PHASE_SCAN_WITH_UPDATES_COMPLETE_SUCCESS" | "TWO_PHASE_SCAN_WITH_UPDATES_FAILED" | "LEGACY_DIRECTORY_SCAN_SPAWN"

// UI Rendering
"UI_RENDER_START" | "UI_RENDER_COMPLETE" | "UI_CLEAR" | "OVERLAY_TOGGLE" | "COMMAND_MODE_ENTER" | "INPUT_PROMPT_SHOW" | "PROMPT_BUFFER_UPDATE" | "TICK_MAIN_REDRAW" | "QUIT_RECEIVED" | "ACTION_NOT_HANDLED" | "REDRAW_REQUESTED" | "REDRAW_CHECK" | "REDRAW_CLEARED" | "PROMPT_SET" | "PROMPT_INSERT" | "PROMPT_BACKSPACE" | "HISTORY_PUSH"

// Performance Tracking
"PERF_DIRECTORY_SCAN" | "PERF_FRAME_RENDER" | "PERF_STATE_UPDATE" | "PERFORMANCE_METRICS_REPORT" | "MEMORY_USAGE" | "FINAL_PERFORMANCE_SUMMARY" | "PERFORMANCE_METRICS_COLLECTED"

// Error Handling
"ERROR_NAVIGATION" | "ERROR_RENDER" | "ERROR_STATE" | "NOTIFICATION_ERROR" | "APPLICATION_PANIC" | "TERMINAL_CLEANUP_FAILED" | "FRAME_RENDER_FAILED"

// Clipboard Operations
"CLIPBOARD_COPY_START" | "CLIPBOARD_CUT_START" | "CLIPBOARD_PASTE_START" | "CLIPBOARD_PASTE_ITEM_SUCCESS" | "CLIPBOARD_PASTE_ITEM_FAILED" | "CLIPBOARD_PASTE_BATCH_COMPLETE" | "CLIPBOARD_CLEAR_SUCCESS"

// State Management (General)
"STATE_BEFORE" | "STATE_AFTER" | "STATE_ERROR" | "STATE_COORDINATOR_INIT" | "APP_STATE_LOCK" | "FS_STATE_LOCK" | "UI_STATE_CLONE" | "UI_STATE_WRITE_LOCK" | "UI_STATE_UPDATED" | "UI_STATE_LOCK_POISONED" | "REDRAW_CHECK_RESULT" | "UI_STATE_READ_LOCK_POISONED" | "WITH_ALL_STATES_ENTER" | "WITH_ALL_STATES_EXIT" | "FS_STATE_UPDATED" | "DIRECTORY_LOAD_PATH_MISMATCH" | "SEARCH_RESULTS_UPDATED" | "PANE_STATE_CLONE" | "GET_ACTIVE_PANE" | "GET_MUTABLE_ACTIVE_PANE" | "DIRECTORY_CACHE_INVALIDATED"

// Task Management
"TASK_INFO_INIT" | "TASK_ELAPSED_TIME" | "TASK_ADDED" | "TASK_WITH_OPERATION_ADDED" | "TASK_REMOVED" | "TASK_RETRIEVED" | "TASK_COMPLETION_SIGNALED" | "TASK_COMPLETED_IN_APP_STATE" | "TASK_SEND_FAILED" | "TASK_RESULT_RECEIVED_LEGACY" | "NO_TASK_RESULT_LEGACY" | "PROGRESS_UPDATE_RECEIVED" | "GENERIC_TASK_COMPLETE" | "GENERIC_TASK_FAILED" | "COMPLETED_TASKS_CLEANED_UP"

// Search
"SEARCH_QUERY_ADDED" | "SEARCH_COMPLETE" | "CONTENT_SEARCH_COMPLETE" | "SEARCH_RESULTS_SET" | "SEARCH_CLEARED"

// Metrics
"ACTION_RECORDED" | "METRICS_RETRIEVED"

// File Selection
"FILE_MARKED" | "FILE_UNMARKED" | "ALL_MARKS_CLEARED" | "IS_MARKED_QUERY" | "RETRIEVE_MARKED_PATHS" | "MARK_VALID_ENTRY" | "MARK_INVALID_ENTRY" | "ENTRY_UNMARKED" | "UNMARK_NO_ENTRY" | "IS_MARKED_CHECK" | "CLEAR_ALL_MARKS" | "GET_SELECTED_PATH" | "NO_MARKED_ENTRIES" | "RETURN_MARKED_PATHS"

// History & Bookmarks
"HISTORY_EVENT_ADDED" | "BOOKMARK_ADDED" | "GET_BOOKMARK" | "REMOVE_BOOKMARK" | "RECENT_DIR_EXISTS" | "RECENT_DIR_CAPACITY_EXCEEDED" | "RECENT_DIR_UPDATED"

// Favorites
"FAVORITE_ADDED" | "FAVORITE_REMOVED" | "IS_FAVORITE_CHECK"

// Operation Tracking
"OPERATION_START" | "OPERATION_UPDATE" | "UPDATE_NON_EXISTENT_OPERATION" | "OPERATION_COMPLETE" | "COMPLETE_NON_EXISTENT_OPERATION" | "CHECK_ACTIVE_OPERATIONS" | "NO_ACTIVE_OPERATIONS_SUMMARY" | "OPERATION_SUMMARY" | "OPERATION_CANCELLED_BEFORE_EXECUTION" | "FILE_OPERATION_COMPLETE" | "FILE_OPERATION_FAILED" | "CLIPBOARD_OPERATION_COMPLETE" | "CLIPBOARD_OPERATION_FAILED"

// Terminal
"TERMINAL_SETUP_COMPLETE" | "TERMINAL_CLEANUP_COMPLETE" | "TERMINAL_CLEANUP_FINAL"

// Handlers
"FILE_OPS_HANDLER_KEY_EVENT" | "KEYBOARD_UNHANDLED_KEYS" | "KEYBOARD_DEBUG_MODE" | "NAVIGATION_HANDLER_KEY_EVENT" | "SEARCH_HANDLER_KEY_EVENT"

// System Monitoring
"SYSTEM_MONITOR_TOGGLE" | "PROCESS_MONITOR_TOGGLE" | "PROCESS_KILL_ATTEMPT" | "PROCESS_KILL_SUCCESS" | "PROCESS_KILL_FAILED"
"SYSTEM_MONITOR_UPDATE" | "PROCESS_MONITOR_UPDATE" | "SYSTEM_DATA_COLLECTED" | "PROCESS_DATA_COLLECTED"

// Command Palette
"COMMAND_PALETTE_SINGLE_COMPLETION" | "COMMAND_PALETTE_APPLY_COMPLETION_START" | "COMMAND_PALETTE_APPLY_COMPLETION_BEFORE" | "COMMAND_PALETTE_APPLY_COMPLETION_AFTER"

// General
"DEFAULT_FS_STATE_CREATED" | "APP_STATE_DEFAULT_CREATED" | "OPTIMIZED_SORT_START" | "APPLY_FILTER_START" | "GET_SELECTED_ENTRY" | "SCROLL_ADJUST_UP" | "SCROLL_ADJUST_DOWN" | "SCROLL_ADJUST_NONE" | "SCROLL_ADJUSTED"
```

### Navigation Actions
```rust
info!(
    marker = "ENTER_START",
    operation_type = "directory_entry",
    current_path = %current.display(),
    target_path = %target.display(),
    entries_count = current_entries,
    selected_index = current_selected,
    "Directory entry navigation initiated"
);

info!(
    marker = "BACKSPACE_START",
    operation_type = "parent_navigation", 
    current_path = %current.display(),
    target_path = %parent.display(),
    entries_count = current_entries,
    selected_index = current_selected,
    "Parent navigation initiated"
);
```

### UI Component Events
```rust
info!(
    marker = "UI_RENDER_START",
    operation_type = "file_table_render",
    current_path = %path.display(),
    entries_count = entries.len(),
    selected_index = selected,
    area_width = area.width,
    area_height = area.height,
    "UI component render initiated"
);

info!(
    marker = "UI_RENDER_COMPLETE",
    operation_type = "file_table_render",
    current_path = %path.display(),
    entries_count = entries.len(), 
    duration_us = duration.as_micros(),
    area_width = area.width,
    area_height = area.height,
    "UI component render completed"
);
```

### Performance Events
```rust
info!(
    marker = "PERF_DIRECTORY_SCAN",
    operation_type = "directory_scan",
    current_path = %dir_path.display(),
    entries_count = entries_processed,
    duration_us = duration.as_micros(),
    cache_hit = was_cached,
    "Directory scan performance measurement"
);

info!(
    marker = "PERF_FRAME_RENDER", 
    operation_type = "frame_render",
    duration_us = duration.as_micros(),
    entries_count = components_rendered,
    area_width = screen_width,
    area_height = screen_height,
    "Frame render performance measurement"
);
```

## TSV CLI Analysis Commands

### Basic TSV Operations
```bash
# Header for reference
echo "timestamp level target marker operation_type current_path target_path entries_count selected_index duration_us cache_hit area_width area_height message"

# Filter by marker (e.g., all navigation starts)
awk -F'\t' '$4 ~ /_START$/ {print $1, $4, $5, $6}' logs/fsm-core.tsv

# Filter by operation type (e.g., all file system operations)
awk -F'\t' '$5 == "file_system" {print $1, $4, $5, $6, $14}' logs/fsm-core.tsv

# Events with specific marker and level
awk -F'\t' '$2 == "ERROR" && $4 == "FILE_OPERATION_FAILED" {print $1, $4, $5, $14}' logs/fsm-core.tsv
```

### Advanced TSV Analysis

```bash
# Application Lifecycle: Track startup and shutdown
awk -F'\t' '$4 ~ /^(APP_START|APP_STARTUP_COMPLETE|APP_EXIT_CLEAN|APPLICATION_PANIC)/ {print $1, $4, $14}' logs/fsm-core.tsv

# Configuration Management: Monitor config loading and saving
awk -F'\t' '$5 == "config" {print $1, $4, $5, $14}' logs/fsm-core.tsv

# Cache Management: Diagnose cache behavior
awk -F'\t' '$5 == "cache" || $5 == "cache_load" || $5 == "cache_health" {print $1, $4, $5, $14}' logs/fsm-core.tsv

# Event Loop & Dispatcher: Analyze action flow and performance
awk -F'\t' '$5 ~ /^(event_loop|action_dispatch|input_processing)/ {print $1, $4, $5, $10, $14}' logs/fsm-core.tsv

# File System Operations: Trace file and directory actions
awk -F'\t' '$5 == "file_system" {print $1, $4, $5, $6, $7, $14}' logs/fsm-core.tsv

# UI Rendering: Check UI performance and redraws
awk -F'\t' '$5 == "ui_render" {print $1, $4, $5, $10, $12, $13, $14}' logs/fsm-core.tsv

# Clipboard Operations: Monitor clipboard activity
awk -F'\t' '$5 == "clipboard" {print $1, $4, $5, $14}' logs/fsm-core.tsv

# State Management (General): Observe state changes
awk -F'\t' '$5 == "state_management" {print $1, $4, $5, $14}' logs/fsm-core.tsv

# Task Management: Track background tasks
awk -F'\t' '$5 == "task_management" {print $1, $4, $5, $14}' logs/fsm-core.tsv

# Search: Analyze search queries and results
awk -F'\t' '$5 == "search" {print $1, $4, $5, $14}' logs/fsm-core.tsv

# Metrics: Review performance metrics
awk -F'\t' '$5 == "metrics" {print $1, $4, $5, $10, $14}' logs/fsm-core.tsv

# File Selection: Monitor file marking and unmarking
awk -F'\t' '$5 == "file_selection" {print $1, $4, $5, $6, $14}' logs/fsm-core.tsv

# History & Bookmarks: Trace navigation history and bookmark usage
awk -F'\t' '$5 ~ /^(navigation|bookmark)/ {print $1, $4, $5, $6, $14}' logs/fsm-core.tsv

# Favorites: Track favorite directory management
awk -F'\t' '$5 == "favorites" {print $1, $4, $5, $6, $14}' logs/fsm-core.tsv

# Operation Tracking: Monitor long-running operations
awk -F'\t' '$5 == "operation_tracking" {print $1, $4, $5, $14}' logs/fsm-core.tsv

# Terminal: Check terminal setup and cleanup
awk -F'\t' '$5 == "terminal" {print $1, $4, $5, $14}' logs/fsm-core.tsv

# Handlers: Debug handler specific events
awk -F'\t' '$5 == "input_handling" {print $1, $4, $5, $14}' logs/fsm-core.tsv

# Command Palette: Analyze command palette interactions
awk -F'\t' '$5 == "command_palette" {print $1, $4, $5, $14}' logs/fsm-core.tsv

# System Monitoring: Track system monitoring events
awk -F'\t' '$5 == "system_monitoring" {print $1, $4, $5, $14}' logs/fsm-core.tsv

# Compare Enter vs Backspace flows (columns 1,4,6,8)
awk -F'\t' '$4 ~ /^(ENTER_|BACKSPACE_)/ {print $1, $4, $6, $8}' logs/fsm-core.tsv

# State change correlation (before/after pairs)
awk -F'\t' '$4 == "STATE_BEFORE" || $4 == "STATE_AFTER" {print $1, $4, $5, $6, $8, $10}' logs/fsm-core.tsv

# Performance analysis - operations over 1ms (column 10)
awk -F'\t' '$10 != "NULL" && $10 > 1000 {print $5, $10, $1}' logs/fsm-core.tsv

# Frame rate analysis (60fps = 16667µs)
awk -F'\t' '$4 == "PERF_FRAME_RENDER" && $10 != "NULL" {
    if($10 <= 16667) fast++; else slow++;
    total++;
} END {
    printf "Frame rate: %.1f%% @60fps, %.1f%% <60fps\n", 
           fast*100/total, slow*100/total
}' logs/fsm-core.tsv
```

### Error Analysis
```bash
# Error clustering by operation type (column 5)
awk -F'\t' '$2 == "ERROR" {count[$5]++} END {
    for(op in count) print count[op], op
}' logs/fsm-core.tsv | sort -nr

# Error recovery time analysis
awk -F'\t' '
    $4 ~ /ERROR/ {error_time = $1; error_line = NR}
    $4 ~ /COMPLETE/ && error_time != "" {
        recovery_lines = NR - error_line;
        print "Recovery:", recovery_lines "lines", $1;
        error_time = ""
    }
'
 logs/fsm-core.tsv

# State consistency verification
awk -F'\t' '$4 == "STATE_AFTER" {
    state = $6 FS $8 FS $9;  # path + entries + selected
    if(seen[state]++) print "Duplicate state:", state, "at", $1
}' logs/fsm-core.tsv
```


## AI Analysis Benefits

### Token Efficiency Comparison
```
JSON Event:   ~300 tokens
TSV Row:      ~50 tokens  
Efficiency:   6x improvement
```

### AI Pattern Recognition
```
# AI can easily parse patterns:
ENTER_START -> ENTER_COMPLETE (success flow)
ENTER_START -> ENTER_ERROR (failure flow)  
BACKSPACE_START -> STATE_AFTER (parent navigation)
UI_RENDER_START -> UI_RENDER_COMPLETE (render cycle)
```

### AI-Friendly Queries
```bash
# Extract specific columns for AI analysis
cut -f1,4,5,6,8,10 logs/fsm-core.tsv > analysis_subset.tsv

# Create operation summaries for AI
awk -F'\t' 'BEGIN{print "operation_type count avg_duration_us avg_entries"}
    $10 != "NULL" {sum[$5] += $10; entries[$5] += $8; count[$5]++}
    END {for(op in sum) printf "%s %d %.0f %.0f\n", 
         op, count[op], sum[op]/count[op], entries[op]/count[op]}' \
    logs/fsm-core.tsv

# Time-series data for AI trend analysis  
awk -F'\t' '$4 == "PERF_FRAME_RENDER" && $10 != "NULL" {
    print $1, $10
}' logs/fsm-core.tsv > frame_performance_timeseries.tsv
```

## Development Workflow Integration

### Real-time TSV Monitoring
```bash
# Live structured event monitoring
tail -f logs/fsm-core.tsv | awk -F'\t' '
    $2 ~ /(ERROR|WARN)/ || $4 ~ /^(ENTER_|BACKSPACE_|ERROR_)/ {
        printf "%-8s %-5s %-20s %-40s %s\n", 
               substr($1,12,8), $2, $4, $6, $8
    }'

# Performance dashboard  
watch -n 2 'echo "=== TSV PERFORMANCE METRICS ===";
echo "Recent render performance:";
tail -20 logs/fsm-core.tsv | awk -F"\t" "\$4 == \"PERF_FRAME_RENDER\" && \$10 != \"NULL\" {sum+=\$10; count++} END {if(count>0) printf \"Avg: %.0fµs (%.1f fps)\\n\", sum/count, 1000000/(sum/count)}";
echo "Error count:";
awk -F"\t" "\$2 == \"ERROR\"" logs/fsm-core.tsv | wc -l'

# Issue investigation by marker
function trace_tsv_issue() {
    local marker_pattern=$1
    awk -F'\t' -v pattern="$marker_pattern" '$4 ~ pattern {
        printf "%-23s %-20s %-20s %-40s %s %s\n", 
               $1, $4, $5, $6, $8, $14
    }' logs/fsm-core.tsv | less
}
```

### Test Verification
```bash
# Automated TSV-based navigation verification
function verify_tsv_navigation() {
    local operation_type=$1
    local latest_before=$(awk -F'\t' -v op="$operation_type" '
        $4 == "STATE_BEFORE" && $5 == op {entries = $8} 
        END {print entries}' logs/fsm-core.tsv)
    local latest_after=$(awk -F'\t' -v op="$operation_type" '
        $4 == "STATE_AFTER" && $5 == op {entries = $8} 
        END {print entries}' logs/fsm-core.tsv)
    
    if [ -z "$latest_before" ] || [ -z "$latest_after" ]; then
        echo "❌ Navigation verification failed: Missing state data"
        return 1
    elif [ "$operation_type" = "parent_navigation" ] && [ "$latest_after" -le "$latest_before" ]; then
        echo "❌ Parent navigation failed: $latest_before -> $latest_after (should increase)"
        return 1
    else
        echo "✅ Navigation verified: $latest_before -> $latest_after entries"
        return 0
    fi
}

# Performance regression detection
function check_tsv_performance() {
    local marker=$1
    local threshold_us=$2
    
    local recent_avg=$(awk -F'\t' -v marker="$marker" '
        $4 == marker && $10 != "NULL" {sum += $10; count++} 
        END {if(count > 0) print sum/count; else print 0}' logs/fsm-core.tsv)
    
    if (( $(echo "$recent_avg > $threshold_us" | bc -l) )); then
        echo "⚠️  Performance regression: $marker avg ${recent_avg}µs > ${threshold_us}µs"
        return 1
    else
        echo "✅ Performance OK: $marker avg ${recent_avg}µs"
        return 0
    fi
}
```

## Log File Management

### TSV-Specific Operations
```bash
# Validate TSV integrity (13 columns expected)
awk -F'\t' 'NF != 14 {print "Invalid row", NR ": " NF " fields"}' logs/fsm-core.tsv

# Extract specific time windows
awk -F'\t' '$1 >= "2025-07-29T14:00:00" && $1 <= "2025-07-29T15:00:00"' logs/fsm-core.tsv

# Create compressed analysis files
cut -f1,4,5,8,10 logs/fsm-core.tsv | gzip > analysis_core.tsv.gz

# Rotate and compress old TSV logs
if [ $(stat -c%s logs/fsm-core.tsv 2>/dev/null) -gt 104857600 ]; then
    mv logs/fsm-core.tsv logs/fsm-core.tsv.$(date +%s)
    gzip logs/fsm-core.tsv.$(date +%s)
fi
```

## Troubleshooting Decision Tree

### 1. Performance Issues
```bash
# Identify slowest operations by marker
awk -F'\t' '$10 != "NULL" && $10 > 0 {sum[$4] += $10; count[$4]++} 
    END {for(m in sum) printf "%.0f %s\n", sum[m]/count[m], m}' \
    logs/fsm-core.tsv | sort -nr | head -10

# Find performance outliers  
awk -F'\t' '$10 != "NULL" && $10 > 5000 {
    printf "%-23s %-20s %6dµs %-40s\n", $1, $4, $10, $6
}' logs/fsm-core.tsv
```

### 2. State Corruption  
```bash
# Trace state inconsistencies
awk -F'\t' '$4 ~ /STATE_(BEFORE|AFTER)/ {
    if($4 == "STATE_BEFORE") {before = $8; before_path = $6}
    else if(before != "" && $6 == before_path) {
        print "State change:", before "->" $8 " at " $6;
        before = ""
    }
}' logs/fsm-core.tsv
```

### 3. UI Issues
```bash
# UI-Backend synchronization check
awk -F'\t' '
    $4 == "UI_RENDER_START" {ui_entries = $8; ui_path = $6}
    $4 == "STATE_AFTER" && $6 == ui_path && ui_entries != "" {
        if($8 != ui_entries) print "SYNC ISSUE:", ui_path, "UI=" ui_entries, "State=" $8
        ui_entries = ""
    }
' logs/fsm-core.tsv
```

This TSV-based approach provides maximum efficiency for AI analysis while maintaining full CLI tool compatibility, enabling rapid diagnosis of complex issues with minimal token overhead.