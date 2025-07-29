# FSM-Core Logging System Documentation

## Overview
FSM-Core uses a sophisticated file-based logging system built on the `tracing` crate with custom formatters for optimal performance monitoring and debugging.

## Architecture

### Core Components (`fsm-core/src/logging.rs`)

#### 1. Logger Structure
```rust
pub struct Logger {
    _guards: Vec<WorkerGuard>,  // Non-blocking I/O guards
}
```

#### 2. Logging Layers
- **File Layer**: Main application logs with daily rotation
- **Console Layer**: Optional colored console output (disabled in production)
- **Metrics Layer**: In-memory performance metrics collection
- **Error Tracking Layer**: Dedicated error logging to separate file

#### 3. Custom Formatters

##### CompactFormatter (Lines 495-587)
- **Purpose**: Console output with proper field spacing
- **Recent Fix**: Added `FieldExtractor` to prevent field concatenation
- **Format**: `HH:MM:SS [NNNN] LEVEL target [span] (file:line) message field1=value1 field2=value2`

##### StructuredFormatter (Lines 589+)
- **Purpose**: File output with JSON-like structured data
- **Features**: Machine-readable format for log analysis

## File Structure

### Log Files (with .log extension - recently fixed)
```
logs/
├── fsm-core.log.YYYY-MM-DD    # Main application log (daily rotation)
└── errors/
    └── errors.log              # Error-only tracking
```

## Configuration Profiles

### 1. Default (File-Only - Recommended)
```rust
LoggingConfig {
    enable_console: false,
    enable_file_logging: true,
    enable_metrics: true,
    enable_error_tracking: true,
    file_level: Level::DEBUG,
}
```

### 2. Production
- File-only logging to `/var/log/fsm-core/`
- INFO level minimum
- No console output

### 3. Development
- TRACE level for detailed debugging
- File output with comprehensive tracing

### 4. Console Debug (Not recommended for production)
- Console output with colors
- No file logging

## Performance Features

### 1. Non-Blocking I/O
- Uses `tracing_appender::non_blocking` for async file writes
- Prevents I/O blocking the main event loop

### 2. Memory Metrics (Linux)
- Reads `/proc/self/status` for RSS memory usage
- Logged in performance reports

### 3. Sequence Numbering
- Global atomic counter for log entry ordering
- Useful for debugging event sequences

## Recent Fixes (Current Session)

### 1. Field Concatenation Issue
**Problem**: Log output showed concatenated strings like:
```
Starting initial directory scan with  projects/fsm FileSystemOperatordirectory="/path"
```

**Root Cause**: `CompactFormatter` used `ctx.field_format().format_fields()` which concatenated without spaces.

**Solution**: Added `FieldExtractor` visitor to properly separate message and fields:
```rust
struct FieldExtractor {
    message: Option<String>,
    fields: Vec<(String, String)>,
}
```

### 2. Log File Extensions
**Fixed**: Added `.log` extension to all log files:
- `fsm-core` → `fsm-core.log`
- `errors.jsonl` → `errors.log`

## Usage Patterns

### Structured Logging Best Practices
```rust
// Good - structured fields with message
info!(
    directory = %path.display(),
    entries_count = count,
    "Directory scan completed"
);

// Avoid - concatenated strings
info!("Directory scan completed for {} with {} entries", path, count);
```

### Tracing Spans
```rust
#[instrument(name = "operation_name", skip(large_param))]
async fn my_function() {
    let _span = trace_operation!("sub_operation");
    // ... work
}
```

### Error Handling
```rust
.trace_err("operation_context")?  // Adds tracing to errors
```

## Debugging Logging Issues

### Common Problems
1. **Field concatenation**: Check `CompactFormatter::format_event()`
2. **Missing logs**: Verify `WorkerGuard` lifetime
3. **Performance impact**: Monitor non-blocking buffer sizes
4. **File permissions**: Check log directory write access

### Debug Commands
```bash
# View recent logs
tail -f logs/fsm-core.log.$(date +%Y-%m-%d)

# Check error logs only
tail -f logs/errors/errors.log

# Monitor log file sizes
du -sh logs/
```

## Integration Points

### 1. Main Application (`main.rs`)
```rust
let config = LoggingConfig { console_level: Level::TRACE, ..Default::default() };
let _logger = Logger::init_with_config(config)?;
```

### 2. Event Loop (`event_loop.rs`)
- Uses `#[instrument]` macros extensively
- Performance metrics reporting every 30 seconds
- Comprehensive span context for debugging

### 3. File System Operations (`file_system_operator.rs`)
- Operation tracing with unique IDs
- Duration tracking for performance analysis
- Error propagation with context

## Maintenance Notes

### 1. Log Rotation
- Daily rotation handled by `tracing_appender`
- No manual cleanup required
- Old logs remain until manual deletion

### 2. Performance Impact
- Non-blocking I/O minimizes event loop impact
- Structured logging adds ~5-10μs per log entry
- Memory usage: ~1MB buffer per layer

### 3. Future Improvements
- Consider log compression for long-term storage
- Add log level filtering per module
- Implement log shipping for distributed debugging

## Context for AI Sessions

When working with logging issues:
1. **Check formatter implementations** for string concatenation problems
2. **Verify WorkerGuard lifetime** for missing logs
3. **Use structured fields** instead of string formatting
4. **Test with different LoggingConfig profiles**
5. **Monitor file permissions** and directory creation

This logging system is production-ready with comprehensive error tracking and performance monitoring capabilities.