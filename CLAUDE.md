# FSM - AI Context (Sonnet 4)
**FSM**: Rust TUI file manager - ✅ **Phase 4.1 COMPLETE + CIRCULAR DEPENDENCY FIX**

## Architecture ✅ FIXED
```rust
StateProvider trait -> StateCoordinator (impl StateProvider)
HandlerRegistry { Arc<dyn StateProvider> } -> Event processing
main.rs: StateCoordinator -> HandlerRegistry (dependency inversion)
```
**Performance**: 60fps throttling, sub-ms events, lock-free navigation

## Rules
1. **Edit ONLY**: CLAUDE.md, Design.md, Implementation.md  
2. **SUDO Override**: "SUDO: Update {filename}"  
3. **Quality Gates**: cargo build → test navigation  

## Status ✅ COMPLETE + CIRCULAR DEPENDENCY RESOLVED
- **StateProvider trait**: Clean abstraction for handler state access
- **HandlerRegistry**: Uses StateProvider trait, no circular dependencies
- **StateCoordinator**: Implements StateProvider, removed HandlerRegistry coupling
- **Event Processing**: Terminal events -> HandlerRegistry -> Actions
- **ActionDispatcher**: Modular action handling with batching optimization
- **UIRenderer**: Real file display, lock-free UI state updates
- **Navigation**: Enter directories, parent navigation, arrow keys
- **FileSystem**: Async directory scanning, ObjectInfo integration

## Latest Updates ✅ ARCHITECTURAL FIX
**Circular Dependency Resolution**: StateProvider trait breaks StateCoordinator<->HandlerRegistry cycles  
**Clean Architecture**: Dependency inversion pattern, testable components  
**Event Processing**: Terminal input now flows through HandlerRegistry properly  
**Input Handling**: Removed inline event processing, delegated to handlers  
**Build Status**: Clean compilation, no circular references  

## Next Development  
**Current**: Clean architecture with proper event handling through HandlerRegistry  
**Future**: Handler implementations for overlays, command parsing, file operations