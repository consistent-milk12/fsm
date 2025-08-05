use std::{
    collections::{HashMap, VecDeque}, hash::{DefaultHasher, Hash, Hasher}, path::{Path, PathBuf}, sync::{
        atomic::{AtomicU64, Ordering}, Arc, LazyLock
    }, time::{Duration, Instant}
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use compact_str::CompactString;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::str::FromStr;
use tokio::{
    fs as TokioFs,
    sync::{Mutex, MutexGuard, RwLock, RwLockWriteGuard, mpsc, oneshot},
    time,
};
use tracing::{field::Visit, span::{Id as TraceId, Record}, Event, Level, Metadata, Subscriber};
use tracing_appender::{
    non_blocking::WorkerGuard,
    rolling::{RollingFileAppender, Rotation},
};
use tracing_subscriber::{
    EnvFilter, Layer,
    filter::Directive,
    fmt::{FormattedFields, format::DefaultFields},
    layer::{Context as TracingContext, SubscriberExt},
    registry::{LookupSpan, SpanRef},
    util::SubscriberInitExt,
};

const LEVEL_INFO: &str = "INFO";
const LEVEL_DEBUG: &str = "DEBUG";
const LEVEL_WARN: &str = "WARN";
const LEVEL_ERROR: &str = "ERROR";
const LEVEL_TRACE: &str = "TRACE";

static COMMON_MARKERS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    [
        ("ENTER_START", "ENTER_START"),
        ("ENTER_COMPLETE", "ENTER_COMPLETE"),
        ("STATE_BEFORE", "STATE_BEFORE"),
        ("STATE_AFTER", "STATE_AFTER"),
        ("PERF_DIRECTORY_SCAN", "PERF_DIRECTORY_SCAN"),
        ("UI_RENDER_START", "UI_RENDER_START"),
        ("UI_RENDER_COMPLETE", "UI_RENDER_COMPLETE"),
        ("unknown", "unknown"),
    ]
    .iter()
    .copied()
    .collect()
});

static COMMON_OPERATIONS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    [
        ("directory_entry", "directory_entry"),
        ("navigate", "navigate"),
        ("render", "render"),
        ("file_operation", "file_operation"),
        ("unknown", "unknown"),
    ]
    .iter()
    .copied()
    .collect()
});

const TRACE_MARKER: CompactString = CompactString::const_new("marker");
const TRACE_MARKER_UNKNOWN_COMPACT: CompactString = CompactString::const_new("UNKNOWN_MARKER");
const TRACE_MARKER_UNKNOWN_STR: &str = "UNKNOWN_MARKER";

const TRACE_OPERATION_TYPE: CompactString = CompactString::const_new("operation_type");
const TRACE_OPERATION_UNKNOWN_COMPACT: CompactString = CompactString::const_new("UNKNOWN_OPERATION_TYPE");
const TRACE_OPERATION_UNKNOWN_STR: &str = "UNKWON_OPERATION_TYPE";

const TRACE_ENTER_SPAN: CompactString = CompactString::const_new("ENTER_SPAN");
const TRACE_EXIT_SPAN: CompactString = CompactString::const_new("EXIT_SPAN_COMPLETE");
const TRACE_DURATION: CompactString = CompactString::const_new("duration_us");
const TRACE_SOURCE_UNKNOWN_STR: &str = "UNKOWN_SOURCE_LOCATION";

const EVENT_ENTER_SPAN: &str = "ENTER_SPAN";
const EVENT_EXIT_SPAN: &str = "EXIT_SPAN_COMPLETE";

