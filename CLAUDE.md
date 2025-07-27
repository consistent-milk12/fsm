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

## Status ✅ COMPLETE + FIXED
- **StateCoordinator**: Lock-free directory loading, navigation (up/down/page)
- **EventLoop**: 60fps throttling, performance monitoring  
- **UIRenderer**: StateCoordinator integration, real file display, **UI persistence fixed**
- **Navigation**: Arrow keys working, directory cache functional, **UI stays visible**

## UI Fix Applied
**Issue**: UI rendered briefly then disappeared due to early return optimization  
**Fix**: Disabled render optimizations in `ui.rs:58` and `ui.rs:105` - UI now persists  
**Status**: Navigation working, press `q` to quit

## Next Development
**Current**: Working file manager with persistent UI and real navigation  
**Future**: Enter directories, parent navigation, file operations