# FSM - AI Context (Sonnet 4)
**FSM**: Rust TUI file manager - ✅ **Phase 4.0 COMPLETE**

## Architecture
```rust
StateCoordinator { current_directory, fs_cache: DashMap, ui_state: ArcSwap }
EventLoop -> StateCoordinator -> UIRenderer -> ratatui
```
**Performance**: 60fps throttling, sub-ms events, lock-free navigation

## Rules
1. **Edit ONLY**: CLAUDE.md, Design.md, Implementation.md  
2. **SUDO Override**: "SUDO: Update {filename}"  
3. **Quality Gates**: cargo build → test navigation  

## Status ✅ COMPLETE + OVERLAY SYSTEM
- **StateCoordinator**: Lock-free directory loading, navigation (up/down/page)
- **ActionDispatcher**: Modular action handling with batching optimization
- **EventLoop**: 60fps throttling, performance monitoring with dispatcher integration
- **UIRenderer**: StateCoordinator integration, **real file display**, UI persistence fixed
- **Navigation**: **Enter directories, parent navigation**, arrow keys, real directory contents
- **FileSystem**: Async directory scanning, ObjectInfo integration
- **Overlay System**: Command mode (:), filename search (/), help (h/?) overlays with input handling

## Latest Updates
**Overlay Implementation**: Complete command mode, search, and help overlay system  
**Input Handling**: Overlay-aware input processing with escape sequences  
**Command Mode**: : key opens command prompt overlay with input field  
**Search Mode**: / key opens filename search overlay with live input  
**Help System**: h/? keys toggle comprehensive help overlay  
**Status**: Full overlay system working with proper input handling and UI state management

## Next Development  
**Current**: Feature-complete file manager with overlays and navigation  
**Future**: Command parsing and execution, file operations (copy/move/delete), advanced search functionality