⎿ FSM Background Tasks Performance Optimization Plan

Current State Analysis

After analyzing all 5 task files, I found excellent foundation 
patterns but critical performance gaps:

Strengths: MetadataManager integration, SmallVec usage, 
CompactString patterns, expert async design
Critical Issues: Manual file streaming, blocking directory 
traversal, inefficient ANSI parsing, buffer waste

Phase 1: Critical Performance Bottlenecks (High Impact)

1.1 File Operations Task (file_ops_task.rs)

- Issue: Manual byte-by-byte streaming with 64KB buffers allocated
    per operation
- Fix: Replace with tokio::fs::copy() + buffer pool pattern
- Impact: 40-60% faster file operations, zero buffer waste

1.2 Size Task (size_task.rs)

- Issue: Blocking WalkDir::new() prevents async concurrency
- Fix: Migrate to tokio::fs::read_dir() with async stream 
processing
- Impact: 50-70% faster directory size calculations

1.3 Search Tasks (search_task.rs, filename_search_task.rs)

- Issue: Manual ANSI escape parsing, excessive string allocations
- Fix: strip-ansi-escapes crate + pre-allocated capacity
- Impact: 25-35% faster search result processing

Phase 2: Memory & Allocation Optimizations (Medium Impact)

2.1 Buffer Pool Implementation

- Pattern: Reusable Bytes buffers for file operations
- Crate: bytes crate with pool management
- Benefit: Zero allocation file I/O

2.2 String Processing Optimization

- Pattern: SmallString for filenames, CompactString for paths
- Integration: Leverage existing CompactString patterns
- Benefit: Stack allocation for 90% of filenames

2.3 Progress Update Batching

- Pattern: Batch progress updates to reduce channel overhead
- Implementation: Time-based or count-based batching
- Benefit: 15-20% reduction in UI update overhead

Phase 3: State-of-Art External Crates Integration (Enhancement)

3.1 High-Performance I/O

- tokio-uring: For Linux io_uring support (where available)
- memmap2: For large file operations via memory mapping
- Impact: 2-3x improvement for large file operations

3.2 Advanced Search Algorithms

- tantivy: For full-text search indexing (optional enhancement)
- rayon: For CPU-intensive search parallelization
- Impact: 5-10x improvement for complex search queries

3.3 Concurrent Execution Optimization

- tokio-stream: Advanced stream processing patterns
- futures-concurrency: Structured concurrency patterns
- Impact: Better resource utilization, predictable performance

Implementation Strategy

1. Preserve Existing Patterns: Maintain MetadataManager 
integration, error handling
2. Incremental Migration: Phase-by-phase deployment to prevent 
regressions  
3. Performance Validation: Before/after measurements for each 
optimization
4. Backward Compatibility: Ensure zero breaking changes to 
existing interfaces

Expected Performance Gains

- File Operations: 40-60% improvement via async I/O + buffer 
pooling
- Directory Traversal: 50-70% improvement via async streams  
- Search Processing: 25-35% improvement via optimized parsing
- Memory Usage: 20-30% reduction via allocation elimination
- Overall Task Throughput: 30-50% improvement across all 
background operations

This plan builds on FSM's excellent architectural foundation while
    addressing the specific bottlenecks that limit maximum 
performance.