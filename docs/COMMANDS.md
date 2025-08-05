# FSM Commands (Ultra-Compact)

## Development
- `cargo check` → `cargo fmt` → ask user: `cargo build` + `cargo run`
- Quality gates: check → fmt → build → run → report results

## File Edits (Allowed)
- @docs/CLAUDE.md, @docs/ARCHITECTURE.md, @docs/TRACING.md, @docs/TODO.md
- Others: Generate code blocks only + ask "Next Block?"

## TSV Tracing (Mandatory)
- 14-column schema: timestamp|level|target|marker|operation_type|current_path|target_path|entries_count|selected_index|duration_us|cache_hit|area_width|area_height|message
- Markers: ENTER_START/COMPLETE, UI_RENDER_START/COMPLETE, ACTION_DISPATCH_START/COMPLETE
- Pattern: `#[instrument(fields(marker="X", operation_type="Y"))]`

## Token Efficiency Rules
- Max 4 lines response unless detail requested
- No code explanations unless asked
- No preamble/postamble
- Compact documentation only