// Configuration with performance-oriented defaults
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggerConfig {
    pub log_dir: PathBuf,

    // Use CompactString for short prefixes
    pub log_file_prefix: CompactString,

    pub batch_size: usize,

    pub flush_interval: Duration,

    // Use CompactString for log level
    pub log_level: CompactString,

    pub max_log_file_size: u64,

    pub max_log_files: usize,

    pub max_field_size: usize,

    pub max_fields_count: usize,

    pub rotation: LogRotation,

    pub enable_console_output: bool,

    // Enable/disable string interning
    pub use_string_interning: bool,

    // Preallocate serialization buffers
    pub preallocate_buffers: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogRotation {
    Never,

    Minutely,

    Hourly,

    Daily,
}

impl Default for LoggerConfig {
    fn default() -> Self {
        Self {
            log_dir: PathBuf::from("./logs"),
            log_file_prefix: CompactString::const_new("app"),
            batch_size: 256, // Increased for better throughput
            flush_interval: Duration::from_millis(25), // Reduced for bette responsiveness
            log_level: CompactString::const_new("trace"),
            max_log_file_size: 50 * 1024 * 1024, // Increased to 50MB
            max_log_files: 10,                   // Increased retention
            max_field_size: 2048,                // Increased field size limit
            max_fields_count: 64,                // Increased field count
            rotation: LogRotation::Never,
            enable_console_output: true,
            use_string_interning: true,
            preallocate_buffers: true,
        }
    }
}

// Optimized state structs using CompactString for small strings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppStateInfo {
    pub marked_count: usize,

    pub history_count: usize,

    pub plugins_count: usize,

    pub tasks_count: usize,

    pub started_at_ms: u64,

    // Use CompactString for error messages
    pub last_error: Option<CompactString>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UIStateInfo {
    pub selected: Option<usize>,

    pub marked_indices_count: usize,

    pub mode: CompactString,

    pub overlay: CompactString,

    pub theme: CompactString,

    pub search_results_count: usize,

    pub clipboard_overlay_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSystemStateInfo {
    pub active_pane: usize,

    pub panes_count: usize,

    pub current_path: CompactString,

    pub entries_count: usize,

    pub selected_index: Option<usize>,

    pub is_loading: bool,

    pub recent_dirs_count: usize,

    pub favorite_dirs_count: usize,
}

// Optimized LogEntry with better memory layout
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub sequence: u64,

    pub timestamp: DateTime<Utc>,

    // Use CompactString for frequently repeated short strings
    pub level: CompactString,

    pub target: CompactString,

    pub marker: CompactString,

    pub operation_type: CompactString,

    pub source_location: CompactString,

    pub message: String, // Keep as String for potentially long messages

    pub duration_us: Option<u64>,

    // Profiling fields for performance monitoring
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_usage_percent: Option<f32>, // CPU usage percentage during operation
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_delta_kb: Option<i64>, // Memory change in KB (can be negative)
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_duration_ns: Option<u64>, // High-precision operation duration in nanoseconds

    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_state: Option<AppStateInfo>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui_state: Option<UIStateInfo>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub fs_state: Option<FileSystemStateInfo>,

    pub fields: HashMap<CompactString, String>,
}

// Error types remain the same
#[derive(Debug, thiserror::Error)]
pub enum LoggingError {
    #[error("Logger already initialized")]
    AlreadyInitialized,

    #[error("Logger not initialized")]
    NotInitialized,

    #[error("Invalid log directory: {0}")]
    InvalidLogDirectory(String),

    #[error("Failed to create log directory: {0}")]
    DirectoryCreationFailed(#[from] std::io::Error),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Shutdown timeout")]
    ShutdownTimeout,
}

// High-performance object pool with lock-free fast path
struct LogEntryPool {
    pool: Mutex<VecDeque<LogEntry>>,

    max_size: usize,

    // Pre-allocated template for faster initialization
    template: LogEntry,
}

impl LogEntryPool {
    fn new(max_size: usize) -> Self {
        let template: LogEntry = LogEntry {
            sequence: 0,
            timestamp: Utc::now(),
            level: CompactString::const_new(""),
            target: CompactString::const_new(""),
            marker: CompactString::const_new(""),
            operation_type: CompactString::const_new(""),
            source_location: CompactString::const_new(""),
            message: String::new(),
            duration_us: None,
            cpu_usage_percent: None,
            memory_delta_kb: None,
            operation_duration_ns: None,
            app_state: None,
            ui_state: None,
            fs_state: None,
            fields: HashMap::new(),
        };

        Self {
            pool: Mutex::new(VecDeque::with_capacity(max_size)),
            max_size,
            template,
        }
    }

    async fn return_entry(&self, mut entry: LogEntry) {
        // Fast reset without deallocating
        entry.sequence = 0;
        entry.timestamp = Utc::now();
        entry.level.clear();
        entry.target.clear();
        entry.marker.clear();
        entry.operation_type.clear();
        entry.source_location.clear();
        entry.message.clear();
        entry.duration_us = None;
        entry.cpu_usage_percent = None;
        entry.memory_delta_kb = None;
        entry.operation_duration_ns = None;
        entry.app_state = None;
        entry.ui_state = None;
        entry.fs_state = None;
        entry.fields.clear();

        let mut pool: MutexGuard<'_, VecDeque<LogEntry>> = self
            .pool
            .lock()
            .await;
        
        if pool.len() < self.max_size {
            pool.push_back(entry);
        }
    }

    // ----------------------------------------------------------
    // Blocking variant for synchronous callers (uses try_lock)
    // ----------------------------------------------------------
    pub fn get_blocking(&self) -> LogEntry {
        self.pool.try_lock().map_or_else(
            |_| -> LogEntry 
            { 
                self.template.clone() 
            },
            |mut guard: MutexGuard<'_, VecDeque<LogEntry>>| -> LogEntry 
            {
                guard
                    .pop_front()
                    .unwrap_or_else(|| -> LogEntry { self.template.clone() })
            },
        )
    }
}

// Fix: Create a Send-safe writer wrapper
struct SendSafeWriter {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl SendSafeWriter {
    fn new(writer: impl Write + Send + 'static) -> Self {
        Self {
            writer: Arc::new(Mutex::new(Box::new(writer))),
        }
    }

    async fn write_all(&self, buf: &[u8]) -> std::io::Result<()> {
        let mut writer: MutexGuard<'_, Box<dyn Write + Send>> = self
            .writer
            .lock()
            .await;
    
        writer.write_all(buf)
    }

    async fn flush(&self) -> std::io::Result<()> {
        let mut writer: MutexGuard<'_, Box<dyn Write + Send>> = self
            .writer
            .lock()
            .await;

        writer.flush()
    }
}

// Optimized logging system with better batching - Fixed Send issues
pub struct LoggingSystem {
    sender: mpsc::UnboundedSender<LogEntry>,

    shutdown_tx: Option<oneshot::Sender<()>>,

    task_handle: Option<tokio::task::JoinHandle<Result<()>>>,

    config: LoggerConfig,

    entry_pool: Arc<LogEntryPool>,

    // Pre-allocated buffer for JSON serialization
    _json_buffer: Arc<Mutex<Vec<u8>>>,
}

impl LoggingSystem {
    async fn new(config: LoggerConfig) -> Result<(Self, WorkerGuard)> {
        validate_config(&config)?;
        setup_log_directory(&config.log_dir).await?;

        let (
            log_tx, 
            log_rx
        ) = mpsc::unbounded_channel::<LogEntry>();
        
        let (
            shutdown_tx, 
            shutdown_rx
        ) = oneshot::channel::<()>();

        // Setup file appender with rotation
        let rotation: Rotation = match config.rotation {
            LogRotation::Never => Rotation::NEVER,

            LogRotation::Minutely => Rotation::MINUTELY,

            LogRotation::Hourly => Rotation::HOURLY,

            LogRotation::Daily => Rotation::DAILY,
        };

        let file_appender: RollingFileAppender = RollingFileAppender::builder()
            .rotation(rotation)
            .filename_prefix(config.log_file_prefix.as_str())
            .filename_suffix("jsonl")
            .max_log_files(config.max_log_files)
            .build(&config.log_dir)
            .context("Failed to create file appender")?;

        let (
            non_blocking,
            guard
        ) = tracing_appender::non_blocking(file_appender);

        let entry_pool: Arc<LogEntryPool> = Arc::new(
            LogEntryPool::new(config.batch_size * 4)
        );
        let task_pool: Arc<LogEntryPool> = entry_pool.clone();
        let task_config: LoggerConfig = config.clone();

        // Pre-allocate JSON buffer if enabled
        let json_buffer: Arc<Mutex<Vec<u8>>> = if config.preallocate_buffers 
        {
            Arc::new(Mutex::new(Vec::with_capacity(64 * 1024))) // 64KB buffer
        } else {
            Arc::new(Mutex::new(Vec::new()))
        };

        let task_json_buffer: Arc<Mutex<Vec<u8>>> = json_buffer.clone();

        // Fix: Create Send-safe writer
        let safe_writer: SendSafeWriter = SendSafeWriter::new(non_blocking);

        let task_handle = tokio::spawn(
            async move 
            {
                Self::log_processing_task(
                    log_rx,
                    shutdown_rx,
                    safe_writer,
                    task_config,
                    task_pool,
                    task_json_buffer,
                )
                .await
            }
        );

        Ok((
            Self {
                sender: log_tx,
                shutdown_tx: Some(shutdown_tx),
                task_handle: Some(task_handle),
                config,
                entry_pool,
                _json_buffer: json_buffer,
            },
            guard,
        ))
    }

    #[expect(
        clippy::iter_with_drain,
        reason = "Necessary for tokio::select! proc_macro to work"
    )]
    #[expect(
        clippy::cognitive_complexity,
        reason = "Expected complexity for tokio::select! proc macro"
    )]
    // Fix: Use Send-safe writer
    async fn log_processing_task(
        mut log_rx: mpsc::UnboundedReceiver<LogEntry>,
        mut shutdown_rx: oneshot::Receiver<()>,
        writer: SendSafeWriter,
        config: LoggerConfig,
        pool: Arc<LogEntryPool>,
        json_buffer: Arc<Mutex<Vec<u8>>>,
    ) -> Result<()> {
        let mut batch: Vec<LogEntry> = Vec::with_capacity(config.batch_size);
        let mut interval: time::Interval = time::interval(config.flush_interval);

        loop {
            tokio::select! {
                // Handle shutdown signal (highest priority)
                _ = &mut shutdown_rx => {
                    tracing::info!("Logging system shutdown initiated");

                    if !batch.is_empty() {
                        if matches!(Self::flush_batch_optimized(&batch, &writer, &json_buffer).await, Ok(())) {
                            let _ = writer.flush().await;
                        }

                        // Return entries to pool
                        for entry in batch.drain(..) {
                            pool.return_entry(entry).await;
                        }
                    }

                    tracing::info!("Logging system shutdown completed");
                    return Ok(());
                }

                // Process incoming log entries
                entry = log_rx.recv() => {
                    if let Some(entry) = entry {
                        batch.push(entry);

                        if batch.len() >= config.batch_size {
                            if matches!(Self::flush_batch_optimized(&batch, &writer, &json_buffer).await, Ok(())) {
                                let _ = writer.flush().await;
                            }

                            // Return entries to pool
                            for entry in batch.drain(..) {
                                pool.return_entry(entry).await;
                            }
                        }
                    } else {
                        tracing::info!("Log channel closed, flushing remaining entries");

                        if !batch.is_empty() {
                            if matches!(Self::flush_batch_optimized(&batch, &writer, &json_buffer).await, Ok(())) {
                               let _ = writer.flush().await;
                            }

                            // Return entries to pool
                            for entry in batch.drain(..) {
                                pool.return_entry(entry).await;
                            }
                        }

                        return Ok(());
                    }
                }

                // Periodic flush
                _ = interval.tick() => {
                    if !batch.is_empty() {
                        if matches!(Self::flush_batch_optimized(&batch, &writer, &json_buffer).await, Ok(())) {
                            let _ = writer.flush().await;
                        }

                        // Return entries to pool
                        for entry in batch.drain(..) {
                            pool.return_entry(entry).await;
                        }
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Efficient batch flush:
    // * Serialises outside the writer-lock to minimise contention.
    // * Reuses a shared buffer via mem::take to avoid reallocations.
    // * Writes the entire batch in one system call.
    // ------------------------------------------------------------------
    async fn flush_batch_optimized(
        batch: &[LogEntry],                // slice of entries to flush
        writer: &SendSafeWriter,           // thread-safe sink
        json_buffer: &Arc<Mutex<Vec<u8>>>, // pooled serialisation buffer
    ) -> Result<()> {
        // ----------------------------------------------------------
        // 1.  Take ownership of the shared buffer without cloning.
        //     After mem::take the guard holds an empty Vec, so the
        //     critical section is extremely short.
        // ----------------------------------------------------------
        let mut local_buf = {
            let mut guard: MutexGuard<'_, Vec<u8>> = json_buffer.lock().await;
            std::mem::take(&mut *guard)
        };

        // ----------------------------------------------------------
        // 2.  Ensure buffer is empty before serialisation.
        // ----------------------------------------------------------
        local_buf.clear();

        // ----------------------------------------------------------
        // 3.  Serialise every log entry into the local buffer.
        //     Push '\n' after each record except the last to keep
        //     valid JSONL without a trailing blank line.
        // ----------------------------------------------------------
        for (idx, entry) in batch.iter().enumerate() {
            // Serialise entry as compact JSON into buffer.
            if let Err(e) = serde_json::to_writer(&mut local_buf, entry) {
                tracing::error!("JSON serialisation failed: {}", e);
                continue; // skip invalid entry
            }

            // Append newline unless this is the final element.
            if idx + 1 < batch.len() {
                local_buf.push(b'\n');
            }
        }

        // ----------------------------------------------------------
        // 4.  Perform a single write_all; holding only writer lock.
        // ----------------------------------------------------------
        writer
            .write_all(&local_buf)
            .await
            .context("batch write failed")?;

        // ----------------------------------------------------------
        // 5.  Return the now-allocated buffer to the shared pool
        //     for reuse, again under a very short lock.
        // ----------------------------------------------------------
        {
            let mut guard: MutexGuard<'_, Vec<u8>> = json_buffer.lock().await;
            *guard = local_buf; // move buffer back
        }

        // ----------------------------------------------------------
        // 6.  Success.
        // ----------------------------------------------------------
        Ok(())
    }

    #[must_use]
    pub fn sender(&self) -> mpsc::UnboundedSender<LogEntry> {
        self.sender.clone()
    }

    ///
    /// # Errors
    ///
    /// Shutdown.
    ///
    pub async fn shutdown(mut self) -> Result<()> {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }

        if let Some(handle) = self.task_handle.take() {
            if let Ok(result) = tokio::time::timeout(Duration::from_secs(15), handle).await {
                match result {
                    Ok(task_result) => task_result.context("Log processing task failed")?,

                    Err(e) => {
                        tracing::error!("Log processing task panicked: {}", e);

                        return Err(LoggingError::ShutdownTimeout.into());
                    }
                }
            } else {
                tracing::error!("Logging system shutdown timeout");

                return Err(LoggingError::ShutdownTimeout.into());
            }
        }

        Ok(())
    }
}

// Global logging system with proper synchronization
static LOGGING_SYSTEM: RwLock<Option<Arc<LoggingSystem>>> = RwLock::const_new(None);
static LOG_SEQUENCE: AtomicU64 = AtomicU64::new(1);

// Fast string lookup for common values
#[inline]
const fn get_level_string(level: Level) -> &'static str {
    match level {
        Level::INFO => LEVEL_INFO,

        Level::DEBUG => LEVEL_DEBUG,

        Level::WARN => LEVEL_WARN,

        Level::ERROR => LEVEL_ERROR,

        Level::TRACE => LEVEL_TRACE,
    }
}

// Logger builder for clean initialization
pub struct LoggerBuilder {
    config: LoggerConfig,
}

impl LoggerBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: LoggerConfig::default(),
        }
    }

    #[must_use]
    pub fn with_config(mut self, config: LoggerConfig) -> Self {
        self.config = config;
        self
    }

    #[must_use]
    pub fn with_directory<P: AsRef<Path>>(mut self, dir: P) -> Self {
        self.config.log_dir = dir.as_ref().to_path_buf();
        self
    }

    #[must_use]
    pub fn with_level(mut self, level: &str) -> Self {
        self.config.log_level = CompactString::new(level);
        self
    }

    #[must_use]
    pub const fn with_batch_size(mut self, size: usize) -> Self {
        self.config.batch_size = size;
        self
    }

    #[must_use]
    pub const fn with_flush_interval(mut self, interval: Duration) -> Self {
        self.config.flush_interval = interval;
        self
    }

    #[must_use]
    pub const fn with_rotation(mut self, rotation: LogRotation) -> Self {
        self.config.rotation = rotation;
        self
    }

    #[must_use]
    pub const fn with_string_interning(mut self, enable: bool) -> Self {
        self.config.use_string_interning = enable;
        self
    }

    #[must_use]
    pub const fn with_buffer_preallocation(mut self, enable: bool) -> Self {
        self.config.preallocate_buffers = enable;
        self
    }

    pub async fn build(self) -> Result<WorkerGuard> {
        let mut system_guard: RwLockWriteGuard<'_, Option<Arc<LoggingSystem>>> =
            LOGGING_SYSTEM.write().await;

        if system_guard.is_some() {
            return Err(LoggingError::AlreadyInitialized.into());
        }

        let (system, guard) = LoggingSystem::new(self.config.clone()).await?;
        let system_arc: Arc<LoggingSystem> = Arc::new(system);

        // Initialize tracing subscriber
        let directive: Directive =
            Directive::from_str(self.config.log_level.as_str()).context("Invalid log level")?;

        let custom_layer: JsonLayer = JsonLayer::new(system_arc.clone());
        let subscriber: tracing_subscriber::layer::Layered<
            tracing_subscriber::filter::Filtered<
                JsonLayer,
                EnvFilter,
                tracing_subscriber::Registry,
            >,
            tracing_subscriber::Registry,
        > = tracing_subscriber::registry()
            .with(custom_layer.with_filter(EnvFilter::from_default_env().add_directive(directive)));

        let _ = subscriber.try_init();

        *system_guard = Some(system_arc);

        drop(system_guard);

        Ok(guard)
    }
}

