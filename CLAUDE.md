# FSM - AI Context (Sonnet 4)
**FSM**: Rust TUI file manager - ✅ **Phase 4.9 COMPLETE + System Monitor UI Integration**

## Architecture ✅ PRODUCTION READY
```rust
main.rs: EventLoop(background) + RenderLoop(60fps) -> Clean separation
StateProvider trait -> StateCoordinator (impl StateProvider)
ActionDispatcher { Arc<dyn StateProvider> } -> Event processing
EventLoop -> ActionDispatcher -> StateCoordinator -> UIRenderer
```
**Performance**: 60fps rendering, sub-ms events, single EventLoop, race-condition-free UI updates

## Rules
1. **Edit ONLY**: CLAUDE.md, Design.md, Implementation.md  
2. **SUDO Override**: "SUDO: Update {filename}"  
3. **Quality Gates**: cargo build → test navigation → check logs
4. **STRICT Token Efficiency**: Be extremely precise and compact about input/output tokens
5. **NEVER cargo run**: Ask user to run and test app when needed

## Status ✅ PHASE 4.9 COMPLETE - System Monitor UI Integration
- **System Monitor UI**: Complete `OptimizedSystemMonitorOverlay` with real-time metrics display
- **UI Integration**: Fully integrated into main UIRenderer with proper state management
- **Performance Optimized**: 2-second update intervals, cached process formatting, top 50 processes
- **Interactive Features**: Process selection, color-coded metrics, keyboard navigation support
- **State Management**: Added `show_system_monitor`, `show_process_monitor`, `selected_process_idx` to UiSnapshot
- **Visual Design**: CPU/Memory/Swap gauges with threshold-based colors, formatted byte display
- **SystemHandler**: 7 handlers including SystemHandler with proper ActionMatcher trait
- **Compilation**: All sysinfo compatibility issues resolved, `cargo check` passes
- **EventLoop Architecture**: Single EventLoop in background task, main.rs handles rendering
- **TSV Logging System**: AI-optimized structured logging (6x token efficiency vs JSON)
- **StateProvider trait**: Clean abstraction for handler state access
- **UIRenderer**: Real-time rendering at 60fps with atomic redraw synchronization
- **Navigation**: Both Enter/Backspace work correctly with proper entry loading
- **FileSystem**: Async directory scanning, entries loaded efficiently
- **Redraw System**: Race-condition-free atomic counter system for UI updates

## Backspace Navigation Fix ✅ RESOLVED (2025-07-29)
**Problem**: UI corruption during backspace - old entries remained visible
**Root Cause**: `navigate_to_parent()` kept stale child entries instead of loading parent entries
**Solution**: Modified `fs_dispatcher.rs:205-227` to always reload parent directory:
```rust
// Always reload parent directory entries (never use cache)
let entries: Vec<ObjectInfo> = self.load_directory(&path).await?;
```
**Verification**: Entry count now correct (19 parent entries vs 2 stale child entries)
**Status**: Build successful, proper entry restoration implemented

## TSV Tracing System ✅ IMPLEMENTED (2025-07-29)

### Architecture
- **Primary**: `logs/fsm-core.tsv` - AI-optimized structured data (14 columns)
- **Schema**: `timestamp level target marker operation_type current_path target_path entries_count selected_index duration_us cache_hit area_width area_height message`
- **Token Efficiency**: 6x more efficient than JSON for AI analysis (~50 vs ~300 tokens/event)

### Marker Standards
```rust
// Navigation: ENTER_START|ENTER_COMPLETE|BACKSPACE_START|BACKSPACE_COMPLETE
// State: STATE_BEFORE|STATE_AFTER  
// UI: UI_RENDER_START|UI_RENDER_COMPLETE|UI_CLEAR
// Performance: PERF_DIRECTORY_SCAN|PERF_FRAME_RENDER
```

### TSV Analysis Commands
```bash
# Compare Enter vs Backspace flows (columns 1,4,6,8)
awk -F'\t' '$4 ~ /^(ENTER_|BACKSPACE_)/ {print $1, $4, $6, $8}' logs/fsm-core.tsv

# Performance analysis - operations over 1ms (column 10)
awk -F'\t' '$10 != "NULL" && $10 > 1000 {print $5, $10, $1}' logs/fsm-core.tsv

# Entry count progression tracking
awk -F'\t' '$8 != "NULL" {
    if(prev != "" && $8 != prev) changes++;
    prev = $8; print NR": " $1 " " $4 " " $8
} END {print "Directory changes:", changes}' logs/fsm-core.tsv
```

**Reference**: See `TRACING.md` for complete TSV logging methodology and AI analysis patterns.

## System Monitor UI Integration ✅ COMPLETED (2025-07-29)
**Achievement**: Complete system monitor overlay implementation and UI integration
**Components Implemented**:
1. **OptimizedSystemMonitorOverlay**: Full-featured system monitor with caching and performance optimization
2. **Real-time Metrics**: CPU/Memory/Swap gauges with color-coded thresholds (green/yellow/red)
3. **Process Management**: Interactive process list (top 50 by CPU) with selection and color-coded status
4. **UI Architecture**: Proper integration into UIRenderer overlay system with AppState access
5. **State Management**: Extended UiSnapshot with system monitoring fields and proper synchronization
6. **Performance**: 2-second update intervals, cached formatting, human-readable byte display
7. **Responsive Design**: Graceful handling of missing system data with informative placeholders

**Technical Integration**:
- **Module Export**: Added to lib.rs component exports for proper visibility
- **State Synchronization**: UiSnapshot extended with `show_system_monitor`, `show_process_monitor`, `selected_process_idx`  
- **Rendering Pipeline**: Integrated into main overlay rendering system with proper lock management
- **Memory Efficient**: Process list limited to top 50, cached formatting, minimal allocations
**Status**: Full UI integration complete, `cargo check` passes, ready for live system monitoring testing