# FSM - AI Context (Sonnet 4)
**FSM**: Rust TUI file manager - ✅ **Phase 4.3 COMPLETE + UI RACE CONDITION FIXED**

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

## Status 🚨 UI CORRUPTION ISSUE - BACKSPACE NAVIGATION BROKEN
- **EventLoop Architecture**: Single EventLoop in background task, main.rs handles rendering
- **Logging System**: Fixed field concatenation, proper spacing, .log extensions
- **StateProvider trait**: Clean abstraction for handler state access
- **ActionDispatcher**: Modular action handling with 6 handlers (no duplication)
- **UIRenderer**: Real-time rendering at 60fps with atomic redraw synchronization
- **Navigation**: Enter directories works, **BACKSPACE BROKEN - UI corruption**
- **FileSystem**: Async directory scanning, entries loaded efficiently
- **Redraw System**: Race-condition-free atomic counter system for UI updates

## Current Critical Issue ❌ BACKSPACE UI CORRUPTION
**Problem**: Backspace navigation corrupts UI display - old file listings remain visible
**Backend Status**: Navigation logic works correctly (logs show proper entry counts)
**UI Issue**: Terminal content not properly cleared during parent directory navigation
**Attempted Fix**: Added explicit area clearing in object_table.rs - FAILED
**Root Cause**: Unknown - needs deeper investigation into ratatui rendering

## Failed Fixes Applied
1. **Frame Skip Logic**: Enhanced ui.rs render conditions - NO EFFECT
2. **Area Clearing**: Added Block::default().bg(BACKGROUND) clear - NO EFFECT  
3. **Redraw Flags**: Improved main content render logic - NO EFFECT
4. **Debug Logging**: Added frame skip tracing - NO EFFECT

## Current Status ❌ BACKSPACE NAVIGATION BROKEN
**Enter Navigation**: Works correctly with proper UI updates
**Backspace Navigation**: UI corruption - stale content remains visible
**Performance**: 60fps maintained, sub-ms backend navigation
**Next Steps**: Deep investigation of ratatui buffer clearing required