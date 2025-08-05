use std::{
    io::Write,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use compact_str::CompactString;
use serde::{Deserialize, Serialize};
use tokio::{
    fs as TokioFs,
    sync::{Mutex, MutexGuard, mpsc, oneshot},
    time::{self, Interval},
};

use tracing::{
    field::{Field, Visit},
    instrument,
};
use tracing_appender::{
    self as TracingFs,
    non_blocking::{NonBlocking, WorkerGuard},
    rolling::RollingFileAppender,
};

use std::io::Result as StdIoResult;
use tracing_subscriber::{self as TraceSubscriber, Layer, fmt::MakeWriter};

use std::str::FromStr;
use tracing_subscriber::{
    EnvFilter, filter::Directive, layer::SubscriberExt, util::SubscriberInitExt,
};

use dashmap::{DashMap, mapref::one::Ref};
use std::env as StdEnv;
use tracing as Tracer;

pub struct Logger;

static LOG_SEQUENCE: AtomicU64 = AtomicU64::new(1);

const LOG_DIR_NAME: &str = "logs";
const LOG_FILE_NAME: &str = "app_logs";
const LOG_LEVEL: &str = "trace";
const BATCH_SIZE: usize = 100;
const FLUSH_INTERVAL_MS: u64 = 10;

// Pre-allocated constants
const LEVEL_ERROR: CompactString = CompactString::const_new("ERROR");
const LEVEL_WARN: CompactString = CompactString::const_new("WARN");
const LEVEL_INFO: CompactString = CompactString::const_new("INFO");
const LEVEL_DEBUG: CompactString = CompactString::const_new("DEBUG");
const LEVEL_TRACE: CompactString = CompactString::const_new("TRACE");

const UNKNOWN_MARKER: CompactString = CompactString::const_new("UNKNOWN_MARKER");
const UNKNOWN_OPERATION_TYPE: CompactString = CompactString::const_new("UNKNOWN_OPERATION_TYPE");
const UNKNOWN_FILE_PATH: CompactString = CompactString::const_new("UNKNOWN:0");

impl Logger {
    #[instrument]
    pub async fn init_tracing() -> Result<WorkerGuard> {
        let log_dir: PathBuf = StdEnv::current_dir()
            .context("Failed to fetch current working directory through std::env")?
            .join(LOG_DIR_NAME);

        if log_dir.exists() {
            TokioFs::remove_dir_all(&log_dir)
                .await
                .context("Failed to remove existing logs directory")?;
        }

        TokioFs::create_dir(&log_dir)
            .await
            .context("Failed to create logs directory")?;

        // Initialize the async logging system first
        init_logging_system().await?;

        let file: RollingFileAppender = TracingFs::rolling::never(log_dir, LOG_FILE_NAME);
        let (non_blocking, guard) = TracingFs::non_blocking(file);

        let directive: Directive = Directive::from_str(LOG_LEVEL)
            .context("Could not parse LOG_LEVEL provided in logging")?;

        let custom_layer: JsonLayer<NonBlocking> = JsonLayer::new(non_blocking);

        TraceSubscriber::registry()
            .with(custom_layer.with_filter(EnvFilter::from_default_env().add_directive(directive)))
            .init();

        Ok(guard)
    }
}

// Serializable state structs for JSON logging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppStateInfo {
    pub marked_count: usize,
    pub history_count: usize,
    pub plugins_count: usize,
    pub tasks_count: usize,
    pub started_at_ms: u64,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub sequence: u64,
    pub timestamp: DateTime<Utc>,
    pub level: CompactString,
    pub target: CompactString,
    pub marker: CompactString,
    pub operation_type: CompactString,
    pub duration_us: Option<u64>,
    pub source_location: CompactString,
    pub message: CompactString,
    pub app_state: Option<AppStateInfo>,
    pub ui_state: Option<UIStateInfo>,
    pub fs_state: Option<FileSystemStateInfo>,
    pub fields: DashMap<CompactString, String>,
}

