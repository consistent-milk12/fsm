//! Practical logging with comprehensive tracing support
//! Simple, effective tracing without over-engineering

use std::{
    borrow::Cow,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        OnceLock, RwLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::json;
use tracing::{
    Event, Level, Subscriber,
    field::{Field, Visit},
};
use tracing_appender::{
    non_blocking::WorkerGuard,
    rolling::{RollingFileAppender, Rotation},
};
use tracing_subscriber::{
    EnvFilter, Registry,
    fmt::{
        self, FmtContext,
        format::{FormatEvent, FormatFields, Writer},
    },
    layer::SubscriberExt,
    prelude::*,
    registry::LookupSpan,
    util::SubscriberInitExt,
};

/// Simple logger with file and console output
pub struct Logger {
    _guards: Vec<WorkerGuard>,
}

impl Logger {
    /// Initialize basic tracing system
    pub fn init() -> io::Result<Self> {
        Self::init_with_config(LoggingConfig::default())
    }

    /// Initialize with custom configuration
    pub fn init_with_config(config: LoggingConfig) -> io::Result<Self> {
        // Initialize global counters
        SEQ.get_or_init(|| AtomicUsize::new(1));
        METRICS.get_or_init(|| RwLock::new(LogMetrics::default()));
        PROJECT_ROOT.get_or_init(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let mut guards: Vec<WorkerGuard> = Vec::new();

        // Setup log directory
        Self::setup_log_directories(&config)?;

        let registry = Registry::default();

        // Console layer
        let console_layer = if config.enable_console {
            let (non_blocking, guard) = tracing_appender::non_blocking(io::stdout());
            guards.push(guard);

            Some(
                fmt::layer()
                    .event_format(CompactFormatter)
                    .with_writer(non_blocking)
                    .with_ansi(config.enable_colors)
                    .with_filter(
                        EnvFilter::from_default_env().add_directive(config.console_level.into()),
                    ),
            )
        } else {
            None
        };

        // File layer
        let file_layer = if config.enable_file_logging {
            let file_appender =
                RollingFileAppender::new(Rotation::DAILY, &config.log_dir, "fsm-core");

            let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
            guards.push(guard);

            Some(
                fmt::layer()
                    .event_format(StructuredFormatter)
                    .with_writer(non_blocking)
                    .with_ansi(false)
                    .with_filter(
                        EnvFilter::from_default_env().add_directive(config.file_level.into()),
                    ),
            )
        } else {
            None
        };

        // Metrics layer
        let metrics_layer = if config.enable_metrics {
            Some(MetricsLayer)
        } else {
            None
        };

        // Error tracking layer
        let error_layer = if config.enable_error_tracking {
            Some(ErrorTrackingLayer::new(&config.log_dir)?)
        } else {
            None
        };

        // Build subscriber
        let subscriber = registry
            .with(console_layer)
            .with(file_layer)
            .with(metrics_layer)
            .with(error_layer);

        subscriber.init();

        tracing::info!(
            version = env!("CARGO_PKG_VERSION"),
            "Tracing system initialized"
        );

        Ok(Self { _guards: guards })
    }

    fn setup_log_directories(config: &LoggingConfig) -> io::Result<()> {
        if config.clean_on_startup && config.log_dir.exists() {
            fs::remove_dir_all(&config.log_dir)?;
        }
        fs::create_dir_all(&config.log_dir)?;
        fs::create_dir_all(config.log_dir.join("errors"))?;
        Ok(())
    }

    /// Get current metrics
    pub fn metrics() -> LogMetrics {
        METRICS
            .get()
            .and_then(|m| m.read().ok())
            .map(|m| m.clone())
            .unwrap_or_default()
    }

    /// Flush logs
    pub fn flush() {
        tracing::info!("Flushing log buffers");
        // Force a small delay to allow async writers to flush
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Simple logging configuration
#[derive(Debug, Clone)]
pub struct LoggingConfig {
    pub log_dir: PathBuf,
    pub enable_console: bool,
    pub enable_file_logging: bool,
    pub enable_metrics: bool,
    pub enable_error_tracking: bool,
    pub enable_colors: bool,
    pub clean_on_startup: bool,
    pub console_level: Level,
    pub file_level: Level,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            log_dir: PathBuf::from("logs"),
            enable_console: true,
            enable_file_logging: true,
            enable_metrics: true,
            enable_error_tracking: true,
            enable_colors: true,
            clean_on_startup: true,
            console_level: Level::INFO,
            file_level: Level::DEBUG,
        }
    }
}

/// Simple metrics tracking
#[derive(Debug, Clone)]
pub struct LogMetrics {
    pub total_events: u64,
    pub errors_count: u64,
    pub start_time: SystemTime,
}

impl Default for LogMetrics {
    fn default() -> Self {
        Self {
            total_events: 0,
            errors_count: 0,
            start_time: SystemTime::now(),
        }
    }
}

/// Global state
static SEQ: OnceLock<AtomicUsize> = OnceLock::new();
static PROJECT_ROOT: OnceLock<PathBuf> = OnceLock::new();
static METRICS: OnceLock<RwLock<LogMetrics>> = OnceLock::new();

/// Compact formatter for console
struct CompactFormatter;

impl<S, N> FormatEvent<S, N> for CompactFormatter
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result {
        let seq = SEQ.get().unwrap().fetch_add(1, Ordering::Relaxed);
        let meta = event.metadata();

        let file_path = meta.file().unwrap_or("??");
        let display_path = if let Some(root) = PROJECT_ROOT.get() {
            Path::new(file_path)
                .strip_prefix(root)
                .unwrap_or(Path::new(file_path))
                .to_string_lossy()
        } else {
            Cow::Borrowed(file_path)
        };

        write!(
            writer,
            "[{:06}] {:5} [{}:{}] {}: ",
            seq,
            meta.level(),
            display_path,
            meta.line().unwrap_or(0),
            meta.target()
        )?;

        // Add span context if available
        if let Some(span) = ctx.lookup_current() {
            write!(writer, "[{}] ", span.name())?;
        }

        ctx.field_format().format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

/// Structured formatter for files
struct StructuredFormatter;

impl<S, N> FormatEvent<S, N> for StructuredFormatter
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result {
        let meta = event.metadata();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();

        write!(
            writer,
            "ts={} level={} target={} ",
            timestamp,
            meta.level(),
            meta.target()
        )?;

        if let Some(file) = meta.file() {
            write!(writer, "file={} ", file)?;
        }
        if let Some(line) = meta.line() {
            write!(writer, "line={} ", line)?;
        }

        // Add span context
        if let Some(span) = ctx.lookup_current() {
            write!(writer, "span={} ", span.name())?;
        }

        ctx.field_format().format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

/// Simple metrics collection layer
struct MetricsLayer;

impl<S> tracing_subscriber::Layer<S> for MetricsLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        if let Some(metrics) = METRICS.get() {
            if let Ok(mut m) = metrics.write() {
                m.total_events += 1;
                if event.metadata().level() == &Level::ERROR {
                    m.errors_count += 1;
                }
            }
        }
    }
}

/// Error tracking layer
struct ErrorTrackingLayer {
    error_file: PathBuf,
}

impl ErrorTrackingLayer {
    fn new(log_dir: &Path) -> io::Result<Self> {
        Ok(Self {
            error_file: log_dir.join("errors").join("errors.jsonl"),
        })
    }
}

impl<S> tracing_subscriber::Layer<S> for ErrorTrackingLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_event(&self, event: &Event<'_>, ctx: tracing_subscriber::layer::Context<'_, S>) {
        if event.metadata().level() == &Level::ERROR {
            let mut visitor = JsonVisitor::new();
            event.record(&mut visitor);

            let span_context = ctx.lookup_current().map(|span| span.name());

            let error_record = json!({
                "timestamp": SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                "level": "ERROR",
                "target": event.metadata().target(),
                "file": event.metadata().file(),
                "line": event.metadata().line(),
                "span": span_context,
                "fields": visitor.fields,
            });

            // Write to error file (best effort)
            if let Ok(mut file) = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.error_file)
            {
                let _ = writeln!(file, "{}", error_record);
            }
        }
    }
}

