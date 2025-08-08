//! Profiling macros for easy instrumentation throughout the codebase
//! Provides zero-cost abstractions when profiling is disabled

/// Profile a code block execution time
#[macro_export]
macro_rules! profile_block {
    ($collector:expr, $name:expr, $block:block) => {{
        #[cfg(feature = "profiling")]
        {
            let start = std::time::Instant::now();
            let result = $block;
            let duration = start.elapsed();
            
            $collector.record_event(
                $name,
                duration.as_millis() as u64,
                Some("profile_block"),
            );
            
            result
        }
        
        #[cfg(not(feature = "profiling"))]
        {
            $block
        }
    }};
}

/// Profile a function call
#[macro_export] 
macro_rules! profile_function {
    ($collector:expr, $name:expr, $func:expr $(, $arg:expr)*) => {{
        #[cfg(feature = "profiling")]
        {
            let start = std::time::Instant::now();
            let result = $func($($arg),*);
            let duration = start.elapsed();
            
            $collector.record_event(
                $name,
                duration.as_millis() as u64,
                Some("profile_function"),
            );
            
            result
        }
        
        #[cfg(not(feature = "profiling"))]
        {
            $func($($arg),*)
        }
    }};
}

/// Profile Arc lock operations
#[macro_export]
macro_rules! profile_arc_lock {
    ($collector:expr, $arc:expr, $operation:expr) => {{
        #[cfg(feature = "profiling")]
        {
            let start = std::time::Instant::now();
            let mut contention_detected = false;
            
            // Try non-blocking first to detect contention
            if $arc.try_lock().is_err() {
                contention_detected = true;
            }
            
            let guard = $arc.lock();
            let wait_time = start.elapsed();
            
            $collector.record_arc_operation(
                $operation,
                wait_time.as_millis() as u64,
                contention_detected,
            );
            
            guard
        }
        
        #[cfg(not(feature = "profiling"))]
        {
            $arc.lock()
        }
    }};
}

/// Profile memory usage at a specific point
#[macro_export]
macro_rules! profile_memory_checkpoint {
    ($collector:expr, $checkpoint_name:expr) => {{
        #[cfg(feature = "profiling")]
        {
            let mut system = sysinfo::System::new();
            system.refresh_memory();
            
            let used_mb = system.used_memory() / 1024 / 1024;
            let available_mb = system.available_memory() / 1024 / 1024;
            
            $collector.record_event(
                &format!("memory_checkpoint_{}", $checkpoint_name),
                used_mb,
                Some(&format!("available_mb:{}", available_mb)),
            );
        }
        
        #[cfg(not(feature = "profiling"))]
        {
            // No-op when profiling disabled
        }
    }};
}

