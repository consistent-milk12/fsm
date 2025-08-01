use chrono::Utc;
use compact_str::{CompactString, ToCompactString};
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::sync::Once;
use std::{collections::HashMap, time::Duration};
use tokio::runtime::Runtime;

use std::hint::black_box;

// Import both logging systems
use fsm_core::logging::{
    AppStateInfo as OriginalAppState, FileSystemStateInfo as OriginalFSState,
    LogEntry as OriginalLogEntry, Logger as OriginalLogger, UIStateInfo as OriginalUIState,
    get_log_sender, shutdown_logging as shutdown_original,
};

use fsm_core::logging_opt::{
    AppStateInfo as OptimizedAppState, FileSystemStateInfo as OptimizedFSState,
    LogEntry as OptimizedLogEntry, LoggerBuilder as OptimizedLogger, LoggerConfig,
    UIStateInfo as OptimizedUIState, get_log_sender as get_opt_sender,
    shutdown_logging as shutdown_optimized,
};

static INIT: Once = Once::new();

pub fn init_tracing_once() {
    INIT.call_once(|| {
        // Your tracing initialization code here
        let _ = tracing_subscriber::fmt()
            .with_env_filter("trace")
            .try_init();
    });
}

fn setup_benchmark() {
    init_tracing_once(); // Safe to call multiple times
}

// Benchmark data generators
fn create_sample_app_state() -> (OriginalAppState, OptimizedAppState) {
    let original = OriginalAppState {
        marked_count: 42,
        history_count: 128,
        plugins_count: 8,
        tasks_count: 16,
        started_at_ms: 1722513600000,
        last_error: Some(CompactString::new("Test error")),
    };

    let optimized = OptimizedAppState {
        marked_count: 42,
        history_count: 128,
        plugins_count: 8,
        tasks_count: 16,
        started_at_ms: 1722513600000,
        last_error: Some("Test error".to_compact_string()),
    };

    (original, optimized)
}

fn create_sample_ui_state() -> (OriginalUIState, OptimizedUIState) {
    let original = OriginalUIState {
        selected: Some(5),
        marked_indices_count: 12,
        mode: CompactString::new("normal"),
        overlay: CompactString::new("none"),
        theme: CompactString::new("dark"),
        search_results_count: 0,
        clipboard_overlay_active: false,
    };

    let optimized = OptimizedUIState {
        selected: Some(5),
        marked_indices_count: 12,
        mode: "normal".to_compact_string(),
        overlay: "none".to_compact_string(),
        theme: "dark".to_compact_string(),
        search_results_count: 0,
        clipboard_overlay_active: false,
    };

    (original, optimized)
}

fn create_sample_fs_state() -> (OriginalFSState, OptimizedFSState) {
    let original = OriginalFSState {
        active_pane: 0,
        panes_count: 2,
        current_path: CompactString::new("/home/user/documents"),
        entries_count: 156,
        selected_index: Some(8),
        is_loading: false,
        recent_dirs_count: 10,
        favorite_dirs_count: 5,
    };

    let optimized = OptimizedFSState {
        active_pane: 0,
        panes_count: 2,
        current_path: "/home/user/documents".to_string().into(),
        entries_count: 156,
        selected_index: Some(8),
        is_loading: false,
        recent_dirs_count: 10,
        favorite_dirs_count: 5,
    };

    (original, optimized)
}

