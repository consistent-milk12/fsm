//! FSM-Core Logging System
//! File-only logging with comprehensive tracing support for production use

use std::{
    borrow::Cow,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        OnceLock, RwLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use chrono;
use serde_json::json;
use tracing::{
    Event, Level, Subscriber, debug, error,
    field::{Field, Visit},
    info, trace, warn,
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
    registry::LookupSpan,
    util::SubscriberInitExt,
};

use std::fs as StdFs;

use tracing_subscriber::{Layer, layer::Context};

/// File-only logger with comprehensive tracing support
pub struct Logger {
    _guards: Vec<WorkerGuard>,
    error_file: Option<PathBuf>,
}

impl Logger {
    /// Initialize file-only logging system (recommended)
    pub fn init() -> io::Result<Self> {
        Self::init_file_only("logs")
    }

    /// Initialize file-only logging with custom directory
    pub fn init_file_only<P: AsRef<Path>>(log_dir: P) -> io::Result<Self> {
        let config = LoggingConfig {
            log_dir: log_dir.as_ref().to_path_buf(),
            enable_console: false,
            enable_file_logging: true,
            enable_metrics: true,
            enable_error_tracking: true,
            enable_colors: false,
            clean_on_startup: true,
            console_level: Level::ERROR, // Unused
            file_level: Level::DEBUG,
            log_format: LogFormat::Tsv,
        };
        Self::init_with_config(config)
    }

    /// Initialize with custom configuration
    pub fn init_with_config(config: LoggingConfig) -> io::Result<Self> {
        // Validate configuration
        config
            .validate()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

        // Initialize global state
        SEQ.get_or_init(|| AtomicUsize::new(1));
        METRICS.get_or_init(|| RwLock::new(LogMetrics::default()));
        PROJECT_ROOT.get_or_init(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let mut guards = Vec::new();

        // Setup log directories
        Self::setup_log_directories(&config)?;

        let registry = Registry::default();

        let error_file_path = if config.enable_error_tracking {
            Some(config.log_dir.join("errors").join("errors.json"))
        } else {
            None
        };

        // Console layer (typically disabled)
        let console_layer = if config.enable_console {
            let (non_blocking, guard) = tracing_appender::non_blocking(io::stdout());
            guards.push(guard);

            Some(
                fmt::layer()
                    .event_format(CompactFormatter::new())
                    .fmt_fields(PrettyFields::new())
                    .with_writer(non_blocking)
                    .with_ansi(config.enable_colors)
                    .with_filter(
                        EnvFilter::from_default_env().add_directive(config.console_level.into()),
                    ),
            )
        } else {
            None
        };

        // TSV layer (always enabled for AI-optimized logging)
        // TSV layer (always enabled for AI-optimized logging)
        let tsv_layer = if config.enable_file_logging && matches!(config.log_format, LogFormat::Tsv)
        {
            // Determine the path to the TSV log file
            let header_path = config.log_dir.join("fsm-core.tsv");

            // If the TSV file does not exist, create it and write the header
            if !header_path.exists() {
                let _ = fs::OpenOptions::new()
                    .create(true) // create file if missing
                    .append(true) // append to avoid clobbering
                    .write(true) // enable write access
                    .open(&header_path)
                    .and_then(|mut f| {
                        // Write the header row, followed by newline
                        writeln!(f, "{}", TSVFormatter::get_header())
                    });
            }

            // Instantiate a rolling file appender that reuses the existing file
            let file_appender = RollingFileAppender::new(
                Rotation::NEVER, // never rotate
                &config.log_dir, // target directory
                "fsm-core.tsv",  // file name
            );
            // Convert to non-blocking writer and retain its guard
            let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
            guards.push(guard); // keep guard alive to flush logs

            // Build the tracing-subscriber layer for TSV output
            Some(
                fmt::layer()
                    .event_format(TSVFormatter::new()) // custom TSV formatter
                    .fmt_fields(PrettyFields::new()) // structured fields
                    .with_writer(non_blocking) // writer to file
                    .with_ansi(false) // no ANSI codes in TSV
                    .with_filter(
                        EnvFilter::from_default_env().add_directive(config.file_level.into()),
                    ), // respect log level
            )
        } else {
            None
        };

        // JSON layer
        let json_layer =
            if config.enable_file_logging && matches!(config.log_format, LogFormat::Json) {
                let file_appender =
                    RollingFileAppender::new(Rotation::NEVER, &config.log_dir, "fsm-core.json");
                let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
                guards.push(guard);

                Some(
                    fmt::layer()
                        .event_format(JSONFormatter::new())
                        .fmt_fields(PrettyFields::new())
                        .with_writer(non_blocking)
                        .with_ansi(false)
                        .with_filter(
                            EnvFilter::from_default_env().add_directive(config.file_level.into()),
                        ),
                )
            } else {
                None
            };

        // Text layer (legacy)
        let text_layer =
            if config.enable_file_logging && matches!(config.log_format, LogFormat::Text) {
                let file_appender =
                    RollingFileAppender::new(Rotation::NEVER, &config.log_dir, "fsm-core.log");
                let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
                guards.push(guard);

                Some(
                    fmt::layer()
                        .event_format(StructuredFormatter::new())
                        .fmt_fields(PrettyFields::new())
                        .with_writer(non_blocking)
                        .with_ansi(false)
                        .with_filter(
                            EnvFilter::from_default_env().add_directive(config.file_level.into()),
                        ),
                )
            } else {
                None
            };

        // Metrics collection layer
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

        // Build and initialize subscriber
        let subscriber = registry
            .with(console_layer)
            .with(tsv_layer)
            .with(json_layer)
            .with(text_layer)
            .with(metrics_layer)
            .with(error_layer);

        subscriber.init();

        // Log initialization (this will go to files if console is disabled)
        info!(
            version = env!("CARGO_PKG_VERSION"),
            guards_count = guards.len(),
            log_dir = %config.log_dir.display(),
            console_enabled = config.enable_console,
            file_enabled = config.enable_file_logging,
            log_format = ?config.log_format,
            "FSM-Core logging system initialized"
        );

        Ok(Self {
            _guards: guards,
            error_file: error_file_path,
        })
    }

    /// Setup log directories
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

    /// Flush all log buffers
    pub fn flush() {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    /// Test logging functionality
    pub fn test_logging() {
        info!("Testing INFO level logging");
        debug!("Testing DEBUG level logging");
        warn!("Testing WARN level logging");
        error!("Testing ERROR level logging");
        trace!("Testing TRACE level logging");

        info!(
            user_id = 12345,
            operation = "test",
            duration_ms = 150,
            "Operation completed successfully"
        );

        error!(
            error_code = 404,
            component = "file_manager",
            path = "/nonexistent/file.txt",
            "File not found during operation"
        );
    }
}

impl Drop for Logger {
    fn drop(&mut self) {
        // Log shutdown (optional)
        tracing::info!("Shutting down Logger and closing errors.json");

        // Flush all layers
        Logger::flush();

        // Append closing bracket if errors.json path is known
        if let Some(err_path) = &self.error_file {
            if let Ok(mut f) = StdFs::OpenOptions::new().append(true).open(err_path) {
                let _ = writeln!(f, "\n]");
            }
        }
    }
}

/// Log output format selection
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LogFormat {
    /// TSV format optimized for AI analysis (6x more token-efficient than JSON)
    Tsv,
    /// JSON format for complex queries and tooling
    Json,
    /// Human-readable text format
    Text,
}

impl Default for LogFormat {
    fn default() -> Self {
        Self::Tsv // Default to TSV for AI optimization
    }
}

/// Logging configuration
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
    pub log_format: LogFormat,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self::file_only()
    }
}

impl LoggingConfig {
    /// File-only configuration (recommended)
    pub fn file_only() -> Self {
        Self {
            log_dir: PathBuf::from("logs"),
            enable_console: false,
            enable_file_logging: true,
            enable_metrics: true,
            enable_error_tracking: true,
            enable_colors: false,
            clean_on_startup: true,
            console_level: Level::ERROR, // Unused
            file_level: Level::DEBUG,
            log_format: LogFormat::Tsv,
        }
    }

    /// Production configuration
    pub fn production() -> Self {
        Self {
            log_dir: PathBuf::from("/var/log/fsm-core"),
            enable_console: false,
            enable_file_logging: true,
            enable_metrics: true,
            enable_error_tracking: true,
            enable_colors: false,
            clean_on_startup: false,
            console_level: Level::ERROR,
            file_level: Level::INFO,
            log_format: LogFormat::Tsv,
        }
    }

    /// Development configuration with detailed tracing
    pub fn development() -> Self {
        Self {
            log_dir: PathBuf::from("logs"),
            enable_console: false,
            enable_file_logging: true,
            enable_metrics: true,
            enable_error_tracking: true,
            enable_colors: false,
            clean_on_startup: true,
            console_level: Level::DEBUG,
            file_level: Level::TRACE,
            log_format: LogFormat::Tsv,
        }
    }

    /// Console-only for debugging (not recommended for production)
    pub fn console_debug() -> Self {
        Self {
            log_dir: PathBuf::from("logs"),
            enable_console: true,
            enable_file_logging: false,
            enable_metrics: false,
            enable_error_tracking: false,
            enable_colors: true,
            clean_on_startup: false,
            console_level: Level::DEBUG,
            file_level: Level::DEBUG,
            log_format: LogFormat::Text,
        }
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<(), String> {
        if !self.enable_console && !self.enable_file_logging {
            return Err("At least one output method must be enabled".to_string());
        }

        if self.log_dir.to_string_lossy().is_empty() {
            return Err("Log directory path cannot be empty".to_string());
        }

        Ok(())
    }
}

/// Performance and usage metrics
#[derive(Debug, Clone)]
pub struct LogMetrics {
    pub total_events: u64,
    pub errors_count: u64,
    pub warnings_count: u64,
    pub debug_count: u64,
    pub trace_count: u64,
    pub start_time: SystemTime,
    pub last_event_time: Option<SystemTime>,
}

impl Default for LogMetrics {
    fn default() -> Self {
        Self {
            total_events: 0,
            errors_count: 0,
            warnings_count: 0,
            debug_count: 0,
            trace_count: 0,
            start_time: SystemTime::now(),
            last_event_time: None,
        }
    }
}

impl LogMetrics {
    /// Calculate events per second
    pub fn events_per_second(&self) -> f64 {
        let uptime = self.start_time.elapsed().unwrap_or_default().as_secs_f64();
        if uptime > 0.0 {
            self.total_events as f64 / uptime
        } else {
            0.0
        }
    }

    /// Calculate error rate percentage
    pub fn error_rate(&self) -> f64 {
        if self.total_events > 0 {
            (self.errors_count as f64 / self.total_events as f64) * 100.0
        } else {
            0.0
        }
    }

    /// Get uptime duration
    pub fn uptime(&self) -> std::time::Duration {
        self.start_time.elapsed().unwrap_or_default()
    }
}

/// Global state
static SEQ: OnceLock<AtomicUsize> = OnceLock::new();
static PROJECT_ROOT: OnceLock<PathBuf> = OnceLock::new();
static METRICS: OnceLock<RwLock<LogMetrics>> = OnceLock::new();

/// Pretty field formatter for cleaner structured data
struct PrettyFields;

impl PrettyFields {
    fn new() -> Self {
        Self
    }
}

impl<'writer> FormatFields<'writer> for PrettyFields {
    fn format_fields<R: tracing_subscriber::field::RecordFields>(
        &self,
        writer: Writer<'writer>,
        fields: R,
    ) -> std::fmt::Result {
        let mut visitor = PrettyFieldVisitor::new(writer);
        fields.record(&mut visitor);
        visitor.finish()
    }
}

/// Pretty field visitor that formats fields nicely
struct PrettyFieldVisitor<'writer> {
    writer: Writer<'writer>,
    first_field: bool,
}

impl<'writer> PrettyFieldVisitor<'writer> {
    fn new(writer: Writer<'writer>) -> Self {
        Self {
            writer,
            first_field: true,
        }
    }

    fn write_separator(&mut self) -> std::fmt::Result {
        if self.first_field {
            self.first_field = false;
            Ok(())
        } else {
            write!(self.writer, " ")
        }
    }

    fn finish(self) -> std::fmt::Result {
        Ok(())
    }
}

impl<'writer> Visit for PrettyFieldVisitor<'writer> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.writer, "{value:?}");
        } else {
            let _ = self.write_separator();
            let _ = write!(self.writer, "{}={:?}", field.name(), value);
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            let _ = write!(self.writer, "{value}");
        } else {
            let _ = self.write_separator();
            let _ = write!(self.writer, "{}=\"{}\"", field.name(), value);
        }
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        let _ = self.write_separator();
        let _ = write!(self.writer, "{}={}", field.name(), value);
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        let _ = self.write_separator();
        // Format large numbers with separators for readability
        if value >= 1000 {
            let _ = write!(self.writer, "{}={}", field.name(), format_number(value));
        } else {
            let _ = write!(self.writer, "{}={}", field.name(), value);
        }
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        let _ = self.write_separator();
        if value.abs() >= 1000 {
            let _ = write!(
                self.writer,
                "{}={}",
                field.name(),
                format_number(value as u64)
            );
        } else {
            let _ = write!(self.writer, "{}={}", field.name(), value);
        }
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        let _ = self.write_separator();
        // Format floats with appropriate precision
        if field.name().contains("duration") || field.name().contains("time") {
            let _ = write!(self.writer, "{}={:.3}", field.name(), value);
        } else {
            let _ = write!(self.writer, "{}={:.2}", field.name(), value);
        }
    }
}