/// Profile directory/file operations
#[macro_export]
macro_rules! profile_fs_operation {
    ($collector:expr, $operation:expr, $path:expr, $block:block) => {{
        #[cfg(feature = "profiling")]
        {
            let start = std::time::Instant::now();
            let result = $block;
            let duration = start.elapsed();
            
            $collector.record_event(
                &format!("fs_{}_{}", $operation, 
                    $path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown")),
                duration.as_millis() as u64,
                Some(&format!("fs_operation,path:{}", $path.display())),
            );
            
            result
        }
        
        #[cfg(not(feature = "profiling"))]
        {
            $block
        }
    }};
}

/// Profile UI rendering operations
#[macro_export]
macro_rules! profile_ui_render {
    ($collector:expr, $component:expr, $render_block:block) => {{
        #[cfg(feature = "profiling")]
        {
            let start = std::time::Instant::now();
            let result = $render_block;
            let duration = start.elapsed();
            
            $collector.record_event(
                &format!("ui_render_{}", $component),
                duration.as_millis() as u64,
                Some("ui_rendering"),
            );
            
            if duration.as_millis() > 16 {
                tracing::warn!("Slow UI render for {}: {}ms", $component, duration.as_millis());
            }
            
            result
        }
        
        #[cfg(not(feature = "profiling"))]
        {
            $render_block
        }
    }};
}

/// Profile cache operations
#[macro_export] 
macro_rules! profile_cache_operation {
    ($collector:expr, $operation:expr, $key:expr, $block:block) => {{
        #[cfg(feature = "profiling")]
        {
            let start = std::time::Instant::now();
            let result = $block;
            let duration = start.elapsed();
            let hit = matches!(result, Some(_));
            
            $collector.record_event(
                &format!("cache_{}_{}", $operation, if hit { "hit" } else { "miss" }),
                duration.as_micros() as u64 / 1000, // Convert to ms
                Some(&format!("key:{}", $key)),
            );
            
            result
        }
        
        #[cfg(not(feature = "profiling"))]
        {
            $block
        }
    }};
}

/// Profile async task spawning
#[macro_export]
macro_rules! profile_spawn_task {
    ($collector:expr, $task_type:expr, $future:expr) => {{
        #[cfg(feature = "profiling")]
        {
            let task_id = format!("{}_{}", $task_type, nanoid::nanoid!(8));
            let collector_clone = $collector.clone();
            let task_type_clone = $task_type.to_string();
            let task_id_clone = task_id.clone();
            
            $collector.record_async_operation(&task_type_clone, 0, false);
            
            tokio::spawn(async move {
                let start = std::time::Instant::now();
                let result = $future.await;
                let duration = start.elapsed();
                
                collector_clone.record_async_operation(
                    &task_type_clone, 
                    duration.as_millis() as u64, 
                    true
                );
                
                result
            })
        }
        
        #[cfg(not(feature = "profiling"))]
        {
            tokio::spawn($future)
        }
    }};
}

/// Create a profiling scope that automatically records entry/exit
#[macro_export]
macro_rules! profiling_scope {
    ($collector:expr, $scope_name:expr) => {{
        #[cfg(feature = "profiling")]
        {
            $crate::profiling::macros::ProfilingScope::new($collector, $scope_name)
        }
        #[cfg(not(feature = "profiling"))]
        {
            $crate::profiling::macros::NoOpScope
        }
    }};
}

/// RAII profiling scope for automatic timing
#[cfg(feature = "profiling")]
pub struct ProfilingScope<'a> {
    collector: &'a crate::profiling::ProfileCollector,
    scope_name: String,
    start_time: std::time::Instant,
}

#[cfg(feature = "profiling")]
impl<'a> ProfilingScope<'a> {
    pub fn new(collector: &'a crate::profiling::ProfileCollector, scope_name: &str) -> Self {
        tracing::debug!("Entering profiling scope: {}", scope_name);
        Self {
            collector,
            scope_name: scope_name.to_string(),
            start_time: std::time::Instant::now(),
        }
    }
}

#[cfg(feature = "profiling")]
impl<'a> Drop for ProfilingScope<'a> {
    fn drop(&mut self) {
        let duration = self.start_time.elapsed();
        
        self.collector.record_event(
            &format!("scope_{}", self.scope_name),
            duration.as_millis() as u64,
            Some("profiling_scope"),
        );
        
        tracing::debug!("Exiting profiling scope: {} ({}ms)", 
                       self.scope_name, duration.as_millis());
    }
}

/// No-op scope for when profiling is disabled
#[cfg(not(feature = "profiling"))]
pub struct NoOpScope;

/// Conditional compilation helper for profiling-specific code
#[macro_export]
macro_rules! if_profiling {
    ($profiling_code:block) => {{
        #[cfg(feature = "profiling")]
        {
            $profiling_code
        }
    }};
}

/// Helper macro to get current function name for profiling
#[macro_export]
macro_rules! current_function {
    () => {{
        fn f() {}
        fn type_name_of<T>(_: T) -> &'static str {
            std::any::type_name::<T>()
        }
        let name = type_name_of(f);
        // Extract function name from full path
        name.strip_suffix("::f").unwrap_or(name)
            .split("::")
            .last()
            .unwrap_or("unknown")
    }};
}

/// Profile an entire function (place at function start)
#[macro_export]
macro_rules! profile_function_entry {
    ($collector:expr) => {{
        let function_name = $crate::current_function!();
        $crate::profiling_scope!($collector, function_name)
    }};
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_current_function_macro() {
        fn test_function() {
            let name = crate::current_function!();
            assert!(name.contains("test_function") || name == "unknown");
        }
        test_function();
    }

    #[cfg(feature = "profiling")]
    #[test]
    fn test_profiling_scope() {
        use crate::profiling::ProfileCollector;
        let collector = ProfileCollector::new();
        
        {
            let _scope = crate::profiling_scope!(&collector, "test_scope");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        
        let snapshot = collector.get_snapshot().unwrap();
        assert!(!snapshot.custom_events.is_empty());
    }
}