// Benchmark: Log entry creation
fn bench_log_entry_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("log_entry_creation");

    let (orig_app, opt_app) = create_sample_app_state();
    let (orig_ui, opt_ui) = create_sample_ui_state();
    let (orig_fs, opt_fs) = create_sample_fs_state();

    group.bench_function("original_basic", |b| {
        b.iter(|| {
            black_box(OriginalLogEntry {
                sequence: 12345,
                timestamp: Utc::now(),
                level: CompactString::new("INFO"),
                target: CompactString::new("fsm_core::benchmark"),
                marker: CompactString::new("BENCH_START"),
                operation_type: CompactString::new("benchmark_test"),
                duration_us: Some(1250),
                source_location: CompactString::new("benchmark.rs:123"),
                message: CompactString::new("Benchmark message"),
                app_state: None,
                ui_state: None,
                fs_state: None,
                fields: dashmap::DashMap::new(),
            })
        })
    });

    group.bench_function("optimized_basic", |b| {
        b.iter(|| {
            black_box(OptimizedLogEntry {
                sequence: 12345,
                timestamp: Utc::now(),
                level: "INFO".to_compact_string(),
                target: "fsm_core::benchmark".to_compact_string(),
                marker: "BENCH_START".to_compact_string(),
                operation_type: "benchmark_test".to_compact_string(),
                duration_us: Some(1250),
                source_location: "benchmark.rs:123".to_compact_string(),
                message: "Benchmark message".to_string(),
                app_state: None,
                ui_state: None,
                fs_state: None,
                fields: HashMap::new(),
            })
        })
    });

    group.bench_function("original_with_states", |b| {
        b.iter(|| {
            black_box(OriginalLogEntry {
                sequence: 12345,
                timestamp: Utc::now(),
                level: CompactString::new("INFO"),
                target: CompactString::new("fsm_core::benchmark"),
                marker: CompactString::new("BENCH_START"),
                operation_type: CompactString::new("benchmark_test"),
                duration_us: Some(1250),
                source_location: CompactString::new("benchmark.rs:123"),
                message: CompactString::new(
                    "Benchmark message
  with state",
                ),
                app_state: Some(orig_app.clone()),
                ui_state: Some(orig_ui.clone()),
                fs_state: Some(orig_fs.clone()),
                fields: dashmap::DashMap::new(),
            })
        })
    });

    group.bench_function("optimized_with_states", |b| {
        b.iter(|| {
            black_box(OptimizedLogEntry {
                sequence: 12345,
                timestamp: Utc::now(),
                level: "INFO".to_compact_string(),
                target: "fsm_core::benchmark".to_compact_string(),
                marker: "BENCH_START".to_compact_string(),
                operation_type: "benchmark_test".to_compact_string(),
                duration_us: Some(1250),
                source_location: "benchmark.rs:123".to_compact_string(),
                message: "Benchmark message with state".to_string(),
                app_state: Some(opt_app.clone()),
                ui_state: Some(opt_ui.clone()),
                fs_state: Some(opt_fs.clone()),
                fields: HashMap::new(),
            })
        })
    });

    group.finish();
}

// Benchmark: JSON serialization
fn bench_json_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("json_serialization");

    let (orig_app, opt_app) = create_sample_app_state();
    let (orig_ui, opt_ui) = create_sample_ui_state();
    let (orig_fs, opt_fs) = create_sample_fs_state();

    let original_entry = OriginalLogEntry {
        sequence: 12345,
        timestamp: Utc::now(),
        level: CompactString::new("INFO"),
        target: CompactString::new("fsm_core::benchmark"),
        marker: CompactString::new("BENCH_START"),
        operation_type: CompactString::new("benchmark_test"),
        duration_us: Some(1250),
        source_location: CompactString::new("benchmark.rs:123"),
        message: CompactString::new("Benchmark serialization"),
        app_state: Some(orig_app),
        ui_state: Some(orig_ui),
        fs_state: Some(orig_fs),
        fields: dashmap::DashMap::new(),
    };

    let optimized_entry = OptimizedLogEntry {
        sequence: 12345,
        timestamp: Utc::now(),
        level: "INFO".to_compact_string(),
        target: "fsm_core::benchmark".to_compact_string(),
        marker: "BENCH_START".to_compact_string(),
        operation_type: "benchmark_test".to_compact_string(),
        duration_us: Some(1250),
        source_location: "benchmark.rs:123".to_compact_string(),
        message: "Benchmark serialization".to_string(),
        app_state: Some(opt_app),
        ui_state: Some(opt_ui),
        fs_state: Some(opt_fs),
        fields: HashMap::new(),
    };

    group.bench_function("original_pretty_json", |b| {
        b.iter(|| {
            let mut buf = Vec::new();
            original_entry.write_pretty_json(&mut buf).unwrap();
            black_box(())
        })
    });

    group.bench_function("optimized_serde_json", |b| {
        b.iter(|| black_box(serde_json::to_string(&optimized_entry).unwrap()))
    });

    group.finish();
}

// Benchmark: Memory usage patterns
fn bench_memory_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_patterns");
    group.throughput(Throughput::Elements(1000));

    group.bench_function("original_dashmap_fields", |b| {
        b.iter(|| {
            let fields = dashmap::DashMap::with_capacity(16);
            for i in 0..1000 {
                fields.insert(
                    CompactString::new(format!("field_{i}")),
                    format!("value_{i}"),
                );
            }
            black_box(fields)
        })
    });

    group.bench_function("optimized_hashmap_fields", |b| {
        b.iter(|| {
            let mut fields = HashMap::with_capacity(16);
            for i in 0..1000 {
                fields.insert(format!("field_{i}"), format!("value_{i}"));
            }
            black_box(fields)
        })
    });

    group.bench_function("compact_string_allocation", |b| {
        b.iter(|| {
            let mut strings = Vec::with_capacity(1000);
            for i in 0..1000 {
                strings.push(CompactString::new(format!("test_string_{i}")));
            }
            black_box(strings)
        })
    });

    group.bench_function("regular_string_allocation", |b| {
        b.iter(|| {
            let mut strings = Vec::with_capacity(1000);
            for i in 0..1000 {
                strings.push(format!("test_string_{i}"));
            }
            black_box(strings)
        })
    });

    group.finish();
}

