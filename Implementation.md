# Implementation Specification

**ACTIVE:** Phase 4.0 - Event Loop Modular Integration

## Executive Summary
**Objective:** Complete modular event loop refactor by integrating existing components with main event_loop.rs  
**Priority:** Critical (foundational architecture for all future features)  
**Status:** 🚧 Foundation built, integration work required

## Context & Background
**Current State:** Modular components exist but event_loop.rs (2,463 lines) still monolithic  
**Architecture Gap:** EventProcessor exists but not integrated with main event loop  
**Performance Goal:** Achieve documented 10-50x lock contention reduction and sub-millisecond response

## Success Criteria
### P0 (Must Have)
- [ ] **Integrate EventProcessor**: Replace monolithic event loop with modular EventProcessor
- [x] **Handler Integration**: Wire existing handlers (navigation, clipboard, search, etc.) - **COMPLETE**
- [ ] **State Migration**: Move from Arc<Mutex<AppState>> to ArcSwap/DashMap pattern
- [ ] **Performance Validation**: Achieve documented 10-50x improvement targets

### P1 (Should Have)  
- [ ] **Action Batching**: Implement ActionBatcher for operation optimization
- [ ] **Render Throttling**: Add RenderThrottler for consistent 60fps
- [ ] **Metrics Integration**: Add PerformanceMonitor for regression detection

### ✅ **Completed Components**
- **NavigationHandler**: Arrow keys, vim nav, page controls (Priority: 10)
- **ClipboardHandler**: Copy/cut/paste with overlay navigation (Priority: 1-5) 
- **SearchHandler**: File/content search, command mode (Priority: 5-100)
- **FileOpsHandler**: Create, delete, rename, copy/move operations (Priority: 3-50)
- **KeyboardHandler**: True fallback with emergency functions (Priority: 1-255)
- **HandlerRegistry**: Conflict-free handler registration and priority management

## Technical Approach
**Current Architecture:** Monolithic event_loop.rs (2,463 lines) with traditional mutex-based state  
**Target Architecture:** EventProcessor → HandlerRegistry → ActionBatcher → StateCoordinator  
**Migration Strategy:** Incremental replacement preserving existing functionality  
**Risk Mitigation:** Comprehensive testing at each integration step

## Key Implementation Areas
1. **EventProcessor Integration**: Replace main event loop with modular event processor
2. **Handler Registration**: Migrate existing event handling to handler system
3. **State Refactor**: Replace AppState with StateCoordinator lock-free architecture  
4. **Performance Optimization**: Add batching, throttling, and metrics systems

**Current Status:** Foundation components exist but not integrated  
**Integration Points:** Main event_loop.rs, AppState, UIState, existing event handlers

## Design Reference
**Architecture Documentation:** [Phase 4.0 Details](DesignTimeline/Phase_4_0.md)  
**Historical Context:** [Design Archive](Design.md) + [ADRs](DesignTimeline/ADRs.md)  
**AI Context:** [CLAUDE.md](CLAUDE.md) - Load first for session continuity