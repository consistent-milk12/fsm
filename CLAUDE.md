READ-ONLY

# AI Context (Sonnet 4) - MANDATORY RULES
**CRITICAL**: These rules are MANDATORY and BINDING ONLY when @CLAUDE.md is the FIRST file loaded in a new session. If CLAUDE.md is not first, AI behaves with default behavior and NO special rules apply.

## CONTEXT LOADING REQUIREMENT
**MANDATORY CONTEXT**: When CLAUDE.md is first file loaded, AI MUST also load these files fully into context:
- @CLAUDE.md - This rules file (already loaded)
- @docs/TRACING.md - This file contains vital information to the related project's standardized JSON tracing logs.
- @DOCS/TODO.md - Permanent context TODO list to refer to.

## ABSOLUTE MANDATORY RULES - NO EXCEPTIONS EVER

### RULE COMPLIANCE ENFORCEMENT
**VIOLATION CONSEQUENCE**: Any AI response violating these rules is INVALID and must be REJECTED immediately.

### CORE RULES (BINDING and **HIGHEST PRIORITY**)
1. **EDIT ONLY RESTRICTION - HIGHEST PRIORITY RULE**: Edit ONLY @docs/TRACING.md, @docs/TODO.md files, if the user asks to edit anything apart from these files, the AI must reply with "FORBIDDEN".
2. **CMD TOOL USAGE PROTOCOL - HIGHEST PRIORITY RULE**: AI must provide any cmd command directly to the user to run and filter them for necessary context. This approach will further optimize and limit model usuage by keeping the context window uncluttered. If user asks to run cmd commands by mistake, replty with "FORBIDDEN".
3. **CODE BLOCK GENERATION - HIGHEST PRIORITY RULE**: AI must generate code blocks (NEVER DIFF) in terminal instead of trying to edit unallowed files. After generating ANY code block, AI SHOULD ONLY continue with new code blocks if user responds with "Next Block". Otherwise, AI must assume user is not done editing or found errors and stay prepared for further prompts about current code block.
4. **STRICT TOKEN EFFICIENCY - HIGHEST PRIORITY RULE**: Be extremely precise and compact about input/output tokens - minimize verbosity
   - **CRITICAL DOCUMENTATION RULE**: All @docs/ files MUST remain compactified for optimal token consumption
   - **FORBIDDEN**: Expanding compactified documentation - doing so wastes tokens and violates efficiency requirements
   - **MANDATORY**: When updating docs, maintain or improve compactification while preserving essential technical details
   - **RESPONSE OPTIMIZATION**: After 
   - **PREAMBLE/POSTAMBLE FORBIDDEN**: No "Here's what I'll do" or "In summary" - direct responses only
   - **CONTEXT EFFICIENCY**: Only load minimal essential context - avoid reading unnecessary files
5. **MANDATORY JSON TRACING**: ALL code changes MUST use a standardized JSON tracing protocol.

### TSV TRACING REQUIREMENTS (ABSOLUTELY MANDATORY)
**ENFORCEMENT**: Any code without proper TSV tracing will be REJECTED as non-compliant.

### RULE VERIFICATION CHECKLIST
Before ANY response, AI MUST verify:
- Generate code blocks in terminal for unallowed files (NO file edits without SUDO)
- **HIGHEST PRIORITY**: After ANY code block, write "Waiting..." and wait for "Next Block" response
- All code includes proper TSV tracing with standardized markers
- Response is token-efficient and precise
- **DOCUMENTATION COMPLIANCE**: All @docs/ files remain compactified - NO expansion of compact documentation allowed