// Benchmark: Concurrent logging throughput
fn bench_concurrent_logging(c: &mut Criterion) {
    setup_benchmark();
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("concurrent_logging");
    group.sample_size(10);

    for batch_size in [10, 100, 1000].iter() {
        group.throughput(Throughput::Elements(*batch_size));

        group.bench_with_input(
            BenchmarkId::new("original_system", batch_size),
            batch_size,
            |b, &batch_size| {
                b.to_async(&rt).iter(|| async {
                    let _guard = OriginalLogger::init_tracing().await.unwrap();

                    if let Some(sender) = get_log_sender() {
                        for i in 0..batch_size {
                            let entry = OriginalLogEntry {
                                sequence: i,
                                timestamp: Utc::now(),
                                level: CompactString::new("INFO"),
                                target: CompactString::new("benchmark"),
                                marker: CompactString::new("CONCURRENT_TEST"),
                                operation_type: CompactString::new("throughput"),
                                duration_us: Some(100),
                                source_location: CompactString::new("bench.rs:1"),
                                message: CompactString::new(format!("Message {i}")),
                                app_state: None,
                                ui_state: None,
                                fs_state: None,
                                fields: dashmap::DashMap::new(),
                            };
                            let _ = sender.send(entry);
                        }
                    }

                    tokio::time::sleep(Duration::from_millis(10)).await;
                    let _ = shutdown_original().await;
                    black_box(())
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("optimized_system", batch_size),
            batch_size,
            |b, &batch_size| {
                b.to_async(&rt).iter(|| async {
                    let config: LoggerConfig = LoggerConfig {
                        batch_size: batch_size as usize,
                        flush_interval: Duration::from_millis(10),
                        ..Default::default()
                    };

                    let _guard = OptimizedLogger::new()
                        .with_config(config)
                        .build()
                        .await
                        .unwrap();

                    if let Some(sender) = get_opt_sender().await {
                        for i in 0..batch_size {
                            let entry = OptimizedLogEntry {
                                sequence: i,
                                timestamp: Utc::now(),
                                level: "INFO".to_compact_string(),
                                target: "benchmark".to_compact_string(),
                                marker: "CONCURRENT_TEST".to_compact_string(),
                                operation_type: "throughput".to_compact_string(),
                                duration_us: Some(100),
                                source_location: "bench.rs:1".to_compact_string(),
                                message: format!("Message {i}"),
                                app_state: None,
                                ui_state: None,
                                fs_state: None,
                                fields: HashMap::new(),
                            };
                            let _ = sender.send(entry);
                        }
                    }

                    tokio::time::sleep(Duration::from_millis(10)).await;
                    let _ = shutdown_optimized().await;
                    black_box(())
                })
            },
        );
    }

    group.finish();
}

// Benchmark: Field processing efficiency
fn bench_field_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("field_processing");

    // Test with varying field counts
    for field_count in [5, 20, 50].iter() {
        group.throughput(Throughput::Elements(*field_count as u64));

        group.bench_with_input(
            BenchmarkId::new("dashmap_concurrent", field_count),
            field_count,
            |b, &field_count| {
                b.iter(|| {
                    let fields = dashmap::DashMap::with_capacity(field_count);
                    for i in 0..field_count {
                        fields.insert(CompactString::new(format!("key_{i}")), format!("value_{i}"));
                    }

                    // Simulate concurrent reads
                    for i in 0..field_count {
                        let key = CompactString::new(format!("key_{i}"));
                        black_box(fields.get(&key));
                    }
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("hashmap_sequential", field_count),
            field_count,
            |b, &field_count| {
                b.iter(|| {
                    let mut fields = HashMap::with_capacity(field_count);
                    for i in 0..field_count {
                        fields.insert(format!("key_{i}"), format!("value_{i}"));
                    }

                    // Simulate sequential reads
                    for i in 0..field_count {
                        let key = format!("key_{i}");
                        black_box(fields.get(&key));
                    }
                })
            },
        );
    }

    group.finish();
}

// Benchmark: Startup and shutdown performance
fn bench_system_lifecycle(c: &mut Criterion) {
    setup_benchmark();
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("system_lifecycle");
    group.sample_size(10);

    group.bench_function("original_init_shutdown", |b| {
        b.to_async(&rt).iter(|| async {
            let _guard = black_box(OriginalLogger::init_tracing().await.unwrap());
            shutdown_original().await.unwrap();
            black_box(());
        })
    });

    group.bench_function("optimized_init_shutdown", |b| {
        b.to_async(&rt).iter(|| async {
            let _guard = black_box(OptimizedLogger::new().build().await.unwrap());
            shutdown_optimized().await.unwrap();
            black_box(());
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_log_entry_creation,
    bench_json_serialization,
    bench_memory_patterns,
    bench_concurrent_logging,
    bench_field_processing,
    bench_system_lifecycle
);

criterion_main!(benches);
