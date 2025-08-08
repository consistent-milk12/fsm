use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use compact_str::CompactString;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::io::Write;
use tokio::{
    fs as TokioFs,
    sync::{Mutex, RwLock, mpsc, oneshot},
    time,
};

use tracing::{
    Event, Level, Subscriber,
    field::Visit,
    span::{self, Id as TraceId},
};
use tracing_appender::{
    non_blocking::WorkerGuard,
    rolling::{RollingFileAppender, Rotation},
};
use tracing_subscriber::{
    EnvFilter, Layer,
    filter::Directive,
    layer::{Context as TracingContext, SubscriberExt},
    registry::LookupSpan,
    util::SubscriberInitExt,
};

use crate::config::ProfilingConfig;

const LEVEL_INFO: &str = "INFO";
const LEVEL_DEBUG: &str = "DEBUG";
const LEVEL_WARN: &str = "WARN";
const LEVEL_ERROR: &str = "ERROR";
const LEVEL_TRACE: &str = "TRACE";

const TRACE_MARKER: CompactString = CompactString::const_new("marker");
const TRACE_OPERATION_TYPE: CompactString = CompactString::const_new("operation_type");
const TRACE_ENTER_SPAN: CompactString = CompactString::const_new("ENTER_SPAN");
const TRACE_EXIT_SPAN: CompactString = CompactString::const_new("EXIT_SPAN_COMPLETE");
const TRACE_DURATION: CompactString = CompactString::const_new("duration_us");
const TRACE_SOURCE_UNKNOWN_STR: &str = "UNKNOWN_SOURCE_LOCATION";

