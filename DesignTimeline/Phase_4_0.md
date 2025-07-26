# Phase 4.0: Event Loop Architectural Rewrite

**Status:** 🚧 In Progress  
**Core:** Modular event processing foundation built, main event_loop.rs integration pending

## ADR-015: Modular Event Processing Architecture
**Context:** Monolithic 2500+ line event_loop.rs became unmaintainable with excessive lock contention  
**Decision:** Implement modular event processing inspired by Yazi, Helix, Broot architectures  
**Status:** 🚧 Foundation built, integration pending
**Consequences:**
- 🎯 **Target**: 10-50x reduction in lock contention through specialized handlers
- 🎯 **Target**: Sub-millisecond event response times with batched operations
- 🎯 **Target**: Clean separation of concerns (UI/business logic/I/O operations)
- 🎯 **Target**: Consistent 60fps rendering with throttling
- 🎯 **Target**: 50% memory reduction through optimized data structures

## ADR-016: Lock-free State Management
**Context:** Heavy mutex contention in Arc<Mutex<AppState>> causing 60fps frame drops  
**Decision:** Implement lock-free state management using ArcSwap and DashMap  
**Status:** 🚧 Architecture designed, migration pending
**Targets:**
- **10-50x reduction** in lock contention through specialized structures
- **Consistent 60fps** rendering with sub-16ms frame times
- **Sub-millisecond** event response across all operations
- **50% memory reduction** through optimized data layout

---

## Modular Architecture Implementation

### Central Event Processing
```rust
// fsm-core/src/controller/event_processor.rs
pub struct EventProcessor {
    handlers: Vec<Box<dyn EventHandler>>,
    event_queue: flume::Receiver<Event>,
    action_sink: tokio::sync::mpsc::UnboundedSender<Action>,
    performance_monitor: PerformanceMonitor,
}

// fsm-core/src/controller/handler_registry.rs
pub struct HandlerRegistry {
    navigation_handler: NavigationHandler,
    command_handler: CommandHandler,
    search_handler: SearchHandler,
    clipboard_handler: ClipboardHandler,
    file_ops_handler: FileOpsHandler,
    handler_priority: BTreeMap<HandlerType, u8>,
}

// Handler trait system
#[async_trait]
pub trait EventHandler: Send + Sync {
    fn can_handle(&self, event: &Event, ctx: &Context) -> bool;
    async fn handle(&mut self, event: Event, ctx: &mut Context) -> Result<Vec<Action>>;
    fn priority(&self) -> u8 { 128 }
    fn handler_type(&self) -> HandlerType;
}
```

---

## Lock-Free State Management

### StateCoordinator Implementation
```rust
// fsm-core/src/controller/state_coordinator.rs
pub struct StateCoordinator {
    // Read-heavy data uses ArcSwap for lock-free access
    ui_state: ArcSwap<UIState>,
    
    // Concurrent access patterns use DashMap
    fs_cache: DashMap<PathBuf, Arc<DirState>>,
    
    // High-frequency updates use atomic counters
    performance_stats: Arc<AtomicStats>,
    
    // Event sourcing for command history
    command_log: append_only_vec::AppendOnlyVec<Command>,
    
    // Configuration with atomic updates
    config: ArcSwap<AppConfig>,
}

impl StateCoordinator {
    /// Update UI state without blocking readers
    pub fn update_ui_state<F>(&self, update_fn: F) 
    where F: FnOnce(&UIState) -> UIState {
        let current = self.ui_state.load();
        let updated = update_fn(&current);
        self.ui_state.store(Arc::new(updated));
    }
    
    /// Lock-free file system cache access
    pub fn get_dir_state(&self, path: &Path) -> Option<Arc<DirState>> {
        self.fs_cache.get(path).map(|entry| entry.value().clone())
    }
}
```

---

## Action Queue with Batching

### ActionBatcher Implementation  
```rust
// fsm-core/src/controller/action_batcher.rs
pub struct ActionBatcher {
    pending_actions: VecDeque<ActionWithMetadata>,
    batch_timer: Instant,
    batch_timeout: Duration,
    max_batch_size: usize,
}

impl ActionBatcher {
    /// Batch compatible actions for optimized processing
    pub fn add_action(&mut self, action: Action, priority: ActionPriority) -> Option<Vec<Action>> {
        self.pending_actions.push_back(ActionWithMetadata {
            action, priority, timestamp: Instant::now(), source: ActionSource::UserInput,
        });
        
        // Check if we should flush the batch
        if self.should_flush_batch() {
            Some(self.flush_batch())
        } else {
            None
        }
    }
    
    /// Optimize batch by combining compatible actions
    fn optimize_batch(&mut self, actions: Vec<Action>) -> Vec<Action> {
        let mut optimized = Vec::new();
        let mut navigation_count = 0;
        
        for action in actions {
            match action {
                Action::MoveSelectionUp | Action::MoveSelectionDown => {
                    navigation_count += 1;
                    if navigation_count <= 3 {  // Limit navigation batching
                        optimized.push(action);
                    }
                }
                _ => optimized.push(action),
            }
        }
        
        optimized
    }
}
```