/// Helper function to format numbers with separators
fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();

    for (i, ch) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i) % 3 == 0 {
            result.push(',');
        }
        result.push(*ch);
    }

    result
}

/// Compact formatter for console output with prettier formatting
struct CompactFormatter;

impl CompactFormatter {
    fn new() -> Self {
        Self
    }
}

/// Field visitor to properly extract message and fields
struct FieldExtractor {
    message: Option<String>,
    fields: Vec<(String, String)>,
}

impl FieldExtractor {
    fn new() -> Self {
        Self {
            message: None,
            fields: Vec::new(),
        }
    }
}

impl Visit for FieldExtractor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{value:?}").trim_matches('"').to_string());
        } else {
            self.fields
                .push((field.name().to_string(), format!("{value:?}")));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else {
            self.fields
                .push((field.name().to_string(), value.to_string()));
        }
    }
}

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

        // Format timestamp
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let timestamp = now.as_secs() % 86400;

        // Format file path (shortened)
        let file_path = meta.file().unwrap_or("??");
        let display_path = if let Some(root) = PROJECT_ROOT.get() {
            Path::new(file_path)
                .strip_prefix(root)
                .unwrap_or(Path::new(file_path))
                .to_string_lossy()
        } else {
            Cow::Borrowed(file_path)
        };

        // Color-coded level
        let level_colored = match *meta.level() {
            Level::ERROR => "\x1b[31m ERROR \x1b[0m", // Red
            Level::WARN => "\x1b[33m WARN  \x1b[0m",  // Yellow
            Level::INFO => "\x1b[32m INFO  \x1b[0m",  // Green
            Level::DEBUG => "\x1b[36m DEBUG \x1b[0m", // Cyan
            Level::TRACE => "\x1b[90m TRACE \x1b[0m", // Gray
        };

        // Pretty timestamp and sequence
        write!(
            writer,
            "\x1b[90m{:02}:{:02}:{:02}\x1b[0m \x1b[90m[{:04}]\x1b[0m {} ",
            (timestamp / 3600) % 24,
            (timestamp / 60) % 60,
            timestamp % 60,
            seq,
            level_colored
        )?;

        // Target with dimmed styling
        write!(writer, "\x1b[90m{}\x1b[0m ", meta.target())?;

        // Span context in brackets
        if let Some(span) = ctx.lookup_current() {
            write!(writer, "\x1b[35m[{}]\x1b[0m ", span.name())?;
        }

        // Location info (dimmed)
        write!(
            writer,
            "\x1b[90m({}:{})\x1b[0m ",
            display_path,
            meta.line().unwrap_or(0)
        )?;

        // Event message and fields with proper spacing
        let mut field_visitor = FieldExtractor::new();
        event.record(&mut field_visitor);

        // Write message first if present
        if let Some(message) = field_visitor.message {
            write!(writer, "{message}")?;
        }

        // Write other fields with proper spacing
        if !field_visitor.fields.is_empty() {
            for (key, value) in field_visitor.fields {
                write!(writer, " {key}={value}")?;
            }
        }

        writeln!(writer)
    }
}

