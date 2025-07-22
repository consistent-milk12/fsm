use std::{
    fs,
    path::Path,
    sync::OnceLock,
    sync::atomic::{AtomicUsize, Ordering},
};

use tracing::Metadata;
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
};

pub struct Logger;

impl Logger {
    /// Call **once** near the start of `main`.
    pub fn init_tracing() {
        // create logs/ if missing
        let log_dir: &Path = Path::new("logs");
        fs::create_dir_all(log_dir).expect("cannot create logs dir");

        SEQ.get_or_init(|| AtomicUsize::new(1));

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
            .with_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()));

        // optional stderr layer for live debugging
        let stderr_layer = fmt::layer()
            .event_format(SeqFileMod)
            .with_writer(std::io::stderr)
            .with_ansi(true)
            .with_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()));

        tracing_subscriber::registry()
            .with(file_layer)
            .with(stderr_layer)
            .init();
    }
}

static SEQ: OnceLock<AtomicUsize> = OnceLock::new();

/// Custom formatter: `[SEQ] LEVEL [file:line mod::path] message`
struct SeqFileMod;

impl<S, N> FormatEvent<S, N> for SeqFileMod
where
    S: tracing::Subscriber + for<'lookup> tracing_subscriber::registry::LookupSpan<'lookup>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut w: Writer<'_>,
        ev: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        // monotonically‑increasing sequence number
        let seq: usize = SEQ
            .get()
            .expect("SEQ not initialised")
            .fetch_add(1, Ordering::Relaxed);

        let meta: &'static Metadata<'static> = ev.metadata();
        write!(
            w,
            "{seq:06} {:5} [{}:{} {}] ",
            meta.level(),
            meta.file().unwrap_or("??"),
            meta.line().unwrap_or(0),
            meta.module_path().unwrap_or("???"),
        )?;

        // write all key‑value pairs for this event (usually just the message)
        ctx.field_format().format_fields(w.by_ref(), ev)?;
        writeln!(w)
    }
}