impl LogEntry {
    pub fn write_pretty_json<W: Write>(&self, writer: &mut W) -> StdIoResult<()> {
        writeln!(writer, "{{")?;
        writeln!(writer, "  \"sequence\": {},", self.sequence)?;

        writeln!(
            writer,
            "  \"timestamp\": \"{}\",",
            self.timestamp.to_rfc3339()
        )?;

        writeln!(writer, "  \"level\": \"{}\",", self.level)?;
        writeln!(writer, "  \"target\": \"{}\",", self.target)?;
        writeln!(writer, "  \"marker\": \"{}\",", self.marker)?;
        writeln!(writer, "  \"operation_type\": \"{}\",", self.operation_type)?;

        if let Some(duration) = self.duration_us {
            writeln!(writer, "  \"duration_us\": {duration},")?;
        } else {
            writeln!(writer, "  \"duration_us\": null,")?;
        }

        writeln!(
            writer,
            "  \"source_location\": \"{}\",",
            self.source_location
        )?;

        writeln!(writer, "  \"message\": \"{}\",", self.message)?;

        // State objects
        self.state_objects(writer)?;

        // UI state
        self.ui_state(writer)?;

        // FS state
        self.fs_state(writer)?;

        // Additional fields
        write!(writer, "  \"fields\": {{")?;
        let mut first: bool = true;

        for field in &self.fields {
            if !first {
                write!(writer, ",")?;
            }

            first = false;

            writeln!(writer)?;
            write!(writer, "    \"{}\": \"{}\"", field.key(), field.value())?;
        }

        if !first {
            writeln!(writer)?;
            write!(writer, "  ")?;
        }

        writeln!(writer, "}}")?;

        writeln!(writer, "}}")
    }

    #[inline]
    fn state_objects<W: Write>(&self, writer: &mut W) -> StdIoResult<()> {
        match &self.app_state {
            Some(state) => {
                writeln!(writer, "  \"app_state\": {{")?;
                writeln!(writer, "    \"marked_count\": {},", state.marked_count)?;
                writeln!(writer, "    \"history_count\": {},", state.history_count)?;
                writeln!(writer, "    \"plugins_count\": {},", state.plugins_count)?;
                writeln!(writer, "    \"tasks_count\": {},", state.tasks_count)?;
                writeln!(writer, "    \"started_at_ms\": {},", state.started_at_ms)?;

                if let Some(error) = &state.last_error {
                    writeln!(writer, "    \"last_error\": \"{error}\"")?;
                } else {
                    writeln!(writer, "    \"last_error\": null")?;
                }

                writeln!(writer, "  }},")?;
            }

            None => writeln!(writer, "  \"app_state\": null,")?,
        }

        Ok(())
    }

    #[inline]
    fn ui_state<W: Write>(&self, writer: &mut W) -> StdIoResult<()> {
        match &self.ui_state {
            Some(state) => {
                writeln!(writer, "  \"ui_state\": {{")?;

                if let Some(selected) = state.selected {
                    writeln!(writer, "    \"selected\": {selected},")?;
                } else {
                    writeln!(writer, "    \"selected\": null,")?;
                }

                writeln!(
                    writer,
                    "    \"marked_indices_count\": {},",
                    state.marked_indices_count
                )?;

                writeln!(writer, "    \"mode\": \"{}\",", state.mode)?;
                writeln!(writer, "    \"overlay\": \"{}\",", state.overlay)?;
                writeln!(writer, "    \"theme\": \"{}\",", state.theme)?;

                writeln!(
                    writer,
                    "    \"search_results_count\": {},",
                    state.search_results_count
                )?;

                writeln!(
                    writer,
                    "    \"clipboard_overlay_active\":{}",
                    state.clipboard_overlay_active
                )?;

                writeln!(writer, "  }},")?;
            }

            None => writeln!(writer, "  \"ui_state\": null,")?,
        }

        Ok(())
    }

    #[inline]
    fn fs_state<W: Write>(&self, writer: &mut W) -> StdIoResult<()> {
        match &self.fs_state {
            Some(state) => {
                writeln!(writer, "  \"fs_state\": {{")?;
                writeln!(writer, "    \"active_pane\": {},", state.active_pane)?;
                writeln!(writer, "    \"panes_count\": {},", state.panes_count)?;
                writeln!(writer, "    \"current_path\": \"{}\",", state.current_path)?;
                writeln!(writer, "    \"entries_count\": {},", state.entries_count)?;

                if let Some(selected) = state.selected_index {
                    writeln!(writer, "    \"selected_index\": {selected},")?;
                } else {
                    writeln!(writer, "    \"selected_index\": null,")?;
                }

                writeln!(writer, "    \"is_loading\": {},", state.is_loading)?;

                writeln!(
                    writer,
                    "    \"recent_dirs_count\": {},",
                    state.recent_dirs_count
                )?;

                writeln!(
                    writer,
                    "    \"favorite_dirs_count\": {}",
                    state.favorite_dirs_count
                )?;

                writeln!(writer, "  }},")?;
            }

            None => writeln!(writer, "  \"fs_state\": null,")?,
        }

        Ok(())
    }
}

