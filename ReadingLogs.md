# FSM-Core Log Analysis Guide

## Overview
FSM-Core generates comprehensive structured logs for debugging, performance monitoring, and system analysis. This guide provides professional techniques for analyzing the generated log files using modern tools like `ripgrep` (rg), `jq`, and other advanced CLI utilities.

## Log File Location
- **Main Log**: `logs/fsm-core.log`
- **Error Log**: `logs/errors/errors.log` (JSON format)

## Log Format
```
HH:MM:SS [NNNN] LEVEL [span_name] module_path (file:line) message key=value key2=value2
```

Example:
```
03:29:50 [0001] INFO  fsm_core::logging (fsm-core/src/logging.rs:149) FSM-Core logging system initialized version="1.0.0" guards_count=1
```

## Essential Analysis Commands (Using ripgrep)

### 1. Performance Analysis

**Frame Rendering Performance:**
```bash
# Check render times and identify slow frames (ripgrep with regex)
rg 'render_time_us=(\d+)' logs/fsm-core.log -o -r '$1' | sort -n | tail -10

# Average render time with ripgrep and awk
rg 'render_time_us=(\d+)' logs/fsm-core.log -o -r '$1' | awk '{sum+=$1; count++} END {print "Average:", sum/count "µs"}'

# Total frame count
rg -c "Frame rendered" logs/fsm-core.log

# Slow frames (>16ms for <60fps)
rg 'render_time_us=(\d+)' logs/fsm-core.log --only-matching --replace '$1' | awk '$1 > 16000 {print $1 "µs - SLOW FRAME"}'
```

**Action Dispatch Performance:**
```bash
# Slowest action dispatches with context
rg 'execution_time=([0-9.]+[µm]s)' logs/fsm-core.log -A1 -B1 -o -r '$1' | sort -V | tail -10

# Action type frequency analysis
rg 'action=(\w+)' logs/fsm-core.log -o -r '$1' | sort | uniq -c | sort -nr

# Dispatch time distribution
rg 'execution_time=(\d+)\.?\d*µs' logs/fsm-core.log -o -r '$1' | \
  awk '{
    if($1<100) fast++; 
    else if($1<1000) med++;
    else slow++
  } END {
    print "Fast (<100µs):", fast;
    print "Medium (100-1000µs):", med;
    print "Slow (>1000µs):", slow
  }'
```

### 2. UI Component Analysis

**File Table Rendering:**
```bash
# Track entry count changes with timestamps (directory navigation)
rg 'Rendering file table with (\d+) entries' logs/fsm-core.log -r 'Entries: $1' --no-line-number

# File table render performance with entry count correlation
rg 'fsm_core::view::object_table.*render_time_us=(\d+).*entries_count=(\d+)' logs/fsm-core.log -o -r '$1,$2' | \
  awk -F',' '{
    sum+=$1; count++; 
    if($1>10000) slow++; 
    if($2>1000) large++
  } END {
    print "File Table Performance:";
    print "  Average render time:", int(sum/count) "µs";
    print "  Slow renders (>10ms):", slow+0;
    print "  Large directories (>1000 entries):", large+0;
    print "  Total renders:", count
  }'

# File system composition analysis
rg 'fsm_core::view::object_table.*dirs_count=(\d+).*files_count=(\d+).*symlinks_count=(\d+)' logs/fsm-core.log -o -r '$1,$2,$3' | \
  awk -F',' '{
    dirs+=$1; files+=$2; symlinks+=$3; count++
  } END {
    printf "File System Composition:\n";
    printf "  Average directories: %.1f\n", dirs/count;
    printf "  Average files: %.1f\n", files/count; 
    printf "  Average symlinks: %.1f\n", symlinks/count;
    printf "  Samples: %d\n", count
  }'

# UI area dimensions tracking with frequency
rg 'Table area: (\d+x\d+)' logs/fsm-core.log -o -r '$1' | sort | uniq -c

# Navigation patterns analysis
rg 'file table with (\d+) entries' logs/fsm-core.log -o -r '$1' | \
  awk '{
    print NR, $1; 
    if(NR>1 && $1!=prev) changes++; 
    prev=$1
  } END {print "Directory changes:", changes}'

# Selection boundary validation monitoring
rg 'fsm_core::view::object_table.*Selection index out of bounds' logs/fsm-core.log -A1 -B1 --color=always
```

