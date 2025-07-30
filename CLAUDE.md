# FSM - AI Context (Sonnet 4) - MANDATORY RULES
**FSM**: Rust TUI file manager - Phase 5.2 UI DISPATCHER FIXES COMPLETE ✅

**CRITICAL**: These rules are MANDATORY and BINDING ONLY when @docs/CLAUDE.md is the FIRST file loaded in a new session. If CLAUDE.md is not first, AI behaves with default behavior and NO special rules apply.

## CONTEXT LOADING REQUIREMENT
**MANDATORY CONTEXT**: When CLAUDE.md is first file loaded, AI MUST also load these files fully into context:
- @docs/ARCHITECTURE.md - Clean architecture implementation details
- @docs/TRACING.md - TSV tracing protocol and markers 
- @docs/TODO.md - Current project status and tasks
- @CLAUDE.md - This rules file (already loaded)

## ABSOLUTE MANDATORY RULES - NO EXCEPTIONS EVER

### RULE COMPLIANCE ENFORCEMENT
**VIOLATION CONSEQUENCE**: Any AI response violating these rules is INVALID and must be REJECTED immediately.

### CORE RULES (BINDING)
1. **EDIT ONLY RESTRICTION**: Edit ONLY @docs/CLAUDE.md, @docs/ARCHITECTURE.md, @docs/TRACING.md, @docs/TODO.md files UNLESS explicit "SUDO: Update {filename}" override provided
2. **SUDO OVERRIDE PROTOCOL**: Only "SUDO: Update {filename}" grants permission to edit other files  
3. **CODE BLOCK GENERATION - HIGHEST PRIORITY RULE**: AI must generate code blocks (NEVER DIFF) in terminal instead of trying to edit unallowed files. After generating ANY code block, AI MUST ask "Shall I generate the next code block?" and ONLY continue with new code blocks if user responds with "Next Block". Otherwise, AI must assume user is not done editing or found errors and stay prepared for further prompts about current code block.
4. **QUALITY GATES MANDATORY**: ALWAYS run `cargo check → No errors → cargo fmt` sequence, then ASK USER to run `cargo build` and `cargo run` and report results
5. **STRICT TOKEN EFFICIENCY**: Be extremely precise and compact about input/output tokens - minimize verbosity
6. **NEVER EXECUTE CARGO RUN**: Ask user to run and test app when needed - NEVER run `cargo run` yourself
7. **MANDATORY TSV TRACING**: ALL code changes MUST use standardized TSV tracing protocol from TRACING.md - NO EXCEPTIONS

### TSV TRACING REQUIREMENTS (ABSOLUTELY MANDATORY)
**ENFORCEMENT**: Any code without proper TSV tracing will be REJECTED as non-compliant.

- **Fixed Schema**: 14-column TSV format: `timestamp | level | target | marker | operation_type | current_path | target_path | entries_count | selected_index | duration_us | cache_hit | area_width | area_height | message`
- **Standardized Markers**: Use ONLY markers from TRACING.md Column 4 list (ENTER_START, ENTER_COMPLETE, ACTION_DISPATCH_START, etc.)
- **Instrumentation Pattern**: `#[instrument(fields(marker="X", operation_type="Y", ...))]` for functions
- **State Events**: `info!(marker="STATE_BEFORE/STATE_AFTER", ...)` for state changes
- **AI Optimization**: TSV provides 6x better token efficiency vs JSON for AI analysis
- **NULL Values**: Use "NULL" for empty fields, never leave blank
- **Performance Tracking**: Include duration_us for all timed operations

### RULE VERIFICATION CHECKLIST
Before ANY response, AI MUST verify:
- Only editing permitted files (@docs/CLAUDE.md, @docs/ARCHITECTURE.md, @docs/TRACING.md, @docs/TODO.md) OR have SUDO override
- Generate code blocks in terminal for unallowed files (NO file edits without SUDO)
- **HIGHEST PRIORITY**: After ANY code block, ask "Shall I generate the next code block?" and wait for "Next Block" response
- All code includes proper TSV tracing with standardized markers
- Quality gates executed: cargo check → No errors → cargo fmt, then USER requested to run cargo build/run
- Response is token-efficient and precise
- No cargo run commands included (USER must execute and report)