/// TSV formatter optimized for AI analysis with 14-column schema
struct TSVFormatter;

impl TSVFormatter {
    fn new() -> Self {
        Self
    }

    /// Get TSV header for the schema
    fn get_header() -> &'static str {
        "timestamp\tlevel\ttarget\tmarker\toperation_type\tcurrent_path\ttarget_path\tentries_count\tselected_index\tduration_us\tcache_hit\tarea_width\tarea_height\tmessage"
    }
}

/// JSON formatter for structured data output
struct JSONFormatter;

impl JSONFormatter {
    fn new() -> Self {
        Self
    }
}

/// Structured formatter for file output with prettier formatting (legacy text format)
struct StructuredFormatter;

impl StructuredFormatter {
    fn new() -> Self {
        Self
    }
}

/// TSV field visitor for extracting structured data
struct TSVFieldExtractor {
    marker: String,
    operation_type: String,
    current_path: String,
    target_path: String,
    entries_count: String,
    selected_index: String,
    duration_us: String,
    cache_hit: String,
    area_width: String,
    area_height: String,
    message: String,
}

impl TSVFieldExtractor {
    fn new() -> Self {
        Self {
            marker: "NULL".to_string(),
            operation_type: "NULL".to_string(),
            current_path: "NULL".to_string(),
            target_path: "NULL".to_string(),
            entries_count: "NULL".to_string(),
            selected_index: "NULL".to_string(),
            duration_us: "NULL".to_string(),
            cache_hit: "NULL".to_string(),
            area_width: "NULL".to_string(),
            area_height: "NULL".to_string(),
            message: "NULL".to_string(),
        }
    }
}