const EVENT_ENTER_SPAN: &str = "ENTER_SPAN";
const EVENT_EXIT_SPAN: &str = "EXIT_SPAN_COMPLETE";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggerConfig {
    pub log_dir: PathBuf,
    pub log_file_prefix: CompactString,
    pub batch_size: usize,
    pub flush_interval: Duration,
    pub log_level: CompactString,
    pub max_log_files: usize,
    pub max_field_size: usize,
    pub max_fields_count: usize,
    pub rotation: LogRotation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogRotation {
    Never,
    Daily,
}

impl Default for LoggerConfig {
    fn default() -> Self {
        Self {
            log_dir: PathBuf::from("./logs"),
            log_file_prefix: CompactString::const_new("app"),
            batch_size: 256,
            flush_interval: Duration::from_millis(25),
            log_level: CompactString::const_new("info"),
            max_log_files: 10,
            max_field_size: 2048,
            max_fields_count: 64,
            rotation: LogRotation::Daily,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub sequence: u64,
    pub timestamp: DateTime<Utc>,
    pub level: CompactString,
    pub target: CompactString,
    pub marker: CompactString,
    pub operation_type: CompactString,
    pub source_location: CompactString,
    pub message: String,
    pub duration_us: Option<u64>,
    pub fields: HashMap<CompactString, String>,
}

#[derive(Debug, thiserror::Error)]
pub enum LoggingError {
    #[error("Logger already initialized")]
    AlreadyInitialized,

    #[error("Invalid log directory: {0}")]
    InvalidLogDirectory(String),

    #[error("Failed to create log directory: {0}")]
    DirectoryCreationFailed(#[from] std::io::Error),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Shutdown timeout")]
    ShutdownTimeout,
}

// Send-safe writer wrapper
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
        let mut writer = self.writer.lock().await;
        writer.write_all(buf)
    }

    async fn flush(&self) -> std::io::Result<()> {
        let mut writer = self.writer.lock().await;
        writer.flush()
    }
}

// Minimal visitor limits
#[derive(Debug, Copy, Clone)]
struct VisitorLimits {
    max_fields_count: usize,
    max_field_size: usize,
}

impl From<&LoggerConfig> for VisitorLimits {
    fn from(config: &LoggerConfig) -> Self {
        Self {
            max_fields_count: config.max_fields_count,
            max_field_size: config.max_field_size,
        }
    }
}

// Bounded channel with backpressure
pub struct LoggingSystem {
    sender: mpsc::Sender<LogEntry>,
    shutdown_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    task_handle: Arc<Mutex<Option<tokio::task::JoinHandle<Result<()>>>>>,
    visitor_limits: VisitorLimits,
}

impl LoggingSystem {
    async fn new(
        config: LoggerConfig,
        _profiling_config: ProfilingConfig,
    ) -> Result<(Self, WorkerGuard)> {
        validate_config(&config)?;
        setup_log_directory(&config.log_dir).await?;

        let capacity = config.batch_size * 8;
        let (log_tx, log_rx) = mpsc::channel::<LogEntry>(capacity);
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        let rotation = match config.rotation {
            LogRotation::Never => Rotation::NEVER,
            LogRotation::Daily => Rotation::DAILY,
        };

        let file_appender = RollingFileAppender::builder()
            .rotation(rotation)
            .filename_prefix(config.log_file_prefix.as_str())
            .filename_suffix("jsonl")
            .max_log_files(config.max_log_files)
            .build(&config.log_dir)
            .context("Failed to create file appender")?;

        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
        let task_config = config.clone();
        let safe_writer = SendSafeWriter::new(non_blocking);

        let task_handle = tokio::spawn(async move {
            Self::log_processing_task(log_rx, shutdown_rx, safe_writer, task_config).await
        });

        Ok((
            Self {
                sender: log_tx,
                shutdown_tx: Arc::new(Mutex::new(Some(shutdown_tx))),
                task_handle: Arc::new(Mutex::new(Some(task_handle))),
                visitor_limits: VisitorLimits::from(&config),
            },
            guard,
        ))
    }

    async fn log_processing_task(
        mut log_rx: mpsc::Receiver<LogEntry>,
        mut shutdown_rx: oneshot::Receiver<()>,
        writer: SendSafeWriter,
        config: LoggerConfig,
    ) -> Result<()> {
        let mut batch = Vec::with_capacity(config.batch_size);
        let mut buf = Vec::with_capacity(config.batch_size * 512);
        let mut interval = time::interval(config.flush_interval);

        loop {
            tokio::select! {
                _ = &mut shutdown_rx => {
                    if !batch.is_empty() {
                        let _ = Self::flush_batch_optimized(&batch, &writer, &mut buf).await;
                        let _ = writer.flush().await;
                    }
                    return Ok(());
                }

                entry = log_rx.recv() => {
                    if let Some(entry) = entry {
                        batch.push(entry);

                        if batch.len() >= config.batch_size {
                            let _ = Self::flush_batch_optimized(&batch, &writer, &mut buf).await;
                            let _ = writer.flush().await;
                            batch.clear();
                        }
                    } else {
                        if !batch.is_empty() {
                            let _ = Self::flush_batch_optimized(&batch, &writer, &mut buf).await;
                            let _ = writer.flush().await;
                        }
                        return Ok(());
                    }
                }

                _ = interval.tick() => {
                    if !batch.is_empty() {
                        let _ = Self::flush_batch_optimized(&batch, &writer, &mut buf).await;
                        let _ = writer.flush().await;
                        batch.clear();
                    }
                }
            }
        }
    }

    async fn flush_batch_optimized(
        batch: &[LogEntry],
        writer: &SendSafeWriter,
        buf: &mut Vec<u8>,
    ) -> Result<()> {
        buf.clear();

        for entry in batch {
            if serde_json::to_writer(&mut *buf, entry).is_ok() {
                buf.push(b'\n');
            }
        }

        writer.write_all(buf).await.context("batch write failed")?;
        Ok(())
    }

    #[must_use]
    pub fn sender(&self) -> mpsc::Sender<LogEntry> {
        self.sender.clone()
    }

    pub async fn request_shutdown(&self) -> Result<()> {
        if let Some(tx) = self.shutdown_tx.lock().await.take() {
            let _ = tx.send(());
        }

        if let Some(handle) = self.task_handle.lock().await.take() {
            let _ = tokio::time::timeout(Duration::from_secs(15), handle)
                .await
                .context("Shutdown timeout")?
                .context("Task failed")?;
        }

        Ok(())
    }

    pub async fn send_with_backpressure(
        &self,
        entry: LogEntry,
        level: Level,
    ) -> Result<(), mpsc::error::SendError<LogEntry>> {
        match self.sender.try_send(entry) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(entry)) => {
                if level < Level::WARN {
                    return Ok(());
                }
                self.sender.send(entry).await
            }
            Err(mpsc::error::TrySendError::Closed(entry)) => Err(mpsc::error::SendError(entry)),
        }
    }
}