---

## Performance Monitoring

### Comprehensive Monitoring System
```rust
// fsm-core/src/controller/metrics.rs
pub struct PerformanceMonitor {
    event_latencies: RingBuffer<Duration>,
    render_frame_times: RingBuffer<Duration>,
    action_throughput: AtomicU64,
    memory_usage: AtomicU64,
    cache_hit_rates: CacheMetrics,
}

impl PerformanceMonitor {
    /// Record event processing latency with automatic alerting
    pub fn record_event_latency(&self, latency: Duration) {
        self.event_latencies.push(latency);
        
        // Alert on performance regression
        if latency > Duration::from_millis(16) {
            warn!("Slow event processing detected: {:?}", latency);
        }
        
        self.update_performance_metrics();
    }
    
    /// Generate performance report for debugging
    pub fn generate_report(&self) -> PerformanceReport {
        PerformanceReport {
            avg_event_latency: self.calculate_average_latency(),
            p95_event_latency: self.calculate_p95_latency(),
            render_fps: self.calculate_current_fps(),
            cache_hit_rate: self.calculate_cache_hit_rate(),
            memory_usage_mb: self.memory_usage.load(Ordering::Relaxed) / 1024 / 1024,
            action_throughput_per_sec: self.calculate_action_throughput(),
        }
    }
}
```

---

## Specialized Event Handlers

### Navigation Handler
```rust
// fsm-core/src/controller/handlers/navigation.rs
pub struct NavigationHandler {
    state_coordinator: Arc<StateCoordinator>,
    movement_buffer: VecDeque<NavigationAction>,
    last_movement: Instant,
}

#[async_trait]
impl EventHandler for NavigationHandler {
    fn can_handle(&self, event: &Event, ctx: &Context) -> bool {
        matches!(event, Event::Key(KeyEvent { 
            code: KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown,
            ..
        })) && ctx.ui_mode == UIMode::Browse && ctx.ui_overlay == UIOverlay::None
    }
    
    async fn handle(&mut self, event: Event, ctx: &mut Context) -> Result<Vec<Action>> {
        if let Event::Key(key_event) = event {
            // Buffer rapid navigation events for batching
            self.buffer_navigation_event(key_event);
            
            // Process buffered movements
            Ok(self.process_navigation_buffer())
        } else {
            Ok(vec![])
        }
    }
    
    fn priority(&self) -> u8 { 200 } // High priority for responsive navigation
}
```

### Clipboard Handler
```rust
// fsm-core/src/controller/handlers/clipboard.rs
pub struct ClipboardHandler {
    ekey_processor: Arc<EKeyProcessor>,
    state_coordinator: Arc<StateCoordinator>,
}

#[async_trait]
impl EventHandler for ClipboardHandler {
    fn can_handle(&self, event: &Event, ctx: &Context) -> bool {
        if let Event::Key(key_event) = event {
            matches!(key_event.code, KeyCode::Char('c' | 'x' | 'v') | KeyCode::Tab)
                && (ctx.ui_overlay == UIOverlay::None || ctx.clipboard_overlay_active)
        } else {
            false
        }
    }
    
    async fn handle(&mut self, event: Event, ctx: &mut Context) -> Result<Vec<Action>> {
        if let Event::Key(key_event) = event {
            // Use high-performance processor for clipboard operations
            if let Some(eaction) = self.ekey_processor.process_key(
                key_event, ctx.ui_mode, ctx.ui_overlay, ctx.clipboard_overlay_active
            ) {
                Ok(vec![Action::EActionDispatch(eaction)])
            } else {
                Ok(vec![])
            }
        } else {
            Ok(vec![])
        }
    }
    
    fn priority(&self) -> u8 { 255 } // Highest priority for instant response
}
```

---

## Render Throttling System