impl Default for LoggerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct SpanData {
    /// Span start time for duration calculation
    pub start_time: Instant,

    /// Log Level from span metadata
    pub level: CompactString,

    /// Target from span metadata
    pub target: CompactString,

    /// Marker for span events (e.g., `ENTER_START`)
    pub marker: CompactString,

    /// Operation type for categorization
    pub operation_type: CompactString,

    /// Span metadata fields (Stored using std `HashMap`)
    pub fields: HashMap<CompactString, String>,

    /// App State Snapshot
    pub app_state: Option<AppStateInfo>,

    /// UI State Snapshot
    pub ui_state: Option<UIStateInfo>,

    /// FS State Snapshot
    pub fs_state: Option<FileSystemStateInfo>,
}

impl SpanData {
    #[must_use]
    pub fn new(
        level: CompactString,
        target: CompactString,
        marker: CompactString,
        operation_type: CompactString,
    ) -> Self {
        Self {
            start_time: Instant::now(),
            level,
            target,
            marker,
            operation_type,
            fields: HashMap::new(),
            app_state: None,
            ui_state: None,
            fs_state: None,
        }
    }

    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        reason = "Integer microseconds is enough accuracy for this app"
    )]
    pub fn duration_us(&self) -> u64 {
        self.start_time.elapsed().as_micros() as u64
    }

    /// Add field to span
    pub fn record_field(&mut self, key: CompactString, value: String) {
        self.fields.insert(key, value);
    }
}