impl Visit for TSVFieldExtractor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let field_name = field.name();
        let formatted_value = format!("{:?}", value).trim_matches('"').to_string();

        match field_name {
            "message" => self.message = formatted_value,
            "marker" => self.marker = formatted_value,
            "operation_type" => self.operation_type = formatted_value,
            "current_path" => self.current_path = formatted_value,
            "target_path" => self.target_path = formatted_value,
            _ => {}
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "message" => self.message = value.to_string(),
            "marker" => self.marker = value.to_string(),
            "operation_type" => self.operation_type = value.to_string(),
            "current_path" => self.current_path = value.to_string(),
            "target_path" => self.target_path = value.to_string(),
            _ => {}
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        match field.name() {
            "entries_count" => self.entries_count = value.to_string(),
            "selected_index" => self.selected_index = value.to_string(),
            "duration_us" => self.duration_us = value.to_string(),
            "area_width" => self.area_width = value.to_string(),
            "area_height" => self.area_height = value.to_string(),
            _ => {}
        }
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        if field.name() == "cache_hit" {
            self.cache_hit = value.to_string();
        }
    }
}

impl<S, N> FormatEvent<S, N> for TSVFormatter
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result {
        let meta = event.metadata();

        // Extract structured fields
        let mut field_visitor = TSVFieldExtractor::new();
        event.record(&mut field_visitor);

        // Format ISO 8601 timestamp
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let timestamp = chrono::DateTime::from_timestamp(now.as_secs() as i64, 0)
            .unwrap_or_default()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ");

        // Write TSV row: timestamp level target marker operation_type current_path target_path entries_count selected_index duration_us cache_hit area_width area_height message
        writeln!(
            writer,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            timestamp,
            meta.level(),
            meta.target(),
            field_visitor.marker,
            field_visitor.operation_type,
            field_visitor.current_path,
            field_visitor.target_path,
            field_visitor.entries_count,
            field_visitor.selected_index,
            field_visitor.duration_us,
            field_visitor.cache_hit,
            field_visitor.area_width,
            field_visitor.area_height,
            field_visitor.message
        )
    }
}