**Overlay Activity:**
```bash
# Search overlay lifecycle with context
rg 'search overlay' logs/fsm-core.log -A2 -B1 --color=always

# Modal overlay state transitions
rg '(Creating|Rendering).*overlay' logs/fsm-core.log --color=always

# Overlay type distribution
rg 'overlay.*(\w+Search|\w+Overlay)' logs/fsm-core.log -o | sort | uniq -c

# Loading overlay performance analysis
rg 'fsm_core::view::loading_overlay.*render_time_us=(\d+)' logs/fsm-core.log -o -r '$1' | \
  awk '{sum+=$1; count++; if($1>5000) slow++} END {
    print "Loading Overlay Performance:";
    print "  Slow renders (>5ms):", slow+0;
    print "  Total renders:", count
  }'

# Notification overlay activity with severity breakdown
rg 'fsm_core::view::notification_overlay.*level = (\w+)' logs/fsm-core.log -o -r '$1' | \
  sort | uniq -c | sort -nr

# Notification overlay render performance
rg 'fsm_core::view::notification_overlay.*render_time_us=(\d+)' logs/fsm-core.log -o -r '$1' | \
  awk '{sum+=$1; count++; if($1>3000) slow++} END {
    printf "Notification Overlay Performance:\n";
    printf "  Slow renders (>3ms): %d\n", slow+0;
    printf "  Total renders: %d\n", count
  }'
```

### 3. System Health Monitoring

**Memory and State Health:**
```bash
# Critical: Check for poisoned states (system integrity)
rg -i 'poison|corrupt|deadlock' logs/fsm-core.log --color=always -A3 -B1

# Lock contention and blocking operations
rg -i '(lock|mutex|rwlock|blocking|waiting)' logs/fsm-core.log -C2

# Error and warning patterns with severity classification
rg '(ERROR|WARN)' logs/fsm-core.log --color=always | \
  awk '{
    if(/ERROR/) errors++; 
    if(/WARN/) warnings++
  } END {
    print "Errors:", errors, "Warnings:", warnings
  }'
```

**Event Loop Health:**
```bash
# Event processing throughput analysis
rg 'total_actions=(\d+)' logs/fsm-core.log -o -r '$1' | \
  awk '{print NR, $1; if(NR>1) print "Rate:", ($1-prev)"/interval"; prev=$1}'

# Task completion success rate
rg '(Task.*completed|Task.*failed)' logs/fsm-core.log | \
  awk '/completed/{success++} /failed/{failed++} END{print "Success:", success, "Failed:", failed}'

# Event loop performance metrics
rg 'Event loop.*(\d+)ms' logs/fsm-core.log -o --color=always
```

### 4. Navigation and File Operations

**Directory Navigation Tracking:**
```bash
# CWD transition timeline with ripgrep
rg 'cwd="([^"]*)"' logs/fsm-core.log -o -r '$1' | nl -w3 -s': '

# Directory scan performance analysis
rg 'duration_ms=(\d+)' logs/fsm-core.log -o -r '$1' | \
  awk '{
    sum+=$1; count++; 
    if($1>100) slow++
  } END {
    print "Avg scan time:", sum/count "ms";
    print "Slow scans (>100ms):", slow
  }'

# File system operation success vs failure rates
rg '(scan.*completed|scan.*failed|operation.*completed|operation.*failed)' logs/fsm-core.log | \
  awk '/completed/{success++} /failed/{failed++} END{printf "Success rate: %.1f%%\n", success*100/(success+failed)}'
```

**File System Operations:**
```bash
# Operations timeline with duration context
rg '(Starting|completed).*(?:scan|operation).*duration_ms=(\d+)' logs/fsm-core.log -A1 -B1

# Entry loading performance tracking
rg 'entries_count=(\d+).*execution_time=([0-9.]+)ms' logs/fsm-core.log -r 'Loaded $1 entries in $2ms' --no-line-number

# Directory content size distribution
rg 'entries_count=(\d+)' logs/fsm-core.log -o -r '$1' | \
  awk '{
    if($1<10) small++; 
    else if($1<100) medium++; 
    else large++
  } END {
    print "Small dirs (<10):", small;
    print "Medium dirs (10-100):", medium; 
    print "Large dirs (>100):", large
  }'
```

## Advanced Analysis Techniques (ripgrep-powered)

