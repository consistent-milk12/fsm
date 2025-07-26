# FSM - AI Context Memory (Sonnet 4)
**LOAD FIRST: Token-optimized project context for persistent AI sessions**

## Project Identity
**FSM**: Rust TUI file manager - Extreme performance clipboard complete, modular event loop integration pending  
**Current Phase**: 4.0 In Progress (Event Loop Modular Integration)

## Architecture (Lock-Free MVC + Modular Events)
```rust
StateCoordinator { ui_state: ArcSwap<UIState>, fs_cache: DashMap<PathBuf, DirState> }
EventProcessor -> HandlerRegistry -> ActionBatcher -> StateCoordinator -> UI updates
```

**Performance Baseline**: Sub-millisecond events, 60fps rendering, 10-50x lock contention reduction

## Code Rules (Token-Optimized)
1. **RESTRICTED EDITING** - Claude edits ONLY: CLAUDE.md, Design.md, Implementation.md
2. **ALL OTHER FILES** - Generate terminal diffs/code for manual implementation
3. **SUDO Override** - "SUDO: Update {filename} directly"
4. **BATCH TOOL CALLS** - Parallel reads for efficiency 
5. **MINIMAL OUTPUT** - Concise unless detail requested
6. **Quality Gates** - cargo fmt → check → clippy → build
7. **Performance First** - lock-free, batching, throttling patterns

## AI Optimization Rule (MANDATORY)
**All additions/updates to CLAUDE.md, Design.md, Implementation.md MUST be AI-optimized with minimum tokens for context and memory persistence across clean sessions.**

## Session Continuity (3-File System)
**CLAUDE.md**: AI context/rules (THIS FILE) - loaded first every session  
**Design.md**: Historical archive + ADRs - reference for decisions/completed phases  
**Implementation.md**: Active work spec - current feature roadmap with success criteria  
**Phase 4.0 Details**: [DesignTimeline/Phase_4_0.md](DesignTimeline/Phase_4_0.md) - Architecture reference

## Current Status (Token-Optimized)
**Phase 4.0 In Progress**: Modular components built, main event_loop.rs (2,463 lines) integration pending  
**Critical Gap**: EventProcessor exists but not integrated with monolithic event loop

**Foundation**: Production-ready architecture with extreme performance patterns established  
**Detail**: See DesignTimeline/ for complete phase documentation

## Development Workflow (Claude-Optimized)
**Session Init**: Load CLAUDE.md → batch read Design.md + Implementation.md + DesignTimeline/Phase_4_0.md → create TodoWrite  
**Development**: Batch research → generate terminal diffs → verify → track with todos  
**Phase Complete**: Archive to Design.md → update Implementation.md → sync CLAUDE.md

**Context Management**: Use Task tool for searches, batch parallel reads, minimize tokens  
**Safety**: Edit only CLAUDE.md/Design.md/Implementation.md unless SUDO override

**This enables seamless AI session continuity with optimized performance.**