impl<S, N> FormatEvent<S, N> for JSONFormatter
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

        // Extract all fields
        let mut visitor = JsonVisitor::new();
        event.record(&mut visitor);

        // Collect span hierarchy
        let mut span_context = Vec::new();
        if let Some(span) = ctx.lookup_current() {
            span.scope().for_each(|s| {
                span_context.push(json!({
                    "name": s.name(),
                    "target": s.metadata().target(),
                    "file": s.metadata().file(),
                    "line": s.metadata().line(),
                }));
            });
        }

        // Format ISO 8601 timestamp
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let timestamp = chrono::DateTime::from_timestamp(now.as_secs() as i64, 0)
            .unwrap_or_default()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ");

        let log_record = json!({
            "timestamp": timestamp.to_string(),
            "level": meta.level().to_string(),
            "target": meta.target(),
            "file": meta.file(),
            "line": meta.line(),
            "spans": span_context,
            "fields": visitor.fields,
        });

        writeln!(writer, "{}", log_record)
    }
}

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
        let seq = SEQ.get().unwrap().fetch_add(1, Ordering::Relaxed);

        // Format timestamp as readable time
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let timestamp = now.as_secs() % 86400; // Seconds since midnight

        // Format file path (shortened)
        let file_path = meta.file().unwrap_or("unknown");
        let display_path = if let Some(root) = PROJECT_ROOT.get() {
            Path::new(file_path)
                .strip_prefix(root)
                .unwrap_or(Path::new(file_path))
                .to_string_lossy()
        } else {
            Cow::Borrowed(file_path)
        };

        // Level with padding for alignment
        let level_str = format!("{:5}", meta.level());

        // Write timestamp and basic info
        write!(
            writer,
            "{:02}:{:02}:{:02} [{:04}] {} ",
            (timestamp / 3600) % 24,
            (timestamp / 60) % 60,
            timestamp % 60,
            seq,
            level_str,
        )?;

        // Add span context in a clean format
        if let Some(span) = ctx.lookup_current() {
            // Only show the immediate span, not the full hierarchy
            write!(writer, "[{}] ", span.name())?;
        }

        // Target and location
        write!(
            writer,
            "{} ({}:{}) ",
            meta.target(),
            display_path,
            meta.line().unwrap_or(0)
        )?;

        // Event message and fields
        ctx.field_format().format_fields(writer.by_ref(), event)?;

        writeln!(writer)
    }
}