// Global logging system with proper synchronization
static LOGGING_SYSTEM: RwLock<Option<Arc<LoggingSystem>>> = RwLock::const_new(None);
static LOG_SEQUENCE: AtomicU64 = AtomicU64::new(1);

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

// Logger builder
pub struct LoggerBuilder {
    config: LoggerConfig,
    profiling_config: ProfilingConfig,
}

impl LoggerBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: LoggerConfig::default(),
            profiling_config: ProfilingConfig::default(),
        }
    }

    #[must_use]
    pub fn with_config(mut self, config: LoggerConfig) -> Self {
        self.config = config;
        self
    }

    #[must_use]
    pub const fn with_profiling_config(mut self, profiling_config: ProfilingConfig) -> Self {
        self.profiling_config = profiling_config;
        self
    }

    #[must_use]
    pub fn with_level(mut self, level: &str) -> Self {
        self.config.log_level = CompactString::new(level);
        self
    }

    pub async fn build(self) -> Result<WorkerGuard> {
        let mut system_guard = LOGGING_SYSTEM.write().await;

        if system_guard.is_some() {
            return Err(LoggingError::AlreadyInitialized.into());
        }

        let (system, guard) =
            LoggingSystem::new(self.config.clone(), self.profiling_config.clone()).await?;

        let system_arc = Arc::new(system);
        let level_str = self.config.log_level.clone();
        let make_filter = || -> Result<EnvFilter> {
            Ok(EnvFilter::from_default_env().add_directive(
                Directive::from_str(&level_str).context("Invalid log level in config")?,
            ))
        };

        let json_layer = JsonLayer::new(system_arc.clone(), self.profiling_config.clone())
            .with_filter(make_filter()?);

        let fmt_layer = tracing_subscriber::fmt::layer()
            .with_writer(std::io::sink)
            .with_filter(make_filter()?);

        tracing_subscriber::registry()
            .with(json_layer)
            .with(fmt_layer)
            .try_init()
            .context("Failed to install global tracing subscriber")?;

        *system_guard = Some(system_arc);
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
    pub start_time: Instant,
    pub level: Level,
    pub level_str: CompactString,
    pub target: CompactString,
    pub source_location: CompactString,
    pub marker: CompactString,
    pub operation_type: CompactString,
    pub fields: HashMap<CompactString, String>,
}

impl SpanData {
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn duration_us(&self) -> u64 {
        self.start_time.elapsed().as_micros() as u64
    }
}

pub struct JsonLayer {
    system: Arc<LoggingSystem>,
    span_storage: DashMap<TraceId, SpanData>,
    profiling_config: ProfilingConfig,
    #[cfg(feature = "profiling")]
    profile_collector: Arc<crate::profiling::ProfileCollector>,
}

impl JsonLayer {
    #[must_use]
    pub fn new(system: Arc<LoggingSystem>, profiling_config: ProfilingConfig) -> Self {
        Self {
            system,
            span_storage: DashMap::new(),
            profiling_config,
            #[cfg(feature = "profiling")]
            profile_collector: Arc::new(crate::profiling::ProfileCollector::new()),
        }
    }

    /// Get the profile collector (when profiling feature is enabled)
    #[cfg(feature = "profiling")]
    pub fn profile_collector(&self) -> Arc<crate::profiling::ProfileCollector> {
        self.profile_collector.clone()
    }

    fn should_emit_span_event(level: Level) -> bool {
        level >= Level::INFO
    }

