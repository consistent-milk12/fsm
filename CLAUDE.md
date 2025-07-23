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

## Features
✅ Async dir navigation, incremental loading
✅ File ops: create/delete with input prompts  
✅ Search: filename(/) + content(:search) with ripgrep+ANSI
✅ Vim command mode(:) with args, no Ctrl+P overlay
✅ Input prompts: modal n/f key bindings
✅ Caching: Moka LRU, background metadata
✅ UI: ObjectTable, footer hotkeys, overlays

## Key Implementations
- **Search**: Raw ripgrep --color=always, ansi-to-tui parsing, Shift+Enter file jump
- **Commands**: :search :grep nf nd, CommandAction enum, parse_command()
- **State**: UIState.Command mode, InputPromptType enum, input_prompt_type field
- **Actions**: ShowInputPrompt/SubmitInputPrompt, Box::pin() for async recursion
- **UI**: Integrated command line, hotkey footer, centered modals

## Recent Fixes
- Ripgrep: JSON→raw output, ANSI colors preserved
- Search nav: parse_file_info_with_base(), absolute paths  
- Folder counts: direct children only, not recursive
- Clippy: let-chains, format!(), #[derive(Default)], Display trait
- Command unification: removed Ctrl+P, vim-style only
- :grep overlay: Fixed DirectContentSearch to activate ContentSearch overlay
- Line jumping: OpenFile(PathBuf, Option<u32>), VS Code --goto file:line support

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

## Update Instructions
- Update project instructions to show the default diff on terminal for user approval, don't use external editor