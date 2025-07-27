# FSM Design Archive

## ✅ Phase 4.1 COMPLETE + OVERLAY SYSTEM (Full UI Interactions)
**Performance**: 60fps, sub-ms events, lock-free StateCoordinator + ActionBatcher optimization  
**Architecture**: EventLoop -> ActionDispatcher -> StateCoordinator -> UIRenderer -> Overlays  
**Navigation**: Arrow keys, page nav, **Enter/Backspace directory navigation**, real file display  
**Overlays**: Command mode, filename search, help system with input handling

## Key Mappings 
**Navigation**: `↑↓` selection, `PgUp/PgDn` page, `Enter` into dir, `Backspace` parent, `q` quit  
**Overlays**: `:` command mode, `/` filename search, `h/?` help, `Esc` close overlay  
**Future**: `Tab` clipboard, `cxv` operations, `Ctrl+F` content search

## Build
```bash
cargo build --workspace  
RUST_LOG=debug cargo run -p fsm-core --bin fs
```

## Architecture Achievements
- ✅ **ActionDispatcher**: Modular action handling with ActionBatcher optimization
- ✅ **Directory Navigation**: Enter dirs, parent navigation, async file loading
- ✅ **Lock-free**: ArcSwap UIState, atomic selections, DashMap cache
- ✅ **Performance**: 60fps throttling, action batching, sub-ms response
- ✅ **Clean Separation**: main.rs orchestration, ActionDispatcher business logic
- ✅ **Overlay System**: Command mode, filename search, help overlays with input handling
- ✅ **Input Management**: Overlay-aware input processing, escape handling, state transitions

## Next: Command Parsing + File Operations