    fn emit_span_event(&self, _id: &TraceId, marker: &str, span_data: &SpanData) {
        if !Self::should_emit_span_event(span_data.level) {
            return;
        }

        let mut entry = LogEntry {
            sequence: 0,
            timestamp: Utc::now(),
            level: CompactString::const_new(""),
            target: CompactString::const_new(""),
            marker: CompactString::const_new(""),
            operation_type: CompactString::const_new(""),
            source_location: CompactString::const_new(""),
            message: String::new(),
            duration_us: None,
            fields: HashMap::new(),
        };

        entry.sequence = LOG_SEQUENCE.fetch_add(1, Ordering::AcqRel);
        entry.timestamp = Utc::now();
        entry.level = span_data.level_str.clone();
        entry.target = span_data.target.clone();
        entry.source_location = span_data.source_location.clone();
        entry.marker = CompactString::new(marker);
        entry.operation_type.clone_from(&span_data.operation_type);

        if marker.ends_with("_COMPLETE") {
            entry.duration_us = Some(span_data.duration_us());
        }

        if !span_data.fields.is_empty() {
            entry.fields = span_data.fields.clone();
        }

        entry.message = format!("Span {marker} {}", span_data.operation_type);

        let sender = self.system.sender();
        match sender.try_send(entry) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(entry)) => {
                let system = self.system.clone();
                let level = span_data.level;
                tokio::spawn(async move {
                    let _ = system.send_with_backpressure(entry, level).await;
                });
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {}
        }
    }

    const fn level_to_compact_string(level: Level) -> CompactString {
        CompactString::const_new(get_level_string(level))
    }
}

#[derive(Default)]
struct SpanFieldMap(HashMap<CompactString, String>);

impl<S> Layer<S> for JsonLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: TracingContext<'_, S>) {
        if let Some(span) = ctx.span(id) {
            let mut visitor = JsonVisitor::get_pooled(self.system.visitor_limits);
            attrs.record(&mut visitor);
            let fields = std::mem::take(&mut visitor.fields);
            span.extensions_mut().insert(SpanFieldMap(fields));
            visitor.return_to_pool();
        }
    }

    fn on_enter(&self, id: &TraceId, ctx: TracingContext<'_, S>) {
        if let Some(span) = ctx.span(id) {
            let metadata = span.metadata();
            let level = *metadata.level();

            if !Self::should_emit_span_event(level) {
                return;
            }

            let fields = span
                .extensions()
                .get::<SpanFieldMap>()
                .map(|map| map.0.clone())
                .unwrap_or_default();

            let marker = fields
                .get(&TRACE_MARKER)
                .map_or(TRACE_ENTER_SPAN, |s| CompactString::new(s));

            let operation_type = fields.get(&TRACE_OPERATION_TYPE).map_or_else(
                || CompactString::const_new("unknown"),
                |s| CompactString::new(s),
            );

            let source_location = CompactString::new(format!(
                "{}:{}",
                metadata.file().unwrap_or(TRACE_SOURCE_UNKNOWN_STR),
                metadata.line().unwrap_or(0)
            ));

            let span_data = SpanData {
                start_time: Instant::now(),
                level,
                level_str: Self::level_to_compact_string(level),
                target: CompactString::new(metadata.target()),
                source_location,
                marker,
                operation_type,
                fields,
            };

            if Self::should_emit_span_event(level) {
                self.emit_span_event(id, EVENT_ENTER_SPAN, &span_data);
            }

            self.span_storage.insert(id.clone(), span_data);
        }
    }

    fn on_exit(&self, id: &TraceId, _ctx: TracingContext<'_, S>) {
        if let Some((_, mut span_data)) = self.span_storage.remove(id) {
            let level = span_data.level;
            span_data.marker = TRACE_EXIT_SPAN;

            if Self::should_emit_span_event(level) {
                self.emit_span_event(id, EVENT_EXIT_SPAN, &span_data);
            }
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: TracingContext<'_, S>) {
        let meta = event.metadata();

        // Skip low-priority (DEBUG/TRACE) when profiling is disabled
        if *meta.level() < Level::INFO && !self.profiling_config.enabled {
            return;
        }

        let mut visitor = JsonVisitor::get_pooled(self.system.visitor_limits);
        event.record(&mut visitor);

        let mut entry = LogEntry {
            sequence: 0,
            timestamp: Utc::now(),
            level: CompactString::const_new(""),
            target: CompactString::const_new(""),
            marker: CompactString::const_new(""),
            operation_type: CompactString::const_new(""),
            source_location: CompactString::const_new(""),
            message: String::new(),
            duration_us: None,
            fields: HashMap::new(),
        };

        entry.sequence = LOG_SEQUENCE.fetch_add(1, Ordering::AcqRel);
        entry.timestamp = Utc::now();
        entry.level = CompactString::const_new(get_level_string(*meta.level()));
        entry.target = CompactString::new(meta.target());

        if let Some(span) = ctx.lookup_current()
            && let Some(span_data) = self.span_storage.get(&span.id())
            && entry.operation_type.is_empty()
        {
            entry.operation_type.clone_from(&span_data.operation_type);
        }

        entry.marker = visitor
            .fields
            .get(&TRACE_MARKER)
            .map(|s| CompactString::new(s))
            .unwrap_or_else(|| CompactString::const_new("EVENT"));

        entry.operation_type = visitor.fields.get(&TRACE_OPERATION_TYPE).map_or_else(
            || CompactString::const_new("unknown"),
            |s| CompactString::new(s),
        );

        entry.duration_us = visitor
            .fields
            .get(&TRACE_DURATION)
            .and_then(|v| v.parse().ok());

        entry.source_location = CompactString::new(format!(
            "{}:{}",
            meta.file().unwrap_or(TRACE_SOURCE_UNKNOWN_STR),
            meta.line().unwrap_or(0)
        ));

        entry.message = std::mem::take(&mut visitor.message);
        entry.fields = std::mem::take(&mut visitor.fields);

        let level = *meta.level();
        let sender = self.system.sender();
        match sender.try_send(entry) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(entry)) => {
                let system = self.system.clone();
                tokio::spawn(async move {
                    let _ = system.send_with_backpressure(entry, level).await;
                });
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {}
        }

        visitor.return_to_pool();
    }

    fn on_close(&self, id: TraceId, _ctx: TracingContext<'_, S>) {
        self.span_storage.remove(&id);
    }
}