### 1. Timeline Analysis
```bash
# Application lifecycle timeline with precise timing
rg '(initialization|startup|Event loop|directory scan)' logs/fsm-core.log --color=always -n | head -20

# Startup performance breakdown
rg 'completed.*duration_ms=(\d+)' logs/fsm-core.log -r 'Duration: $1ms' | head -10

# Session timeline markers
rg '^(\d{2}:\d{2}:\d{2}).*(?:Starting|completed|initialized)' logs/fsm-core.log --color=always
```

### 2. Performance Bottleneck Detection
```bash
# Critical bottlenecks: operations >1ms with full context
rg 'duration_ms=(\d+)' logs/fsm-core.log | awk -F'duration_ms=' '$2+0 > 1 {print}' | head -20

# Frame rate analysis: identify <60fps periods
rg 'render_time_us=(\d+)' logs/fsm-core.log -o -r '$1' | \
  awk '$1 > 16000 {slow++} $1 <= 16000 {fast++} END {
    total = slow + fast;
    printf "Frame rate analysis:\n";
    printf "Fast frames (≥60fps): %d (%.1f%%)\n", fast, fast*100/total;
    printf "Slow frames (<60fps): %d (%.1f%%)\n", slow, slow*100/total
  }'

# Resource utilization hotspots
rg '(execution_time|duration|render_time)=(\d+)' logs/fsm-core.log -o | \
  sort -t'=' -k2 -n | tail -15
```

### 3. State Consistency Verification
```bash
# Navigation state consistency audit
rg '(Rendering file table|entries_count=)' logs/fsm-core.log -A1 -B1 --color=always

# Action-result correlation analysis
rg '(MoveSelection|EnterSelected|GoToParent)' logs/fsm-core.log -A3 -B1 --color=always

# State transition validation
rg 'cwd=.*entries.*selected=' logs/fsm-core.log --color=always

# UI state synchronization check
rg '(UI snapshot|Creating.*snapshot)' logs/fsm-core.log -C2
```

### 4. Component Lifecycle Tracking
```bash
# Component initialization sequence
rg 'Creating new (\w+)' logs/fsm-core.log -r 'Component: $1' --no-line-number | nl

# Component interaction matrix
rg '(UIRenderer|StatusBar|FileTable|SearchOverlay)' logs/fsm-core.log --color=always | head -15

# Resource allocation patterns
rg '(initialized|created|allocated).*(\d+)' logs/fsm-core.log -i --color=always

# Component teardown and cleanup
rg '(cleanup|shutdown|dropping|destroying)' logs/fsm-core.log -i -C2
```

## Log Level Control

Set different verbosity levels using `RUST_LOG`:

```bash
# Production monitoring (INFO and above)
RUST_LOG=info ./target/debug/fsm-core

# Development debugging (DEBUG and above)  
RUST_LOG=debug ./target/debug/fsm-core

# Deep analysis (all logs including TRACE)
RUST_LOG=trace ./target/debug/fsm-core

# Component-specific tracing
RUST_LOG="fsm_core::view=trace,fsm_core::controller=debug" ./target/debug/fsm-core
```

## Common Patterns to Monitor

### 🟢 Healthy Patterns
- Consistent render times <16ms (60fps)
- Action dispatch times <1ms
- Directory scans <10ms
- No poisoned state warnings
- Steady entry count changes during navigation

### 🟡 Warning Patterns
- Render times 16-33ms (30-60fps)
- Action dispatch >1ms but <10ms
- Directory scans 10-100ms
- Occasional lock timeouts

### 🔴 Critical Patterns
- Render times >33ms (<30fps)
- Action dispatch >10ms
- Directory scans >100ms
- Poisoned state errors
- Lock deadlocks or panics

## Error Investigation (Advanced JSON Analysis)

**JSON Error Logs with jq:**
```bash
# Structured error analysis with jq
jq '.' logs/errors/errors.log 2>/dev/null | head -20

# Error classification and frequency
jq -r '.level' logs/errors/errors.log 2>/dev/null | sort | uniq -c

# Critical errors with full context
jq 'select(.level == "ERROR") | {timestamp, target, spans, fields}' logs/errors/errors.log 2>/dev/null

# Error timeline analysis
jq -r '[.timestamp, .level, .target] | @tsv' logs/errors/errors.log 2>/dev/null | sort

# Span hierarchy analysis for debugging
jq '.spans[]? | {name, target, file, line}' logs/errors/errors.log 2>/dev/null | head -10
```