pub struct LoggingSystem {
    sender: mpsc::UnboundedSender<LogEntry>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    task_handle: tokio::task::JoinHandle<()>,
    _writer_guard: WorkerGuard,
}

impl LoggingSystem {
    pub fn new() -> Result<Self> {
        let (log_tx, mut log_rx) = mpsc::unbounded_channel::<LogEntry>();

        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

        // Get writer with guard
        let log_dir: PathBuf = StdEnv::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(LOG_DIR_NAME);
        let file = TracingFs::rolling::never(log_dir, LOG_FILE_NAME);
        let (non_blocking, writer_guard) = TracingFs::non_blocking(file);
        let mut writer = non_blocking.make_writer();

        let task_handle = tokio::spawn(async move {
            let mut batch: Vec<LogEntry> = Vec::with_capacity(BATCH_SIZE);
            let mut interval: Interval = time::interval(Duration::from_millis(FLUSH_INTERVAL_MS));

            loop {
                tokio::select! {
                    // Handle shutdown first (highest priority)
                    _ = &mut shutdown_rx => {
                        // Flush remaining entries
                        if !batch.is_empty() {
                            let _ = flush_batch(&mut batch, &mut writer);
                        }

                        break;
                    }

                    entry = log_rx.recv() => {
                        if let Some(entry) = entry {
                            batch.push(entry);

                            if batch.len() >= BATCH_SIZE {
                                let _ = flush_batch(&mut batch, &mut writer);
                            }
                        } else {
                            // Channel closed, flush and exit
                            if !batch.is_empty() {
                                let _ = flush_batch(&mut batch, &mut writer);
                            }

                            break;
                        }
                    }

                    _ = interval.tick() => {
                        let _ = flush_batch(&mut batch, &mut writer);
                    }
                }
            }
        });

        Ok(Self {
            sender: log_tx,
            shutdown_tx: Some(shutdown_tx),
            task_handle,
            _writer_guard: writer_guard,
        })
    }

    #[must_use]
    pub fn sender(&self) -> mpsc::UnboundedSender<LogEntry> {
        self.sender.clone()
    }

    pub async fn shutdown(mut self) {
        use tokio::time as TokioTime;

        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }

        // Wait for task to complete with timeout
        match TokioTime::timeout(Duration::from_secs(5), self.task_handle).await {
            Ok(result) => {
                if let Err(e) = result {
                    tracing::warn!("Logging task panicked during shutdown: {e}");
                }
            }

            Err(_) => {
                tracing::warn!("Logging task shutdown timeout, forcing abort");
            }
        }
    }
}

// Replace LazyLock with OnceCell for better control
static LOGGING_SYSTEM: Mutex<Option<LoggingSystem>> = Mutex::const_new(None);

pub async fn init_logging_system() -> Result<()> {
    let system: LoggingSystem = LoggingSystem::new()?;
    let mut guard: MutexGuard<'_, Option<LoggingSystem>> = LOGGING_SYSTEM.lock().await;

    if guard.is_some() {
        return Err(anyhow::anyhow!("Logging system already initialized"));
    }

    *guard = Some(system);
    
    drop(guard);

    Ok(())
}

pub fn get_log_sender() -> Option<mpsc::UnboundedSender<LogEntry>> {
    LOGGING_SYSTEM
        .try_lock()
        .ok()?
        .as_ref()
        .map(|sys: &LoggingSystem| sys.sender())
}

pub async fn shutdown_logging() -> Result<()> {
    // Give pending logs time to flush
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    let _ = LOGGING_SYSTEM.lock().await;

    Ok(())
}