// Thread-local visitor pool
thread_local! {
    static VISITOR_POOL: RefCell<Vec<JsonVisitor>> = const { RefCell::new(Vec::new()) };
}

struct JsonVisitor {
    message: String,
    fields: HashMap<CompactString, String>,
    limits: VisitorLimits,
}

impl JsonVisitor {
    fn get_pooled(limits: VisitorLimits) -> Self {
        VISITOR_POOL.with(|pool| {
            let mut pool = pool.borrow_mut();
            if let Some(mut visitor) = pool.pop() {
                visitor.message.clear();
                visitor.fields.clear();
                visitor.limits = limits;
                visitor
            } else {
                Self {
                    message: String::with_capacity(256),
                    fields: HashMap::with_capacity(16),
                    limits,
                }
            }
        })
    }

    fn return_to_pool(self) {
        VISITOR_POOL.with(|pool| {
            let mut pool = pool.borrow_mut();
            if pool.len() < 8 {
                pool.push(self);
            }
        });
    }

    fn add_field(&mut self, name: &str, value: String) {
        if self.fields.len() >= self.limits.max_fields_count {
            return;
        }

        let truncated_value = if value.len() > self.limits.max_field_size {
            value.chars().take(self.limits.max_field_size).collect()
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

pub async fn shutdown_logging() -> Result<()> {
    let mut system_guard = LOGGING_SYSTEM.write().await;

    if let Some(system) = system_guard.take() {
        system.request_shutdown().await?;
    }

    Ok(())
}

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
    if path.components().count() == 0 {
        return Err(LoggingError::InvalidLogDirectory("Empty path".to_string()).into());
    }

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
    if !log_dir.exists() {
        TokioFs::create_dir_all(log_dir)
            .await
            .with_context(|| format!("Failed to create log directory: {}", log_dir.display()))?;
    }
    Ok(())
}

pub async fn init_default_logging() -> Result<WorkerGuard> {
    LoggerBuilder::new().build().await
}

pub async fn init_logging_with_level(level: &str) -> Result<WorkerGuard> {
    LoggerBuilder::new().with_level(level).build().await
}

pub async fn init_logging_with_config(config: LoggerConfig) -> Result<WorkerGuard> {
    LoggerBuilder::new().with_config(config).build().await
}
