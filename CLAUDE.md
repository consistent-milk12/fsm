# AI Context - EXPERT DEVELOPMENT STANDARDS

## 🚨 **MANDATORY CRISIS LESSONS**
**Issue**: Cache-registry architectural divergence caused 9.1% performance regression  
**Solution**: MetadataManager unification → 80% hit rate, 36% faster scans  
**CRITICAL RULE**: Question ALL architectural assumptions before implementation

## 🎯 **EXPERT COLLABORATION PROTOCOL**

### **ARCHITECTURAL ANALYSIS** - MANDATORY FIRST STEP
- **Challenge core assumptions** before any code changes
- **System thinking**: Analyze data flow, memory patterns, component interactions  
- **Developer validation**: Present architectural concerns immediately
- **Evidence-driven**: Profile existing code, measure actual impact

### **IMPLEMENTATION STRATEGY**  
- **Plan mode**: Use for complex changes - present design, get approval
- **Foundation first**: Complete architectural changes before optimizations
- **Gradual migration**: Update high-impact components first, validate
- **Performance validation**: Ensure changes compile and maintain standards

### **RUST PERFORMANCE PATTERNS**
- **Memory optimization**: Algorithm → memory → data structures → implementation
- **Zero-cost abstractions**: Arc<> sharing, DashMap lock-free patterns  
- **Hot path focus**: Optimize operations called thousands of times
- **Syscall reduction**: Eliminate duplicate filesystem operations

## 🏗️ **WINNING ARCHITECTURAL PATTERNS**
```rust
// MetadataManager: Unified cache-registry prevents divergence
pub struct MetadataManager {
    registry: Arc<DashMap<ObjectId, Arc<ObjectInfo>>>,
    cache: Cache<SharedPath, InfoArc>,
}
// Single entry point prevents cache bypassing
```

**Zero-Duplicate Syscalls**: from_path_sync_with_meta captures metadata during creation  
**Sync-First Design**: Fast sync core + async wrappers for compatibility

## 💡 **PERFORMANCE ACHIEVEMENTS**  
**Cache Hit**: 70.9% → 80.0% (+9.1% vs target)  
**Directory Scan**: 14.2ms → 9.01ms (36% improvement)  
**Memory**: 76% reduction via Arc<PathBuf> + SortableEntry architecture