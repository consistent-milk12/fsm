use std::{
    borrow::Cow,
    fs,
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};

use tracing::{Event, Metadata, Subscriber};
use tracing_appender::rolling::{RollingFileAppender, daily};
use tracing_subscriber::{
    EnvFilter, Registry,
    filter::Filtered,
    fmt::{
        self, FmtContext, Layer,
        format::{DefaultFields, FormatEvent, FormatFields, Writer},
    },
    layer::SubscriberExt,
    prelude::*,
    registry::LookupSpan,
};

pub struct Logger;

impl Logger {
    /// Call **once** near the start of `main`.
    pub fn init_tracing() {
        // empty logs/ dir
        let log_dir: &Path = Path::new("logs");

        if log_dir.exists() {
            fs::remove_dir_all(log_dir).expect("cannot remove logs dir");
        }

        fs::create_dir_all(log_dir).expect("cannot create logs dir");

        SEQ.get_or_init(|| AtomicUsize::new(1));
        PROJECT_ROOT
            .get_or_init(|| std::env::current_dir().expect("Failed to get current directory"));

        // daily rolling file appender → logs/app-YYYY-MM-DD.log
        let file: RollingFileAppender = daily("logs", "app");

        let file_layer: Filtered<
            Layer<Registry, DefaultFields, SeqFileMod, RollingFileAppender>,
            EnvFilter,
            Registry,
        > = fmt::layer()
            .event_format(SeqFileMod) // our compact formatter
            .with_writer(file) // write to file
            .with_ansi(false)
            .with_filter(EnvFilter::from_default_env().add_directive("debug".parse().unwrap()));

        tracing_subscriber::registry().with(file_layer).init();
    }
}

static SEQ: OnceLock<AtomicUsize> = OnceLock::new();
static PROJECT_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Custom formatter: `[SEQ] LEVEL [file:line mod::path] message`
struct SeqFileMod;

impl<S, N> FormatEvent<S, N> for SeqFileMod
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut w: Writer<'_>,
        ev: &Event<'_>,
    ) -> std::fmt::Result {
        // monotonically‑increasing sequence number
        let seq: usize = SEQ
            .get()
            .expect("SEQ not initialised")
            .fetch_add(1, Ordering::Relaxed);

        let meta: &'static Metadata<'static> = ev.metadata();
        let file_path_str: &'static str = meta.file().unwrap_or("??");
        let file_path: &Path = Path::new(file_path_str);

        let display_path: Cow<'_, str> = if let Some(root) = PROJECT_ROOT.get() {
            file_path
                .strip_prefix(root)
                .unwrap_or(file_path)
                .to_string_lossy()
        } else {
            file_path.to_string_lossy()
        };

        write!(
            w,
            "{seq:06} {:5} [{}:{}] ",
            meta.level(),
            display_path,
            meta.line().unwrap_or(0),
        )?;

        // write all key‑value pairs for this event (usually just the message)
        ctx.field_format().format_fields(w.by_ref(), ev)?;
        writeln!(w)
    }
}
