# FSM - AI Context Memory (Sonnet 4)
**LOAD FIRST: Core project context for persistent AI development sessions**

## Project Identity
- **FSM**: Rust TUI file manager (ratatui+tokio, async MVC, actor model)
- **Status**: Production-ready with async file ops, progress tracking, search, commands
- **Current Phase**: 2.3 (FileOperationsOverlay UI component)

## Architecture (Critical AI Context)
```rust
// Core pattern: Arc<Mutex<AppState>> + mpsc channels
AppState { fs: FSState, ui: UIState, task_tx, action_tx }
EventLoop -> dispatch_action() -> background tasks -> TaskResult -> UI updates
```

**Key Files:**
- `src/controller/event_loop.rs` - Central dispatcher, action handlers
- `src/tasks/file_ops_task.rs` - Background file operations with progress
- `src/model/ui_state.rs` - UI state + active_file_operations HashMap
- `src/controller/actions.rs` - Action enum (Copy/Move/Rename + progress variants)

## Code Rules (MANDATORY)
1. **RESTRICTED FILE EDITING** - AI can ONLY directly edit: CLAUDE.md, Design.md, Implementation.md
2. **ALL OTHER FILES** - Generate terminal diffs/code for user to implement manually  
3. **SUDO Override** - User can override with "SUDO: Update {filename} directly."
4. **Read Access** - AI can read ANY file as needed for analysis and context
5. **Display Only** - AI displays generated diffs/code directly in terminal
6. **cargo fmt → check → clippy → build** after any code change
7. **Modern Rust 2024** - explicit types, structured errors, const values
8. **Performance patterns**: pre-calculated intervals, labeled loops, adaptive algorithms
9. **Quality > speed** - robust error handling, proper lifetimes

## AI Session Continuity Protocol (MANDATORY)
**Professional 3-file system for persistent AI development:**

### CLAUDE.md - AI Context & Rules (THIS FILE)
- **Purpose**: Project identity, rules, workflow for AI sessions
- **AI Usage**: FIRST file loaded in every session for context
- **Updates**: AI can edit directly for workflow improvements
- **Critical**: Contains file editing permissions and development rules

### Design.md - Archive & ADRs 
- **Purpose**: Completed implementations + Architecture Decision Records
- **AI Usage**: Historical context, architectural decisions, lessons learned
- **Updates**: AI adds completed phases + ADRs, never removes content
- **Format**: ADRs → Completed Phases → Technical Foundation → Roadmap

### Implementation.md - Active Work Specification
- **Purpose**: Current feature specification with standardized template
- **AI Usage**: Complete implementation roadmap with success criteria
- **Updates**: AI replaces entirely when phase completes
- **Format**: Executive Summary → Context → Success Criteria → Technical Approach → Code Specs

## Current Implementation Context (Phase 2.3)
**Backend Complete:** Real-time progress tracking via UIState.active_file_operations HashMap
**Missing:** FileOperationsOverlay UI component for progress visualization
**Files:** Create `src/view/components/file_operations_overlay.rs` + integrate with main UI

## Key System Knowledge
- **TaskResult enum**: Legacy + FileOperationComplete + FileOperationProgress variants
- **Progress flow**: file_ops_task → TaskResult → event_loop → UIState → UI rendering
- **Cancellation**: tokio_util::CancellationToken integrated, escape key handling pending
- **Error patterns**: AppError with manual Clone impl, structured error construction
- **Performance**: 64KB BUFFER_SIZE, adaptive progress intervals, hot loop optimization

## Dependencies & Build
```bash
ratatui tokio crossterm tracing moka serde anyhow thiserror ansi-to-tui tokio-util uuid
cargo fmt && cargo check && cargo clippy && cargo build
RUST_LOG=debug cargo run
```

## Features Status
✅ **Async navigation** - incremental loading, background metadata
✅ **File operations** - c/m/r keys, background tasks, progress backend  
✅ **Search** - filename (/) + content (:search), ripgrep ANSI colors
✅ **Commands** - vim-style (:), auto-completion, input prompts
✅ **Progress infrastructure** - real-time tracking, cancellation tokens, state management
⏳ **Progress UI** - FileOperationsOverlay component (Phase 2.3 IN PROGRESS)

## AI Development Workflow (STRICT - CLEAN SESSION PROTOCOL)

### New Session Initialization
1. **MANDATORY**: User loads CLAUDE.md first for AI context
2. **Auto-read**: AI reads Design.md for historical context + ADRs
3. **Auto-read**: AI reads Implementation.md for current active work
4. **Context Load**: AI reads key system files as specified in project context

### Development Cycle (Every Session)
1. **Analyze**: Read current Implementation.md specification  
2. **Research**: Read codebase files for understanding context
3. **Generate**: Create terminal diffs/code showing exact changes
4. **Display**: Show implementation code in terminal for user
5. **Restrict**: NO direct file edits except CLAUDE.md, Design.md, Implementation.md
6. **Override**: SUDO token allows direct edits: "SUDO: Update {filename} directly"
7. **Verify**: Read updated files after user implements changes
8. **Complete**: Check all success criteria, update todos

### Phase Completion Protocol
1. **Archive**: Add completed phase to Design.md with technical details
2. **ADR**: Document major architectural decisions in Design.md  
3. **Reset**: Replace Implementation.md with next phase specification
4. **Update**: Modify CLAUDE.md current phase context
5. **Validate**: Ensure all files synchronized for next session

### Quality Gates
- **Pre-code**: cargo fmt && cargo check && cargo clippy
- **Post-implementation**: cargo build && integration tests
- **Documentation**: All changes reflected in Design.md archive

**This context enables seamless AI session continuity with full project knowledge retention.**