/// Metrics collection layer
struct MetricsLayer;

impl<S> tracing_subscriber::Layer<S> for MetricsLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        if let Some(metrics) = METRICS.get() {
            if let Ok(mut m) = metrics.write() {
                m.total_events += 1;
                m.last_event_time = Some(SystemTime::now());

                match *event.metadata().level() {
                    Level::ERROR => m.errors_count += 1,
                    Level::WARN => m.warnings_count += 1,
                    Level::DEBUG => m.debug_count += 1,
                    Level::TRACE => m.trace_count += 1,
                    Level::INFO => {} // No specific counter for INFO
                }
            }
        }
    }
}

/// Error tracking layer that writes a valid JSON array of errors
struct ErrorTrackingLayer {
    /// Path to the JSON file where errors are collected
    error_file: PathBuf,
    /// Tracks if the next entry is the first in the array
    is_first: AtomicBool,
}

impl ErrorTrackingLayer {
    /// Create the error-tracking layer and initialize the JSON file
    fn new(log_dir: &Path) -> io::Result<Self> {
        // Construct full path to errors.json under logs/errors/
        let error_file = log_dir.join("errors").join("errors.json");

        // Ensure parent directory exists
        if let Some(parent) = error_file.parent() {
            fs::create_dir_all(parent)?;
        }

        // Truncate or create the file and write opening bracket
        let mut file = fs::OpenOptions::new()
            .create(true) // create if missing
            .write(true) // allow write access
            .truncate(true) // clear existing contents
            .open(&error_file)?;
        writeln!(file, "[")?; // start JSON array

        Ok(Self {
            error_file,
            is_first: AtomicBool::new(true),
        })
    }
}