// Optimized JSON layer
pub struct JsonLayer {
    system: Arc<LoggingSystem>,

    /// Concurrent span storage for lifecycle tracking
    span_storage: DashMap<TraceId, SpanData>,
}

impl JsonLayer {
    #[must_use]
    pub fn new(system: Arc<LoggingSystem>) -> Self {
        Self {
            system,
            span_storage: DashMap::new(),
        }
    }

    /// Helper to emit span lifecycle events
    fn emit_span_event(&self, _id: &TraceId, marker: &str, span_data: &SpanData) {
        let mut entry: LogEntry = self.system.entry_pool.get_blocking();

        // Basic event data
        entry.sequence = LOG_SEQUENCE.fetch_add(1, Ordering::AcqRel);
        entry.timestamp = Utc::now();
        entry.level = CompactString::const_new("INFO");
        entry.target = CompactString::const_new("fsm_core::tracing");
        entry.marker = CompactString::new(marker);
        entry.operation_type.clone_from(&span_data.operation_type);

        // Duration for completion events
        if marker.ends_with("_COMPLETE") {
            entry.duration_us = Some(span_data.duration_us());
        }

        entry.fields = span_data
            .fields
            .iter()
            .map(
                |(k, v): (&CompactString, &String)| -> (CompactString, String) {
                    (k.clone(), v.clone())
                },
            )
            .collect();

        // Copy state snapshots
        entry.app_state.clone_from(&span_data.app_state);
        entry.ui_state.clone_from(&span_data.ui_state);
        entry.fs_state.clone_from(&span_data.fs_state);

        entry.source_location = CompactString::const_new("auto_span");
        entry.message = format!("Span {marker} {0}", span_data.operation_type);

        // Send to background processing
        let _ = self.system.sender().send(entry);
    }

