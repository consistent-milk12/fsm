# FSM - File System Manager (TUI)

Rust TUI file manager: ratatui+tokio, async MVC pattern, actor model with channels.

## Architecture
- **MVC**: Model(AppState), View(UI), Controller(events)  
- **Async**: tokio runtime, non-blocking I/O
- **State**: Arc<Mutex<AppState>>, thread-safe
- **Communication**: mpsc channels between components

## Key Modules
```
src/main.rs lib.rs error.rs logging.rs
cache/ config/ controller/ model/ view/ fs/ tasks/ plugins/ util/
```

## Dependencies
ratatui tokio crossterm tracing moka serde anyhow thiserror ansi-to-tui

## Mandatory Workflow
After ANY source edit:
1. cargo fmt
2. cargo check  
3. cargo clippy
4. cargo build (only after significant features)

## Code Guidelines
1. Show diff for approval before changes
2. Quality > speed
3. Modern Rust 2024, full clippy compliance
4. Document significant changes in CLAUDE.md
5. Compactify CLAUDE.md when lengthy
- Always organize rust functions into structs

## Performance Optimization Patterns (2024-07-25)
**Learned from file_ops_task.rs implementation:**
- **Pre-calculated intervals**: Compute adaptive progress intervals outside hot loops
- **Explicit types**: Use explicit type annotations for self-documenting code
- **Structured errors**: Break complex error construction into readable steps
- **Constants**: Use `const` for fixed values like `BUFFER_SIZE = 64 * 1024`
- **Loop labels**: Use labeled loops for clear control flow (`'copy_file_bytes: loop`)
- **Adaptive algorithms**: Scale with input size (min(1MB, file_size/10))
- **Hot loop optimization**: Minimize function calls and repeated calculations

## Features
✅ Async dir navigation, incremental loading
✅ File ops: create/delete/copy/move/rename with input prompts  
✅ Search: filename(/) + content(:search) with ripgrep+ANSI
✅ Vim command mode(:) with args, no Ctrl+P overlay
✅ Input prompts: modal n/f/c/m/r key bindings
✅ Caching: Moka LRU, background metadata
✅ UI: ObjectTable, footer hotkeys, overlays
✅ Background file operations: async copy/move/rename via FileOperationTask

## Key Implementations
- **Search**: Raw ripgrep --color=always, ansi-to-tui parsing, Shift+Enter file jump
- **Commands**: :search :grep nf nd, CommandAction enum, parse_command()
- **State**: UIState.Command mode, InputPromptType enum, input_prompt_type field
- **Actions**: ShowInputPrompt/SubmitInputPrompt, Box::pin() for async recursion
- **UI**: Integrated command line, hotkey footer, centered modals
- **File Operations**: Copy/Move/Rename actions, FileOperationTask background processing
- **Task System**: TaskResult enum (Legacy + FileOperationComplete variants)

## Recent Fixes
- Ripgrep: JSON→raw output, ANSI colors preserved
- Search nav: parse_file_info_with_base(), absolute paths  
- Folder counts: direct children only, not recursive
- Clippy: let-chains, format!(), #[derive(Default)], Display trait
- Command unification: removed Ctrl+P, vim-style only
- :grep overlay: Fixed DirectContentSearch to activate ContentSearch overlay
- Line jumping: OpenFile(PathBuf, Option<u32>), VS Code --goto file:line support
- File operations: Complete copy/move/rename implementation with c/m/r keys
- TaskResult enum: Migrated from struct to enum with Legacy + FileOperationComplete variants
- AppError Clone: Manual implementation for non-Clone fields (io::Error)
- Async recursion: Box::pin() for recursive copy_directory function

## Technical Notes
- Action enum: no PartialEq (RawSearchResult.Text)
- Caching: two-phase loading, ObjectInfo::from_light_info()
- Virtual scrolling, conditional redraws
- Background tasks via task_tx/action_tx channels
- Plugin system: stub implementation ready

## Project Recipes
- **Git Recipe**: `git clone [url]` → `cd [repo]` → `git checkout -b feature-branch` → `git add .` → `git commit -m "descriptive message"` → `git push origin feature-branch`

Build: `cargo build --release`
Run: `RUST_LOG=debug cargo run`

## Recipes
**recipe git**: 1.git status 2.For each modified/untracked file: git add {file} → analyze git diff → git commit {msg} → git push

## Design Documentation Protocol
**MANDATORY**: All significant feature development MUST follow this workflow:

### 1. Planning Phase (Before Implementation)
- Analysis → Planning → Implementation workflow  
- Use @Design.md for comprehensive feature planning
- Document tiered priority system (TIER 1 = Critical, TIER 2 = High Impact, etc.)
- Break features into phases with clear success criteria
- Include architecture changes, dependencies, and technical considerations

### 2. Implementation Phase (During Development)  
- Document detailed implementation workflow in Design.md
- Include actual code snippets and step-by-step changes made
- Record technical challenges encountered and solutions
- Update success criteria as features are completed [x] vs [ ]

### 3. Completion Phase (After Implementation)
- Add comprehensive "IMPLEMENTATION COMPLETE" section to Design.md
- Document workflow summary with 8 key implementation steps:
  1. Action System Extension (new enums/variants)
  2. Background Task System (new task types)  
  3. Data Structure Extensions (TaskResult, etc.)
  4. Key Bindings Implementation
  5. Input Processing Logic
  6. Handler Implementation  
  7. Core Operation Logic
  8. Completion/Error Handling
- Include Technical Challenges Resolved section
- Document Architecture Integration details
- Describe User Experience workflow

**This chronology creates permanent project memory and enables efficient future development.**

## Update Instructions
- Update project instructions to show the default diff on terminal for user approval, don't use external editor

## General Workflow Notes
- Always generate the code in the terminal don't add directly if you are not adding to @Design.md or @CLAUDE.md. I will write the edits to the code base by myself. After I am done I will tell you so that you can do a read again to make sure that it was added/changed properly.