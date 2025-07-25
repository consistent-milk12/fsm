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

## Code Rules (CLAUDE-OPTIMIZED)
1. **RESTRICTED FILE EDITING** - Claude can ONLY directly edit: CLAUDE.md, Design.md, Implementation.md
2. **ALL OTHER FILES** - Generate terminal diffs/code for user to implement manually  
3. **SUDO Override** - User can override with "SUDO: Update {filename} directly."
4. **BATCH TOOL CALLS** - Claude MUST use parallel tool calls for efficiency (Read multiple files simultaneously)
5. **CONTEXT WINDOW OPTIMIZATION** - Use TodoWrite for complex tasks, offload searches to Task tool
6. **MINIMAL OUTPUT** - Concise responses unless detail requested (Claude's strength)
7. **COMPREHENSIVE ANALYSIS** - Use Claude's reasoning for edge cases and error scenarios
8. **cargo fmt → check → clippy → build** after any code change
9. **Modern Rust 2024** - explicit types, structured errors, const values
10. **Performance patterns** - pre-calculated intervals, labeled loops, adaptive algorithms
11. **Quality > speed** - robust error handling, proper lifetimes

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

## Current Implementation Context (Phase 2.4)
**Phase 2.3 Complete:** FileOperationsOverlay UI component with real-time progress visualization
**Next:** ESC key cancellation + operation cleanup for user-initiated cancellation
**Files:** Update event_loop.rs key handling + implement cancellation token cleanup

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
✅ **Progress UI** - FileOperationsOverlay component with real-time metrics
⏳ **ESC Cancellation** - User-initiated operation cancellation (Phase 2.4 IN PROGRESS)

## AI Development Workflow (STRICT - CLEAN SESSION PROTOCOL)

### New Session Initialization (Claude-Optimized)
1. **MANDATORY**: User loads CLAUDE.md first for Claude context
2. **BATCH READ**: Claude reads Design.md + Implementation.md simultaneously (parallel tool calls)
3. **SMART CONTEXT**: Claude analyzes current phase and reads only relevant system files
4. **PROACTIVE TODO**: Claude creates TodoWrite for complex multi-step tasks immediately
5. **CONTEXT SUMMARY**: Claude provides 2-line summary of current state for user confirmation

### Development Cycle (Claude-Optimized)
1. **SMART ANALYSIS**: Claude reads Implementation.md + uses reasoning to identify key dependencies
2. **BATCH RESEARCH**: Claude reads multiple relevant files simultaneously (parallel tool calls)
3. **STRUCTURED GENERATION**: Claude creates terminal diffs with clear change explanations
4. **CONCISE DISPLAY**: Claude shows implementation with minimal commentary (leverage brevity strength)
5. **SAFETY RESTRICTION**: NO direct file edits except CLAUDE.md, Design.md, Implementation.md
6. **SUDO OVERRIDE**: "SUDO: Update {filename} directly" allows controlled exceptions
7. **EFFICIENT VERIFICATION**: Claude batch-reads updated files to verify implementation
8. **TODO MANAGEMENT**: Claude actively uses TodoWrite to track complex multi-step processes
9. **COMPLETION ANALYSIS**: Claude uses reasoning to check success criteria and edge cases

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

## Claude-Specific Optimizations

### Context Window Management
- **Prioritized Loading**: Load CLAUDE.md → Design.md ADRs → Implementation.md → key system files only
- **Smart File Selection**: Claude analyzes current phase to determine which files are actually needed
- **Chunked Analysis**: Use Task tool for complex searches to preserve context space
- **Context Efficiency**: Batch read related files simultaneously rather than sequential reads

### Leverage Claude's Strengths
- **Reasoning Power**: Use for architectural analysis, edge case detection, error scenario planning
- **Code Analysis**: Deep understanding of code relationships and dependencies
- **Concise Communication**: Minimal output unless detail specifically requested
- **Safety Focus**: Conservative approach to file editing with clear change explanations
- **Parallel Processing**: Simultaneous tool calls for efficient information gathering

### Claude Limitations Mitigations
- **No Memory Between Sessions**: CLAUDE.md system provides persistent context
- **Context Window Limits**: Task tool offloading for complex searches
- **File Editing Restrictions**: Terminal diff system for safe code changes
- **Batch Operations**: Parallel tool calls instead of sequential operations

### Performance Patterns for Claude
- **Single Message Multi-Tool**: Batch multiple tool calls in one response
- **Context Preservation**: Read all needed files upfront, not as-needed
- **Structured Output**: Use markdown formatting for clear diffs and explanations
- **Proactive Todo Management**: Create todos immediately for multi-step processes
- **Error Anticipation**: Use reasoning to predict and prevent common issues

**This context enables seamless Claude session continuity with optimized performance and safety.**