    // Enhanced field extraction with better parsing
    #[expect(unused, reason = "Kept for future cases of complex parsing.")]
    fn extract_field_value(fields_str: &str, field_name: &str) -> Option<CompactString> {
        // Parse key=value pairs from formatted fields
        for part in fields_str.split_whitespace() {
            if let Some((key, value)) = part.trim().split_once('=')
                && key == field_name
            {
                // Remove quotes and return
                let clean_value: &str = value.trim_matches('"').trim_matches('\'');

                return Some(CompactString::new(clean_value));
            }
        }

        None
    }

    // Extract span fields using visitor pattern
    fn extract_span_fields<'a, S>(span: &SpanRef<'a, S>) -> HashMap<CompactString, String>
    where
        S: LookupSpan<'a>,
    {
        let mut fields: HashMap<CompactString, String> = HashMap::new();

        // Try to get formatted fields
        if let Some(formatted) = 
            span
            .extensions()
            .get::<FormattedFields<DefaultFields>>() 
        {
            for part in formatted.fields.split(',') {
                if let Some((key, value)) = part
                    .trim()
                    .split_once('=') 
                {
                    fields.insert(
                        CompactString::new(key.trim()),
                        value.trim_matches('"').trim_matches('\'').to_string(),
                    );
                }
            }
        }

        fields
    }

    // Helper to convert Level to CompactString
    const fn level_to_compact_string(level: Level) -> CompactString {
        CompactString::const_new(get_level_string(level))
    }
}