fn flush_batch<W: Write>(batch: &mut Vec<LogEntry>, writer: &mut W) -> Result<()> {
    for entry in batch.drain(..) {
        entry.write_pretty_json(writer)?;
    }

    writer.flush().context("Could not flush the writer")
}

pub struct JsonLayer<W> {
    _writer: W, // Keep for compatibility but use async channel
}

impl<W> JsonLayer<W>
where
    W: for<'a> TraceSubscriber::fmt::MakeWriter<'a> + 'static,
{
    pub const fn new(writer: W) -> Self {
        Self { _writer: writer }
    }
}

impl<S, W> Layer<S> for JsonLayer<W>
where
    S: Tracer::Subscriber + for<'lookup> TraceSubscriber::registry::LookupSpan<'lookup>,
    W: for<'a> TraceSubscriber::fmt::MakeWriter<'a> + 'static,
{
    fn on_event(&self, event: &Tracer::Event<'_>, _ctx: TraceSubscriber::layer::Context<'_, S>) {
        use std::fmt::Write;

        let mut visitor: JsonVisitor = JsonVisitor::new();
        event.record(&mut visitor);

        let metadata: &'static tracing::Metadata<'static> = event.metadata();
        let sequence: u64 = LOG_SEQUENCE.fetch_add(1, Ordering::Relaxed);

        // Optimized level matching
        let level: CompactString = match metadata.level().as_str() {
            "ERROR" => LEVEL_ERROR,
            "WARN" => LEVEL_WARN,
            "INFO" => LEVEL_INFO,
            "DEBUG" => LEVEL_DEBUG,
            "TRACE" => LEVEL_TRACE,
            other => CompactString::new(other),
        };

        // Direct field extraction
        let marker: CompactString = visitor
            .fields
            .get("marker")
            .map_or(UNKNOWN_MARKER, |v: Ref<'_, CompactString, String>| {
                CompactString::new(v.value())
            });

        let operation_type: CompactString = visitor.fields.get("operation_type").map_or(
            UNKNOWN_OPERATION_TYPE,
            |v: Ref<'_, CompactString, String>| CompactString::new(v.value()),
        );

        // Optimized source location
        let source_location: CompactString =
            if let (Some(file_path), Some(line_number)) = (metadata.file(), metadata.line()) {
                // Estimate for line number
                let buf_len: usize = file_path.len() + 12;
                let mut source: CompactString = CompactString::with_capacity(buf_len);

                let _ = write!(source, "{file_path}:{line_number}");

                source
            } else {
                UNKNOWN_FILE_PATH
            };

        let duration_us: Option<u64> = visitor
            .fields
            .get("duration_us")
            .and_then(|v: Ref<'_, CompactString, String>| v.value().parse::<u64>().ok());

        let log_entry: LogEntry = LogEntry {
            sequence,
            timestamp: Utc::now(),
            level,
            target: CompactString::new(metadata.target()),
            marker,
            operation_type,
            duration_us,
            source_location,
            message: CompactString::new(&visitor.message),
            app_state: None,
            ui_state: None,
            fs_state: None,
            fields: visitor.fields,
        };

        // Send to async batching system
        if let Some(sender) = get_log_sender() {
            if sender.send(log_entry).is_err() {
                eprintln!("Failed to send log entry: Channel Closed");
            }
        } else {
            println!("Logging system channel is closed, dropping remaining log entries.");
        }
    }
}

struct JsonVisitor {
    message: String,
    fields: DashMap<CompactString, String>,
    debug_buffer: String,
}

impl JsonVisitor {
    fn new() -> Self {
        Self {
            message: String::with_capacity(256),
            fields: DashMap::with_capacity(16),
            debug_buffer: String::with_capacity(128),
        }
    }
}

impl Visit for JsonVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        use std::fmt::Write;

        if field.name() == "message" {
            self.message.clear();

            let _ = write!(self.message, "{value:?}");
        } else {
            self.debug_buffer.clear();

            let _ = write!(self.debug_buffer, "{value:?}");

            self.fields.insert(
                CompactString::new(field.name()),
                String::from(&self.debug_buffer),
            );
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.clear();

            self.message.push_str(value);
        } else {
            self.fields
                .insert(CompactString::new(field.name()), value.to_string());
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields
            .insert(CompactString::new(field.name()), value.to_string());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields
            .insert(CompactString::new(field.name()), value.to_string());
    }
}