**Advanced Error Pattern Analysis:**
```bash
# Error clustering by module
rg -i 'error|exception|panic|fail' logs/fsm-core.log --color=always -C2 | \
  rg '::(\w+)' -o | sort | uniq -c | sort -nr

# Critical error sequences (cascading failures)
rg -i 'error.*error|fail.*fail' logs/fsm-core.log -A5 -B5 --color=always
```

## Performance Baseline Establishment (ripgrep-powered)

Create comprehensive performance baselines:

```bash
# Automated baseline extraction script
#!/bin/bash
echo "=== FSM-CORE PERFORMANCE BASELINE ==="
echo "Timestamp: $(date)"
echo

# Render performance
render_avg=$(rg 'render_time_us=(\d+)' logs/fsm-core.log -o -r '$1' | awk '{sum+=$1; count++} END {printf "%.1f", sum/count}')
render_count=$(rg -c 'Frame rendered' logs/fsm-core.log)
echo "Render Performance:"
echo "  Average frame time: ${render_avg}µs"
echo "  Total frames: $render_count"
echo "  Target: <16,667µs (60fps)"

# Action dispatch performance
action_avg=$(rg 'execution_time=(\d+\.?\d*)µs' logs/fsm-core.log -o -r '$1' | awk '{sum+=$1; count++} END {printf "%.1f", sum/count}')
action_count=$(rg -c 'Starting action dispatch' logs/fsm-core.log)
echo
echo "Action Dispatch Performance:"
echo "  Average dispatch time: ${action_avg}µs"
echo "  Total actions: $action_count"
echo "  Target: <1,000µs"

# Directory operations
scan_avg=$(rg 'duration_ms=(\d+)' logs/fsm-core.log -o -r '$1' | awk '{sum+=$1; count++} END {printf "%.1f", sum/count}')
scan_count=$(rg -c 'directory scan completed' logs/fsm-core.log)
echo
echo "Directory Operations:"
echo "  Average scan time: ${scan_avg}ms"
echo "  Total scans: $scan_count"
echo "  Target: <100ms"

# System health indicators
error_count=$(rg -c -i 'error' logs/fsm-core.log)
warn_count=$(rg -c -i 'warn' logs/fsm-core.log)
echo
echo "System Health:"
echo "  Errors: $error_count"
echo "  Warnings: $warn_count"
echo "  Target: 0 errors, minimal warnings"
```

## Integration with Monitoring Tools

**Production Monitoring with ripgrep:**
```bash
# Real-time log monitoring with ripgrep
rg --follow --color=always '(ERROR|WARN|CRITICAL)' logs/fsm-core.log

# Performance alerting thresholds
rg 'render_time_us=(\d+)' logs/fsm-core.log -o -r '$1' | \
  awk '$1 > 33000 {print "ALERT: Slow frame detected:", $1"µs"}'

# Automated health checks
rg -i 'poison|deadlock|panic' logs/fsm-core.log && echo "CRITICAL: System integrity issue detected"
```

**Development Workflow:**
```bash
# Live performance dashboard
watch -n 1 'echo "=== LIVE FSM METRICS ==="; 
  echo "Current FPS: $(rg "render_time_us=(\d+)" logs/fsm-core.log -o -r "$1" | tail -10 | awk "{sum+=\$1; count++} END {printf \"%.1f\", 60000000/(sum/count)}")";
  echo "Latest errors: $(rg -c ERROR logs/fsm-core.log)";
  echo "Memory warnings: $(rg -c -i "memory|alloc" logs/fsm-core.log)"'

# Development debugging session
rg --follow --color=always -A2 -B1 '(Creating|Rendering|dispatch)' logs/fsm-core.log | head -50
```

**Advanced Integration Examples:**
```bash
# Export to Prometheus format
rg 'render_time_us=(\d+)' logs/fsm-core.log -o -r '$1' | \
  awk '{print "fsm_render_time_microseconds", $1, systime()}' > metrics.prom

# Log parsing for Grafana
rg '(\d{2}:\d{2}:\d{2}).*render_time_us=(\d+)' logs/fsm-core.log -r '$1,$2' | \
  awk -F',' '{print $1, $2}' > render_times.csv
```

## Quick Diagnostic Commands