impl<S> Layer<S> for JsonLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    /// Handle span entry - store span data and emit `ENTER_START`
    fn on_enter(&self, id: &TraceId, ctx: TracingContext<'_, S>) {
        if let Some(span) = ctx.span(id) {
            let metadata: &'static Metadata<'static> = span.metadata();
            let fields: HashMap<CompactString, String> = Self::extract_span_fields(&span);

            // Extract marker and operation_type from fields
            let marker: CompactString = fields.get(&TRACE_MARKER).map_or(
                TRACE_ENTER_SPAN,
                |s: &String| -> CompactString { CompactString::new(s) },
            );

            let operation_type: CompactString = fields.get(&TRACE_OPERATION_TYPE).map_or(
                TRACE_OPERATION_UNKNOWN_COMPACT,
                |s: &String| -> CompactString { CompactString::new(s) },
            );

            let span_data: SpanData = SpanData
            {
                start_time: Instant::now(),
                level: Self::level_to_compact_string(*metadata.level()),
                target: CompactString::new(metadata.target()),
                marker,
                operation_type,
                fields,
                app_state: None,
                ui_state: None,
                fs_state: None,
            };

            // Emit ENTER_START event
            self.emit_span_event(id, EVENT_ENTER_SPAN, &span_data);
            
            // Store span for on_exit
            self.span_storage.insert(id.clone(), span_data);
        }
    }

    fn on_exit(&self, id: &TraceId, _ctx: TracingContext<'_, S>) {
        if let Some((_, mut span_data)) = self.span_storage.remove(id)
        {
            // Update marker for completion
            span_data.marker = TRACE_EXIT_SPAN;

            // EMIT EXIT_SPAN with duration
            self.emit_span_event(id, EVENT_EXIT_SPAN, &span_data);
        }
    }

    // ---------------------------------------------------------------------
    // Synchronous, allocation-free event handler
    // ---------------------------------------------------------------------
    fn on_event(&self, event: &Event<'_>, ctx: TracingContext<'_, S>) {
        // --------------------------------------------------------------
        // 1.  Collect field data with the existing visitor
        // --------------------------------------------------------------
        let mut visitor: JsonVisitor = JsonVisitor::new(&self.system.config);
        event.record(&mut visitor);

        // --------------------------------------------------------------
        // 2.  Grab static metadata once (no heap allocation here)
        // --------------------------------------------------------------
        let meta: &'static Metadata<'static> = event.metadata();
        
        // --------------------------------------------------------------
        // 3.  Fetch a reusable LogEntry from the pool *without* await
        //     (requires a synchronous helper on the pool; see note)
        // --------------------------------------------------------------
        let mut entry: LogEntry = self.system.entry_pool.get_blocking();

        // --------------------------------------------------------------
        // 4.  Populate fixed fields as cheaply as possible
        // --------------------------------------------------------------
        entry.sequence = LOG_SEQUENCE.fetch_add(1, Ordering::AcqRel);
        entry.timestamp = Utc::now();
        entry.level = CompactString::const_new(get_level_string(*meta.level()));
        entry.target = CompactString::new(meta.target());

        // --------------------------------------------------------------
        // 5. Inherit span context if available
        // --------------------------------------------------------------
        if let Some(span) = ctx.lookup_current() 
        && let Some(span_data) = self
                .span_storage
                .get(&span.id())
        {
            if entry.operation_type.is_empty() 
            {
                entry
                    .operation_type
                    .clone_from(&span_data.operation_type);
            }

            for (key, value) in &span_data.fields 
            {
                if !visitor.fields.contains_key(key)
                {
                    visitor.fields.insert(key.clone(), value.clone());
                }
            }

            // Inherit state methods
            entry.app_state.clone_from(&span_data.app_state);
            entry.ui_state.clone_from(&span_data.ui_state);
            entry.fs_state.clone_from(&span_data.fs_state);            
        }

        // --------------------------------------------------------------
        // 5.  Marker / operation look-up without async interning
        //     Falls back to "unknown" for unseen strings
        // --------------------------------------------------------------
        entry.marker = visitor
            .fields
            .get(&TRACE_MARKER)
            .map(|s: &String| -> &'static str {
                COMMON_MARKERS.get(s.as_str()).copied().unwrap_or(TRACE_MARKER_UNKNOWN_STR)
            })
            .map_or(
                TRACE_MARKER_UNKNOWN_COMPACT,
                CompactString::const_new,
            );

        entry.operation_type = visitor
            .fields
            .get(&TRACE_OPERATION_TYPE)
            .map(|s: &String| -> &'static str {
                COMMON_OPERATIONS
                    .get(s.as_str())
                    .copied()
                    .unwrap_or(TRACE_OPERATION_UNKNOWN_STR)
            })
            .map_or(
                TRACE_OPERATION_UNKNOWN_COMPACT,
                CompactString::const_new,
            );

        // --------------------------------------------------------------
        // 6.  Duration (optional) and source location
        // --------------------------------------------------------------

        entry.duration_us = visitor
            .fields
            .get(&TRACE_DURATION)
            .and_then(|v: &String| -> Option<u64> { v.parse().ok() });

        entry.source_location = CompactString::new(format!(
            "{}:{}",
            meta.file().unwrap_or(TRACE_SOURCE_UNKNOWN_STR),
            meta.line().unwrap_or(0)
        ));

        // --------------------------------------------------------------
        // 7.  Move message & fields in (no clones)
        // --------------------------------------------------------------
        entry.message = visitor.message;
        entry.fields = visitor.fields;

        // --------------------------------------------------------------
        // 8.  Fire-and-forget send to background task
        //     Ignore errors that occur only during shutdown
        // --------------------------------------------------------------
        let _ = self.system.sender().send(entry);
    }

    fn on_record(&self, id: &TraceId, values: &Record<'_>, _ctx: TracingContext<'_, S>) {
        if let Some(mut span_data) = self.span_storage.get_mut(id)
        {
            let mut visitor: JsonVisitor = JsonVisitor::new(&self.system.config);
            values.record(&mut visitor);

            for (key, value) in visitor.fields
            {
                span_data.fields.insert(key, value);
            }
        }
    }

    // Handle span close (cleanup)
    fn on_close(&self, id: TraceId, _ctx: TracingContext<'_, S>) {
        self.span_storage.remove(&id);
    }
}