### 60fps Render Throttling
```rust
// fsm-core/src/controller/render_throttler.rs
pub struct RenderThrottler {
    last_render: Instant,
    min_frame_duration: Duration,
    dirty_regions: BitSet,
    frame_budget: Duration,
}

impl RenderThrottler {
    /// Check if render should proceed (60fps throttling)
    pub fn should_render(&mut self) -> bool {
        let elapsed = self.last_render.elapsed();
        
        if elapsed >= self.min_frame_duration {
            self.last_render = Instant::now();
            true
        } else {
            false
        }
    }
    
    /// Mark region as needing redraw
    pub fn mark_dirty(&mut self, region: RenderRegion) {
        self.dirty_regions.insert(region as usize);
    }
    
    /// Get regions needing redraw and clear dirty state
    pub fn get_dirty_regions(&mut self) -> Vec<RenderRegion> {
        let regions = self.dirty_regions.iter().map(RenderRegion::from).collect();
        self.dirty_regions.clear();
        regions
    }
}
```

---

## Modular File Structure

### Current Implementation Status
**✅ High-Quality Components:** Production-ready modular foundation  
**❌ Integration Gap:** Main event_loop.rs (2,463 lines) not using modular system

```
controller/
├── event_processor.rs      # ✅ Production-ready: kanal, priority channels, lock-free registry, metrics
├── handlers/
│   └── keyboard_handler.rs # ✅ Solid: HashMap bindings, sequences, modes (minor async trait fix needed)
├── handler_registry.rs     # ❌ Planned component, not implemented
├── state_coordinator.rs    # ❌ Planned component, not implemented  
├── action_batcher.rs       # ❌ Planned component, not implemented
├── render_throttler.rs     # ❌ Planned component, not implemented
├── metrics.rs              # ❌ Planned component, not implemented
└── event_loop.rs           # ❌ Still monolithic 2,463 lines
```

**Quality Assessment:** EventProcessor shows production-ready architecture with excellent performance patterns  
**Critical Work:** Integrate high-quality components with main event loop and implement missing StateCoordinator

---

## Implementation Status & Next Steps

### Components Status
**✅ Completed - Production Quality:**
- **EventProcessor**: Kanal channels, priority routing, lock-free handler registry, comprehensive metrics
- **KeyboardHandler**: HashMap bindings, sequence handling, mode awareness (needs async trait compatibility)
- **Handler Trait System**: Complete EventHandler trait with priority and registration framework

**🚧 In Progress:**
- Integration of EventProcessor with main event loop
- Migration from Arc<Mutex<AppState>> to lock-free patterns

**❌ Pending:**
- StateCoordinator implementation with ArcSwap/DashMap
- ActionBatcher for operation optimization  
- RenderThrottler for 60fps consistency
- Additional specialized handlers (navigation, clipboard, search, file_ops)
- KeyboardHandler async trait compatibility fix

### Performance Targets (Not Yet Achieved)
- **Lock Contention Reduction**: 10-50x reduction through lock-free structures
- **Event Response Time**: Sub-millisecond response across all event types
- **Render Performance**: Consistent 60fps with sub-16ms frame times
- **Memory Efficiency**: 50% reduction through optimized caching and data structures

### Key Dependencies Integrated
- **flume**: High-performance MPMC channels for event queuing
- **arc-swap**: Lock-free atomic reference updates for read-heavy state
- **dashmap**: Concurrent HashMap for file system cache
- **crossbeam**: Lock-free data structures and atomic operations
- **parking_lot**: Fast mutexes where locking still needed

---

## Development Process Excellence

### Critical Integration Work Required
**Foundation Quality:** ✅ EventProcessor production-ready with excellent lock-free patterns  
**Priority 1:** Replace monolithic event_loop.rs with EventProcessor integration  
**Priority 2:** Implement StateCoordinator with ArcSwap/DashMap (architecture proven in EventProcessor)  
**Priority 3:** Add missing components leveraging established patterns  
**Priority 4:** Minor async trait compatibility fixes and performance validation

### Code Quality Findings
**EventProcessor Excellence:**
- Kanal stack-to-stack optimization, lock-free ArcSwap handler registry
- Comprehensive atomic metrics with exponential moving averages
- Graceful overflow handling with event recovery
- Production-ready architecture suitable for high-performance requirements

**KeyboardHandler Strengths:**
- Efficient HashMap bindings with O(1) lookup performance
- Proper sequence buffer management for multi-key commands
- Clean mode separation and helper functions
- Minor fix needed: async trait compatibility for EventHandler interface

### Quality Gates (Pending)
- **Compilation**: Ensure zero errors across integrated modules
- **Testing**: Comprehensive unit tests for new architecture  
- **Performance**: Validate 10-50x improvement targets
- **Memory Safety**: Verify lock-free operations correctness
- **Integration**: Seamless migration without feature regression

**Current Status:** 🚧 Foundation built, major integration work required to achieve documented benefits