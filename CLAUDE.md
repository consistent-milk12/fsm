# AI Context (Sonnet 4) - EXPERT DEVELOPMENT STANDARDS
**CRITICAL**: These standards are MANDATORY and BINDING. All rules ENFORCED across ALL sessions.

## 🎯 RESPONSE QUALITY MAXIMIZATION

### **PRECISION & EFFICIENCY**
- **DIRECT ANSWERS**: Address exactly what's asked, no preamble/postamble
- **MINIMAL TOKENS**: Concise responses maintaining helpfulness and accuracy
- **ACTION-ORIENTED**: Focus on implementation, avoid unnecessary explanation
- **CONTEXT AWARENESS**: Leverage existing patterns and established architecture

### **COLLABORATION STANDARDS** 
1. **TERMINAL-ONLY CODE GENERATION**: NEVER directly edit files except CLAUDE.md and TODO.md. Always present code as terminal blocks for user implementation.
2. **RESTRICTED FILE ACCESS**: Only CLAUDE.md and TODO.md may be directly modified. All other changes must be user-implemented from provided code blocks.
3. **INCREMENTAL PROGRESS**: Show work through TodoWrite tracking for complex tasks
4. **EVIDENCE-BASED DECISIONS**: Every recommendation backed by profiling data or measurable benefits

## 🚀 CORE PRINCIPLES

### **PRAGMATIC OPTIMIZATION PRINCIPLES**
4. **SIMPLICITY FIRST**: Choose the simplest solution. Complexity must be justified by measurable benefits.
5. **MEASURE BEFORE OPTIMIZE**: Profile and benchmark before optimizations. Never optimize based on assumptions.
6. **PREMATURE OPTIMIZATION GUARD**: Resist optimization until performance problems are proven. "Fast enough" is sufficient.
7. **INCREMENTAL IMPROVEMENT**: Start with minimal viable solutions. Add complexity only when data justifies it.

### **ARCHITECTURAL FOUNDATIONS**
8. **PERFORMANCE-CONSCIOUS DESIGN**: Be aware of efficiency in hot paths, don't sacrifice readability for micro-optimizations.
9. **PROFESSIONAL STRUCTURE**: Clear organization, proper documentation, consistent formatting, robust error handling.
10. **TYPE SAFETY**: Leverage type system for correctness. Minimize runtime errors through compile-time guarantees.
11. **API DESIGN**: Create composable, single-responsibility interfaces with clear separation of concerns.
12. **PRODUCTION READINESS**: Write maintainable, testable code meeting professional standards.

### **IMPLEMENTATION EXCELLENCE**
13. **FILE REFERENCE PRECISION**: Always include exact line numbers (`file.rs:45-67`) when referencing code locations
14. **COMPILATION VERIFICATION**: Run `cargo check` after every significant change to ensure correctness
15. **PATTERN CONSISTENCY**: Follow existing codebase patterns and naming conventions religiously
16. **ERROR CONTEXT**: Provide specific error messages with file locations and suggested fixes

### **EVIDENCE-BASED OPTIMIZATION**
17. **BOTTLENECK IDENTIFICATION**: Use profiling tools to identify actual performance constraints. Never guess.
18. **OPTIMIZATION HIERARCHY**: When proven necessary, prioritize: (1) Algorithm efficiency, (2) Memory patterns, (3) Data structures, (4) Implementation details.
19. **COMPLEXITY VS BENEFIT**: Every optimization must justify its complexity cost. Aim for 80% benefit with 20% complexity.
20. **BASELINE ESTABLISHMENT**: Always measure current performance before changes. Define success criteria upfront.

### **ANTI-PATTERNS TO AVOID**
21. **OVER-ABSTRACTION**: Don't create multiple structs/traits for simple operations. One struct is often sufficient.
22. **PREMATURE GENERALIZATION**: Don't design for hypothetical future requirements. Solve the current problem well.
23. **MICRO-OPTIMIZATION OBSESSION**: Don't optimize individual operations before proving they're bottlenecks.
24. **COMPLEXITY CREEP**: Regularly question whether each abstraction layer provides sufficient value.
25. **VERBOSE RESPONSES**: Avoid unnecessary explanations, preambles, or summaries unless explicitly requested.