**One-liner System Health Check:**
```bash
# Comprehensive health summary
echo "=== FSM HEALTH CHECK ==="; \
echo "Errors: $(rg -c ERROR logs/fsm-core.log)"; \
echo "Avg Frame Time: $(rg 'render_time_us=(\d+)' logs/fsm-core.log -o -r '$1' | awk '{sum+=$1; count++} END {printf "%.0fµs", sum/count}')"; \
echo "Slow Frames: $(rg 'render_time_us=(\d+)' logs/fsm-core.log -o -r '$1' | awk '$1>16000{count++} END{print count+0}')"; \
echo "Navigation Events: $(rg -c 'MoveSelection|EnterSelected' logs/fsm-core.log)"; \
echo "Directory Scans: $(rg -c 'directory scan completed' logs/fsm-core.log)"
```

**Performance Regression Detection:**
```bash
# Compare current session vs baseline
current_avg=$(rg 'render_time_us=(\d+)' logs/fsm-core.log -o -r '$1' | tail -100 | awk '{sum+=$1; count++} END {print sum/count}')
baseline=12000  # Set your baseline
echo "Current avg: ${current_avg}µs, Baseline: ${baseline}µs"
awk -v current="$current_avg" -v baseline="$baseline" 'BEGIN {
  if(current > baseline * 1.2) print "⚠️  REGRESSION: Performance degraded by", int((current-baseline)*100/baseline)"%"
  else print "✅ Performance within acceptable range"
}'
```

## Troubleshooting Decision Tree

**1. System Won't Start:**
```bash
rg '(initialization|startup).*(?:error|fail)' logs/fsm-core.log -i -A5 -B2
```

**2. Poor Performance:**
```bash
# Identify bottleneck
rg '(render_time_us|execution_time|duration_ms)=(\d+)' logs/fsm-core.log -o | sort -t'=' -k2 -n | tail -10
```

**3. Navigation Issues:**
```bash
# Debug navigation flow  
rg '(GoToParent|EnterSelected|directory scan|entries_count)' logs/fsm-core.log -A2 -B1 --color=always
```

**4. UI Glitches:**
```bash
# Component lifecycle issues
rg '(Creating|Rendering).*(?:UI|Table|Status|Overlay)' logs/fsm-core.log -C2
```

**5. Memory/State Issues:**
```bash
# Critical system state problems
rg -i '(poison|corrupt|deadlock|mutex.*fail)' logs/fsm-core.log -A5 -B2 --color=always
```

**6. Crashes/Panics:**
```bash
# Emergency diagnostics
rg -i '(panic|abort|segfault|signal|crash)' logs/fsm-core.log -A10 -B5 --color=always
jq 'select(.level == "ERROR")' logs/errors/errors.log 2>/dev/null | tail -5
```

---

## ripgrep Quick Reference

**Essential ripgrep flags for log analysis:**
- `-o, --only-matching`: Show only matched text
- `-r, --replace`: Replace matches with template
- `-A, --after-context`: Show N lines after match
- `-B, --before-context`: Show N lines before match  
- `-C, --context`: Show N lines before and after
- `-c, --count`: Count matches instead of showing them
- `-i, --ignore-case`: Case insensitive search
- `-n, --line-number`: Show line numbers
- `--color=always`: Force color output
- `--follow`: Follow file like `tail -f`

## Race Condition Diagnosis (UI Rendering Issues)

**Navigation UI Update Problems:**
```bash
# Trace redraw request/processing race conditions
rg 'request_redraw.*flag=(All|Main)' logs/fsm-core.log -A5 -B2 --color=always

# Check needs_redraw timing vs clear_redraw cycles  
rg 'needs_redraw.*needs=(true|false)' logs/fsm-core.log -C2 | head -20

# Navigation action correlation with UI updates
rg -A10 -B5 'dispatch_action.*GoToParent' logs/fsm-core.log

# Find render cycle timing issues
rg 'clear_redraw|request_redraw' logs/fsm-core.log -C1 | head -30

# Atomic counter synchronization verification
rg 'pending_requests=(\d+)' logs/fsm-core.log -o -r '$1' | awk '$1>0{print "Pending:", $1}'
```

**60fps Render Loop vs Action Timing:**
```bash
# Show race between render clearing and action requests
rg '(clear_redraw|request_redraw|needs_redraw.*false)' logs/fsm-core.log -n | head -20

# Navigation timing analysis
rg 'navigation.*completed.*entries_count=(\d+)' logs/fsm-core.log -A3 -B1 -r 'Nav: $1 entries'

# Render flag state transitions
rg '(requesting redraw|clearing redraw flags|needs_redraw)' logs/fsm-core.log --color=always
```

This advanced ripgrep-based approach provides superior performance and flexibility compared to traditional grep-based log analysis, enabling professional-grade debugging and monitoring of FSM-Core applications.