/// JSON field visitor
struct JsonVisitor {
    fields: serde_json::Map<String, serde_json::Value>,
}

impl JsonVisitor {
    fn new() -> Self {
        Self {
            fields: serde_json::Map::new(),
        }
    }
}

impl Visit for JsonVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.fields
            .insert(field.name().to_string(), json!(format!("{:?}", value)));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields.insert(field.name().to_string(), json!(value));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields.insert(field.name().to_string(), json!(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields.insert(field.name().to_string(), json!(value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields.insert(field.name().to_string(), json!(value));
    }
}

/// Convenience macros for operation tracing
#[macro_export]
macro_rules! trace_operation {
    ($operation_name:expr) => {
        tracing::info_span!($operation_name).entered()
    };
    ($operation_name:expr, $($field:tt)*) => {
        tracing::info_span!($operation_name, $($field)*).entered()
    };
}

#[macro_export]
macro_rules! measure_time {
    ($name:expr, $block:expr) => {{
        let start = std::time::Instant::now();
        let result = $block;
        let duration = start.elapsed();
        tracing::info!(
            operation = $name,
            duration_ms = duration.as_millis(),
            "Operation completed"
        );
        result
    }};
}

#[macro_export]
macro_rules! trace_fn {
    ($fn_name:expr) => {
        tracing::info_span!("fn", name = $fn_name).entered()
    };
    ($fn_name:expr, $($field:tt)*) => {
        tracing::info_span!("fn", name = $fn_name, $($field)*).entered()
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tracing::{debug, error, info};

    #[test]
    fn test_logger_initialization() {
        let temp_dir = TempDir::new().unwrap();
        let config = LoggingConfig {
            log_dir: temp_dir.path().to_path_buf(),
            clean_on_startup: false,
            ..Default::default()
        };

        let _logger = Logger::init_with_config(config).unwrap();

        info!("Test info message");
        error!(error_code = 404, "Test error message");
        debug!(user_id = 123, action = "test", "Debug message");

        let metrics = Logger::metrics();
        assert!(metrics.total_events > 0);
    }
}