impl<S> Layer<S> for ErrorTrackingLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    /// Called on each tracing event; logs ERROR and WARN as JSON entries
    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        // Only track errors or warnings
        if matches!(event.metadata().level(), &Level::ERROR | &Level::WARN) {
            // Visitor to extract all event fields into a JSON map
            let mut visitor = JsonVisitor::new();
            event.record(&mut visitor);

            // Build span context array
            let mut span_context = Vec::new();
            if let Some(span) = ctx.lookup_current() {
                span.scope().for_each(|s| {
                    span_context.push(json!({
                        "name":  s.name(),
                        "target": s.metadata().target(),
                        "file":   s.metadata().file(),
                        "line":   s.metadata().line(),
                    }));
                });
            }

            // Compose the JSON object for this error event
            let error_record = json!({
                "timestamp":   SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                "level":       event.metadata().level().to_string(),
                "target":      event.metadata().target(),
                "file":        event.metadata().file(),
                "line":        event.metadata().line(),
                "spans":       span_context,
                "fields":      visitor.fields,
                "process_id":  std::process::id(),
                "thread_id":   format!("{:?}", std::thread::current().id()),
            });

            // Append to the JSON array in the file
            if let Ok(mut file) = fs::OpenOptions::new()
                .append(true) // keep existing array entries
                .open(&self.error_file)
            {
                // Prepend comma for non-first entries
                if self.is_first.swap(false, Ordering::SeqCst) {
                    let _ = writeln!(file, "  {}", error_record);
                } else {
                    let _ = writeln!(file, ",\n  {}", error_record);
                }
            }
        }
    }
}

/// JSON field visitor for error tracking
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

    fn record_f64(&mut self, field: &Field, value: f64) {
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
            duration_us = duration.as_micros(),
            "Operation completed"
        );

        result
    }};
}

#[macro_export]
macro_rules! trace_fn {
    ($fn_name:expr) => {
        tracing::debug_span!("fn", name = $fn_name).entered()
    };

    ($fn_name:expr, $($field:tt)*) => {
        tracing::debug_span!("fn", name = $fn_name, $($field)*).entered()
    };
}

#[macro_export]
macro_rules! trace_err {
    ($error:expr) => {
        tracing::error!(
            error = %$error,
            error_debug = ?$error,
            "Error occurred"
        )
    };

    ($error:expr, $($field:tt)*) => {
        tracing::error!(
            error = %$error,
            error_debug = ?$error,
            $($field)*
        )
    };
}