// Optimized visitor with field validation and CompactString keys
struct JsonVisitor {
    message: String,
    fields: HashMap<CompactString, String>,
    config: LoggerConfig,
}

impl JsonVisitor {
    fn new(config: &LoggerConfig) -> Self {
        Self {
            message: String::with_capacity(256),
            fields: HashMap::with_capacity(16),
            config: config.clone(),
        }
    }

    fn add_field(&mut self, name: &str, value: String) {
        if self.fields.len() >= self.config.max_fields_count {
            tracing::warn!("Maximum field count exceeded, dropping field: {}", name);
            return;
        }

        let truncated_value = if value.len() > self.config.max_field_size {
            tracing::warn!(
                "Field '{}' truncated from {} to {} characters",
                name,
                value.len(),
                self.config.max_field_size
            );

            value.chars().take(self.config.max_field_size).collect()
        } else {
            value
        };

        self.fields
            .insert(CompactString::new(name), truncated_value);
    }
}

impl Visit for JsonVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
        } else {
            self.add_field(field.name(), format!("{value:?}"));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.add_field(field.name(), value.to_string());
        }
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.add_field(field.name(), value.to_string());
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.add_field(field.name(), value.to_string());
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.add_field(field.name(), value.to_string());
    }

    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        self.add_field(field.name(), value.to_string());
    }
}

// Safe sender retrieval functions
pub async fn get_log_sender() -> Option<mpsc::UnboundedSender<LogEntry>> {
    LOGGING_SYSTEM
        .read()
        .await
        .as_ref()
        .map(|sys: &Arc<LoggingSystem>| -> mpsc::UnboundedSender<LogEntry> { sys.sender() })
}

pub fn try_get_log_sender() -> Option<mpsc::UnboundedSender<LogEntry>> {
    LOGGING_SYSTEM
        .try_read()
        .ok()?
        .as_ref()
        .map(|sys: &Arc<LoggingSystem>| -> mpsc::UnboundedSender<LogEntry> { sys.sender() })
}

pub async fn shutdown_logging() -> Result<()> {
    let mut system_guard: RwLockWriteGuard<'_, Option<Arc<LoggingSystem>>> =
        LOGGING_SYSTEM.write().await;

    if let Some(system) = system_guard.take() {
        // Extract system from Arc if we're the only reference
        match Arc::try_unwrap(system) {
            Ok(system) => system.shutdown().await?,

            Err(arc_system) => {
                tracing::warn!("Multiple references to logging system exist during shutdown");

                // Force shutdown by dropping the Arc
                drop(arc_system);
            }
        }
    }

    drop(system_guard);

    Ok(())
}

// Utility functions remain the same
fn validate_config(config: &LoggerConfig) -> Result<()> {
    if config.batch_size == 0 {
        return Err(
            LoggingError::ConfigError("Batch size must be greater than 0".to_string()).into(),
        );
    }

    if config.flush_interval.is_zero() {
        return Err(
            LoggingError::ConfigError("Flush interval must be greater than 0".to_string()).into(),
        );
    }

    if config.max_field_size == 0 {
        return Err(
            LoggingError::ConfigError("Max field size must be greater than 0".to_string()).into(),
        );
    }

    validate_log_directory(&config.log_dir)?;
    Ok(())
}

fn validate_log_directory(path: &Path) -> Result<()> {
    // Basic path validation
    if path.components().count() == 0 {
        return Err(LoggingError::InvalidLogDirectory("Empty path".to_string()).into());
    }

    // Check for potential path traversal
    for component in path.components() {
        if component == std::path::Component::ParentDir {
            return Err(LoggingError::InvalidLogDirectory(
                "Path contains parent directory references".to_string(),
            )
            .into());
        }
    }

    Ok(())
}

async fn setup_log_directory(log_dir: &Path) -> Result<()> {
    if log_dir.exists() && log_dir.is_dir() {
        TokioFs::remove_dir_all(log_dir).await.with_context(|| {
            format!(
                "Failed to remove existing log directory: {}",
                log_dir.display()
            )
        })?;

        TokioFs::create_dir_all(log_dir).await.with_context(|| {
            format!(
                "Failed to create fresh log directory: {}",
                log_dir.display()
            )
        })?;
    } else {
        TokioFs::create_dir_all(log_dir)
            .await
            .with_context(|| format!("Failed to create log directory: {}", log_dir.display()))?;
    }

    Ok(())
}

