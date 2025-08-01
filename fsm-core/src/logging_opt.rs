use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{
        Arc, LazyLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use compact_str::{CompactString, ToCompactString};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::str::FromStr;
use tokio::{
    fs as TokioFs,
    sync::{Mutex, MutexGuard, RwLock, mpsc, oneshot},
    time,
};
use tracing::{Level, field::Visit};
use tracing_appender::{
    non_blocking::WorkerGuard,
    rolling::{RollingFileAppender, Rotation},
};
use tracing_subscriber::{
    EnvFilter, Layer, filter::Directive, layer::SubscriberExt, util::SubscriberInitExt,
};

// Pre-allocated string constants with static lifetime for zero-allocation access
static LEVEL_STRINGS: LazyLock<[&'static str; 5]> =
    LazyLock::new(|| ["ERROR", "WARN", "INFO", "DEBUG", "TRACE"]);

static COMMON_MARKERS: LazyLock<std::collections::HashMap<&'static str, &'static str>> =
    LazyLock::new(|| {
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
        .cloned()
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
    .cloned()
    .collect()
});

// Fast string interning for frequently used strings - Fixed with LazyLock
struct StringInterner {
    strings: RwLock<HashMap<String, &'static str>>,
}

impl StringInterner {
    fn new() -> Self {
        Self {
            strings: RwLock::const_new(HashMap::new()),
        }
    }

    async fn intern(&self, s: &str) -> &'static str {
        // Try read-only access first
        if let Ok(guard) = self.strings.try_read() {
            if let Some(&interned) = guard.get(s) {
                return interned;
            }
        }

        // Need to write
        let mut guard = self.strings.write().await;
        if let Some(&interned) = guard.get(s) {
            return interned;
        }

        // Create new interned string
        let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
        guard.insert(s.to_string(), leaked);
        leaked
    }
}

// Fix: Use LazyLock for proper static initialization
static STRING_INTERNER: LazyLock<StringInterner> = LazyLock::new(StringInterner::new);

// Configuration with performance-oriented defaults
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggerConfig {
    pub log_dir: PathBuf,
    pub log_file_prefix: CompactString, // Use CompactString for short prefixes
    pub batch_size: usize,
    pub flush_interval: Duration,
    pub log_level: CompactString, // Use CompactString for log level
    pub max_log_file_size: u64,
    pub max_log_files: usize,
    pub max_field_size: usize,
    pub max_fields_count: usize,
    pub rotation: LogRotation,
    pub enable_console_output: bool,
    pub use_string_interning: bool, // New: Enable/disable string interning
    pub preallocate_buffers: bool,  // New: Preallocate serialization buffers
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
            log_level: CompactString::const_new("info"),
            max_log_file_size: 50 * 1024 * 1024, // Increased to 50MB
            max_log_files: 10,                   // Increased retention
            max_field_size: 2048,                // Increased field size limit
            max_fields_count: 64,                // Increased field count
            rotation: LogRotation::Daily,
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
    pub last_error: Option<CompactString>, // Use CompactString for error messages
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

    async fn get(&self) -> LogEntry {
        let mut pool = self.pool.lock().await;
        pool.pop_front().unwrap_or_else(|| self.template.clone())
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
        entry.app_state = None;
        entry.ui_state = None;
        entry.fs_state = None;
        entry.fields.clear();

        let mut pool: MutexGuard<'_, VecDeque<LogEntry>> = self.pool.lock().await;
        if pool.len() < self.max_size {
            pool.push_back(entry);
        }
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
        let mut writer: MutexGuard<'_, Box<dyn Write + Send>> = self.writer.lock().await;
        writer.write_all(buf)
    }

    async fn flush(&self) -> std::io::Result<()> {
        let mut writer: MutexGuard<'_, Box<dyn Write + Send>> = self.writer.lock().await;
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

        let (log_tx, log_rx) = mpsc::unbounded_channel::<LogEntry>();
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

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
            .filename_suffix("json")
            .max_log_files(config.max_log_files)
            .build(&config.log_dir)
            .context("Failed to create file appender")?;

        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

        let entry_pool: Arc<LogEntryPool> = Arc::new(LogEntryPool::new(config.batch_size * 4)); // Larger pool
        let task_pool: Arc<LogEntryPool> = entry_pool.clone();
        let task_config: LoggerConfig = config.clone();

        // Pre-allocate JSON buffer if enabled
        let json_buffer = if config.preallocate_buffers {
            Arc::new(Mutex::new(Vec::with_capacity(64 * 1024))) // 64KB buffer
        } else {
            Arc::new(Mutex::new(Vec::new()))
        };

        let task_json_buffer: Arc<Mutex<Vec<u8>>> = json_buffer.clone();

        // Fix: Create Send-safe writer
        let safe_writer: SendSafeWriter = SendSafeWriter::new(non_blocking);

        let task_handle = tokio::spawn(async move {
            Self::log_processing_task(
                log_rx,
                shutdown_rx,
                safe_writer,
                task_config,
                task_pool,
                task_json_buffer,
            )
            .await
        });

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
                        if let Ok(()) = Self::flush_batch_optimized(&batch, &writer, &json_buffer).await {
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
                    match entry {
                        Some(entry) => {
                            batch.push(entry);

                            if batch.len() >= config.batch_size {
                                if let Ok(()) = Self::flush_batch_optimized(&batch, &writer, &json_buffer).await {
                                    let _ = writer.flush().await;
                                }

                                // Return entries to pool
                                for entry in batch.drain(..) {
                                    pool.return_entry(entry).await;
                                }
                            }
                        }

                        None => {
                            tracing::info!("Log channel closed, flushing remaining entries");

                            if !batch.is_empty() {
                                if let Ok(()) = Self::flush_batch_optimized(&batch, &writer, &json_buffer).await {
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
                }

                // Periodic flush
                _ = interval.tick() => {
                    if !batch.is_empty() {
                        if let Ok(()) = Self::flush_batch_optimized(&batch, &writer, &json_buffer).await {
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

    // Fix: Use Send-safe writer
    async fn flush_batch_optimized(
        batch: &[LogEntry],
        writer: &SendSafeWriter,
        json_buffer: &Arc<Mutex<Vec<u8>>>,
    ) -> Result<()> {
        let mut buffer_guard: MutexGuard<'_, Vec<u8>> = json_buffer.lock().await;
        buffer_guard.clear();

        for entry in batch {
            match serde_json::to_writer(&mut *buffer_guard, entry) {
                Ok(()) => {
                    buffer_guard.push(b'\n');
                }

                Err(e) => {
                    tracing::error!("Failed to serialize log entry: {}", e);
                    continue;
                }
            }
        }

        writer
            .write_all(&buffer_guard)
            .await
            .context("Failed to write batch to log file")?;

        Ok(())
    }

    pub fn sender(&self) -> mpsc::UnboundedSender<LogEntry> {
        self.sender.clone()
    }

    pub async fn shutdown(mut self) -> Result<()> {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }

        if let Some(handle) = self.task_handle.take() {
            match tokio::time::timeout(Duration::from_secs(15), handle).await {
                // Increased timeout
                Ok(result) => match result {
                    Ok(task_result) => task_result.context("Log processing task failed")?,
                    Err(e) => {
                        tracing::error!("Log processing task panicked: {}", e);

                        return Err(LoggingError::ShutdownTimeout.into());
                    }
                },
                Err(_) => {
                    tracing::error!("Logging system shutdown timeout");

                    return Err(LoggingError::ShutdownTimeout.into());
                }
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
fn get_level_string(level: Level) -> &'static str {
    match level {
        Level::ERROR => LEVEL_STRINGS[0],
        Level::WARN => LEVEL_STRINGS[1],
        Level::INFO => LEVEL_STRINGS[2],
        Level::DEBUG => LEVEL_STRINGS[3],
        Level::TRACE => LEVEL_STRINGS[4],
    }
}

#[inline]
async fn get_marker_string(marker: &str, use_interning: bool) -> CompactString {
    if let Some(&interned) = COMMON_MARKERS.get(marker) {
        CompactString::const_new(interned)
    } else if use_interning {
        CompactString::new(STRING_INTERNER.intern(marker).await)
    } else {
        CompactString::new(marker)
    }
}

#[inline]
async fn get_operation_string(operation: &str, use_interning: bool) -> CompactString {
    if let Some(&interned) = COMMON_OPERATIONS.get(operation) {
        CompactString::const_new(interned)
    } else if use_interning {
        CompactString::new(STRING_INTERNER.intern(operation).await)
    } else {
        CompactString::new(operation)
    }
}

// Logger builder for clean initialization
pub struct LoggerBuilder {
    config: LoggerConfig,
}

impl LoggerBuilder {
    pub fn new() -> Self {
        Self {
            config: LoggerConfig::default(),
        }
    }

    pub fn with_config(mut self, config: LoggerConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_directory<P: AsRef<Path>>(mut self, dir: P) -> Self {
        self.config.log_dir = dir.as_ref().to_path_buf();
        self
    }

    pub fn with_level(mut self, level: &str) -> Self {
        self.config.log_level = CompactString::new(level);
        self
    }

    pub fn with_batch_size(mut self, size: usize) -> Self {
        self.config.batch_size = size;
        self
    }

    pub fn with_flush_interval(mut self, interval: Duration) -> Self {
        self.config.flush_interval = interval;
        self
    }

    pub fn with_rotation(mut self, rotation: LogRotation) -> Self {
        self.config.rotation = rotation;
        self
    }

    pub fn with_string_interning(mut self, enable: bool) -> Self {
        self.config.use_string_interning = enable;
        self
    }

    pub fn with_buffer_preallocation(mut self, enable: bool) -> Self {
        self.config.preallocate_buffers = enable;
        self
    }

    pub async fn build(self) -> Result<WorkerGuard> {
        let mut system_guard = LOGGING_SYSTEM.write().await;

        if system_guard.is_some() {
            return Err(LoggingError::AlreadyInitialized.into());
        }

        let (system, guard) = LoggingSystem::new(self.config.clone()).await?;
        let system_arc: Arc<LoggingSystem> = Arc::new(system);

        // Initialize tracing subscriber
        let directive: Directive =
            Directive::from_str(self.config.log_level.as_str()).context("Invalid log level")?;

        let custom_layer: JsonLayer = JsonLayer::new(system_arc.clone());
        let subscriber = tracing_subscriber::registry()
            .with(custom_layer.with_filter(EnvFilter::from_default_env().add_directive(directive)));

        // if self.config.enable_console_output {
        //     let _ = subscriber.with(tracing_subscriber::fmt::layer()).try_init();
        // } else {
        //     let _ = subscriber.try_init();
        // }

        let _ = subscriber.try_init();

        *system_guard = Some(system_arc);

        Ok(guard)
    }
}

impl Default for LoggerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// Optimized JSON layer
pub struct JsonLayer {
    system: Arc<LoggingSystem>,
}

struct EventData {
    level: Level,
    target: String,
    file: Option<String>,
    line: Option<u32>,
    message: String,
    fields: std::collections::HashMap<CompactString, String>,
}

impl JsonLayer {
    pub fn new(system: Arc<LoggingSystem>) -> Self {
        Self { system }
    }
}

impl<S> Layer<S> for JsonLayer
where
    S: tracing::Subscriber + for<'lookup> tracing_subscriber::registry::LookupSpan<'lookup>,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut visitor: JsonVisitor = JsonVisitor::new(&self.system.config);
        event.record(&mut visitor);
        let metadata: &'static tracing::Metadata<'static> = event.metadata();

        let event_data: EventData = EventData {
            level: *metadata.level(),
            target: metadata.target().to_string(),
            file: metadata.file().map(str::to_string),
            line: metadata.line(),
            message: visitor.message,
            fields: visitor.fields,
        };

        let sender: mpsc::UnboundedSender<LogEntry> = self.system.sender();
        let system: Arc<LoggingSystem> = Arc::clone(&self.system);

        // Use spawn_blocking for CPU-intensive work to avoid blocking the async runtime
        tokio::spawn(async move {
            if let Err(e) = Self::process_event(event_data, sender, system).await {
                eprintln!("Failed to process log event: {e}");
            }
        });
    }
}

impl JsonLayer {
    async fn process_event(
        event_data: EventData,
        sender: mpsc::UnboundedSender<LogEntry>,
        system: Arc<LoggingSystem>,
    ) -> Result<()> {
        let sequence: u64 = LOG_SEQUENCE.fetch_add(1, Ordering::Relaxed);

        // Get reusable entry from pool
        let mut entry: LogEntry = system.entry_pool.get().await;

        entry.sequence = sequence;
        entry.timestamp = Utc::now();
        entry.level = get_level_string(event_data.level).to_compact_string();
        entry.target = CompactString::new(&event_data.target);

        // Use optimized string lookup
        entry.marker =
            if let Some(marker_str) = event_data.fields.get(&CompactString::const_new("marker")) {
                get_marker_string(marker_str, system.config.use_string_interning).await
            } else {
                CompactString::const_new("unknown")
            };

        entry.operation_type = if let Some(op_str) = event_data
            .fields
            .get(&CompactString::const_new("operation_type"))
        {
            get_operation_string(op_str, system.config.use_string_interning).await
        } else {
            CompactString::const_new("unknown")
        };

        entry.duration_us = event_data
            .fields
            .get(&CompactString::const_new("duration_us"))
            .and_then(|v| v.parse().ok());

        entry.source_location = CompactString::new(format!(
            "{}:{}",
            event_data.file.unwrap_or_else(|| "unknown".to_string()),
            event_data.line.unwrap_or(0)
        ));

        entry.message = event_data.message;
        entry.fields = event_data.fields;

        sender.send(entry).map_err(|_| {
            anyhow::anyhow!(
                "Failed to send log entry:
  channel closed"
            )
        })?;

        Ok(())
    }
}

// Optimized visitor with field validation and CompactString keys
struct JsonVisitor {
    message: String,
    fields: std::collections::HashMap<CompactString, String>,
    config: LoggerConfig,
}

impl JsonVisitor {
    fn new(config: &LoggerConfig) -> Self {
        Self {
            message: String::with_capacity(256),
            fields: std::collections::HashMap::with_capacity(16),
            config: config.clone(),
        }
    }

    fn add_field(&mut self, name: &str, value: String) {
        if self.fields.len() >= self.config.max_fields_count {
            tracing::warn!(
                "Maximum field count exceeded, dropping
  field: {}",
                name
            );
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
    LOGGING_SYSTEM.read().await.as_ref().map(|sys| sys.sender())
}

pub fn try_get_log_sender() -> Option<mpsc::UnboundedSender<LogEntry>> {
    LOGGING_SYSTEM
        .try_read()
        .ok()?
        .as_ref()
        .map(|sys| sys.sender())
}

pub async fn shutdown_logging() -> Result<()> {
    let mut system_guard = LOGGING_SYSTEM.write().await;

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
        if let std::path::Component::ParentDir = component {
            return Err(LoggingError::InvalidLogDirectory(
                "Path contains parent directory references".to_string(),
            )
            .into());
        }
    }

    Ok(())
}

async fn setup_log_directory(log_dir: &Path) -> Result<()> {
    if log_dir.exists() {
        if !log_dir.is_dir() {
            return Err(LoggingError::InvalidLogDirectory(
                "Log path exists but is not a directory".to_string(),
            )
            .into());
        }
    } else {
        TokioFs::create_dir_all(log_dir)
            .await
            .with_context(|| format!("Failed to create log directory: {}", log_dir.display()))?;
    }
    Ok(())
}

// Convenience functions for common usage patterns
pub async fn init_default_logging() -> Result<WorkerGuard> {
    LoggerBuilder::new().build().await
}

pub async fn init_logging_with_level(level: &str) -> Result<WorkerGuard> {
    LoggerBuilder::new().with_level(level).build().await
}

pub async fn init_logging_with_config(config: LoggerConfig) -> Result<WorkerGuard> {
    LoggerBuilder::new().with_config(config).build().await
}

// Integration with existing application state
impl LogEntry {
    pub fn with_app_state(mut self, state: AppStateInfo) -> Self {
        self.app_state = Some(state);
        self
    }

    pub fn with_ui_state(mut self, state: UIStateInfo) -> Self {
        self.ui_state = Some(state);
        self
    }

    pub fn with_fs_state(mut self, state: FileSystemStateInfo) -> Self {
        self.fs_state = Some(state);
        self
    }
}