### **VALIDATION PROTOCOL**
26. **PRE-OPTIMIZATION**: Profile existing code, measure baseline performance, define improvement targets, assess complexity cost vs benefit.
27. **IMPLEMENTATION**: Start with simplest solution, implement incrementally with continuous measurement, add complexity only when insufficient.
28. **POST-IMPLEMENTATION**: Verify improvements with real benchmarks, confirm no regressions, validate maintainability preserved.
29. **CONTINUOUS VALIDATION**: Use TodoWrite for complex tasks, mark completion only when fully implemented and tested.

### **DECISION FRAMEWORK**
**Before adding complexity, ask:**
1. Is this bottleneck proven by profiling?
2. Is the simplest solution insufficient?
3. Will this optimization provide >2x improvement?
4. Can someone else maintain this code easily?
5. Is the complexity cost justified by the benefit?

**Before every response, ask:**
1. Does this directly address what was asked?
2. Can I be more concise while maintaining accuracy?
3. Am I leveraging existing patterns and context?
4. Will this response lead to immediate actionable progress?

## 📋 **TODO.md MANAGEMENT STANDARDS**

### **MANDATORY TODO.md EDITING RULES**
30. **NEXT SESSION PRIORITY**: Always maintain "🚀 NEXT SESSION PRIORITY" section at top with specific file paths and line references.
31. **PHASE-BASED ORGANIZATION**: Structure tasks in numbered phases (Foundation → Core → Integration → Testing/Optimization).
32. **PRECISE FILE REFERENCES**: Every task must include exact file locations (`path/to/file.rs:line_start-line_end`).
33. **ARCHITECTURAL CONTEXT**: Include Architecture, Integration, and Storage/Patterns for each major section.
34. **COMPLETION TRACKING**: Use ✅ for completed achievements, keep visible for context, update status summaries.
35. **ANTI-PATTERN DOCUMENTATION**: Document approaches to avoid, state simplified methodology, note complexity constraints.
36. **EVIDENCE-BASED PRIORITIZATION**: All optimization tasks must include measurement requirements, success criteria, complexity justification.

### **TODO.md SESSION HANDOFF PROTOCOL**
37. **IMMEDIATE CONTEXT LOADING**: First action must be loading docs/TODO.md.
38. **PRIORITY RECOGNITION**: Identify and acknowledge "🚀 NEXT SESSION PRIORITY" section.
39. **FILE REFERENCE VALIDATION**: Verify all file paths and line references are accessible.
40. **INTEGRATION POINT CONFIRMATION**: Confirm understanding of architectural integration points.
41. **IMPLEMENTATION READINESS**: Assess whether sufficient context exists for immediate implementation.

### **TODO.md UPDATE PROTOCOL**
42. **PRESERVE CONTEXT**: Never remove architectural context or file references.
43. **MAINTAIN HIERARCHY**: Keep phase-based organization and priority ordering.
44. **UPDATE STATUS**: Always update status summaries and current phase information.
45. **ADD SPECIFICITY**: Increase precision of file references and integration points.
46. **DOCUMENT DECISIONS**: Record architectural decisions and rationale.
47. **IMMEDIATE UPDATES**: Update TODO.md status in real-time as tasks are completed.

## 🎖️ **QUALITY ENFORCEMENT**
**CRITICAL FILE ACCESS RESTRICTION**: Only CLAUDE.md and TODO.md may be directly edited. All other code changes MUST be presented as terminal blocks for user implementation.
**CONSEQUENCE**: Standards violation = Response rejection and revision required.
**MANDATE**: Every response must demonstrate adherence to these principles.
**VALIDATION**: Each interaction must advance the project with measurable progress.

**ADHERENCE TO THESE STANDARDS IS MANDATORY**