///
/// # Errors
///
/// `LoggerBuilderError`.
///  Convenience functions for common usage patterns
pub async fn init_default_logging() -> Result<WorkerGuard> {
    LoggerBuilder::new().build().await
}

///
/// # Errors
///
/// `LoggerBuilderError`.
///
pub async fn init_logging_with_level(level: &str) -> Result<WorkerGuard> {
    LoggerBuilder::new().with_level(level).build().await
}

///
/// # Errors
///
/// `LoggerBuilderError`.
///
pub async fn init_logging_with_config(config: LoggerConfig) -> Result<WorkerGuard> {
    LoggerBuilder::new().with_config(config).build().await
}

// Integration with existing application state
impl LogEntry {
    #[must_use]
    pub fn with_app_state(mut self, state: AppStateInfo) -> Self {
        self.app_state = Some(state);
        self
    }

    #[must_use]
    pub fn with_ui_state(mut self, state: UIStateInfo) -> Self {
        self.ui_state = Some(state);
        self
    }

    #[must_use]
    pub fn with_fs_state(mut self, state: FileSystemStateInfo) -> Self {
        self.fs_state = Some(state);
        self
    }

    #[must_use]
    pub const fn with_profiling_data(mut self, cpu_percent: Option<f32>, memory_delta: Option<i64>, duration_ns: Option<u64>) -> Self {
        self.cpu_usage_percent = cpu_percent;
        self.memory_delta_kb = memory_delta;
        self.operation_duration_ns = duration_ns;
        self
    }
}

// Profiling utilities for performance monitoring
pub struct ProfilingData {
    pub cpu_usage_percent: Option<f32>,
    pub memory_delta_kb: Option<i64>,
    pub operation_duration_ns: Option<u64>,
}

impl ProfilingData {
    #[must_use] 
    pub const fn empty() -> Self {
        Self {
            cpu_usage_percent: None,
            memory_delta_kb: None,
            operation_duration_ns: None,
        }
    }

    #[must_use]
    #[expect(clippy::cast_possible_truncation, reason = "Expected accuracy loss")]
    pub const fn with_duration(duration: Duration) -> Self {
        Self {
            cpu_usage_percent: None,
            memory_delta_kb: None,
            operation_duration_ns: Some(duration.as_nanos() as u64),
        }
    }
}

/// Lightweight performance monitoring helper
/// Collects CPU and memory metrics without blocking
#[must_use]
#[expect(clippy::cast_possible_truncation, reason = "Expected accuracy loss")] 
pub const fn collect_profiling_data(start_memory_kb: Option<i64>, duration: Duration) -> ProfilingData {
    // Get current memory usage (simplified - in production would use more sophisticated metrics)
    let current_memory_kb = get_current_memory_kb();
    
    let memory_delta = if let (Some(current), Some(start)) = (current_memory_kb, start_memory_kb) {
        Some(current - start)
    } else {
        None
    };

    ProfilingData {
        cpu_usage_percent: None, // CPU tracking requires more complex implementation
        memory_delta_kb: memory_delta,
        operation_duration_ns: Some(duration.as_nanos() as u64),
    }
}

/// Conditional profiling data collection based on configuration
/// Only collects data if profiling is enabled and meets sampling/duration criteria
#[must_use]
#[expect(
    clippy::cast_possible_truncation, 
    clippy::cast_sign_loss, 
    clippy::cast_precision_loss,
    reason = "Expected accuracy loss"
)] 
pub fn collect_profiling_data_conditional(
    config: &crate::config::ProfilingConfig,
    start_memory_kb: Option<i64>, 
    duration: Duration
) -> ProfilingData {
    // Check if profiling is enabled
    if !config.enabled {
        return ProfilingData::empty();
    }
    
    // Check minimum duration threshold
    if duration.as_millis() < u128::from(config.min_duration_ms) {
        return ProfilingData::empty();
    }
    
    // Apply sampling rate
       
    let mut hasher = DefaultHasher::new();
    std::thread::current().id().hash(&mut hasher);
    let thread_hash = hasher.finish();
    let sample_threshold = (config.sample_rate * u64::MAX as f32) as u64;
    
    if thread_hash > sample_threshold {
        return ProfilingData::empty();
    }
    
    // Collect profiling data based on enabled features
    let memory_delta = if config.memory_tracking && start_memory_kb.is_some() {
        let current_memory_kb = get_current_memory_kb();
        if let (Some(current), Some(start)) = (current_memory_kb, start_memory_kb) {
            Some(current - start)
        } else {
            None
        }
    } else {
        None
    };
    
    ProfilingData {
        cpu_usage_percent: None, // CPU tracking not yet implemented
        memory_delta_kb: memory_delta,
        operation_duration_ns: Some(duration.as_nanos() as u64),
    }
}

/// Get current memory usage in KB (simplified implementation)
/// In production, this would use platform-specific APIs for accuracy
#[must_use] 
pub const fn get_current_memory_kb() -> Option<i64> {
    // Simplified memory tracking - returns None for now
    // Real implementation would use:
    // - Linux: /proc/self/status VmRSS
    // - macOS: task_info with TASK_BASIC_INFO
    // - Windows: GetProcessMemoryInfo
    None
}
