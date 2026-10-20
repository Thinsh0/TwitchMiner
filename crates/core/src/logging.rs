//! Logging: rolling files in `<data>/logs`, optional stdout, and an in-memory
//! ring buffer + live stream that UIs can display.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use chrono::{DateTime, Utc};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::field::{Field, Visit};
use tracing_appender::non_blocking::NonBlocking;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

use crate::paths::Paths;

const BUFFER_LINES: usize = 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogLine {
    pub ts: DateTime<Utc>,
    pub level: String,
    pub target: String,
    pub message: String,
}

#[derive(Clone)]
pub struct LogBuffer {
    lines: Arc<Mutex<VecDeque<LogLine>>>,
    tx: broadcast::Sender<LogLine>,
}

impl Default for LogBuffer {
    fn default() -> Self {
        let (tx, _) = broadcast::channel(512);
        Self {
            lines: Arc::new(Mutex::new(VecDeque::with_capacity(BUFFER_LINES))),
            tx,
        }
    }
}

impl LogBuffer {
    pub fn recent(&self, n: usize) -> Vec<LogLine> {
        let lines = self.lines.lock();
        lines
            .iter()
            .skip(lines.len().saturating_sub(n))
            .cloned()
            .collect()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LogLine> {
        self.tx.subscribe()
    }

    fn push(&self, line: LogLine) {
        {
            let mut lines = self.lines.lock();
            if lines.len() == BUFFER_LINES {
                lines.pop_front();
            }
            lines.push_back(line.clone());
        }
        let _ = self.tx.send(line);
    }
}

struct BufferLayer(LogBuffer);

#[derive(Default)]
struct MessageVisitor {
    message: String,
    fields: String,
}

impl Visit for MessageVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            let _ = write!(self.fields, " {}={}", field.name(), value);
        }
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            let _ = write!(self.fields, " {}={:?}", field.name(), value);
        }
    }
}

impl<S: tracing::Subscriber> Layer<S> for BufferLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let mut v = MessageVisitor::default();
        event.record(&mut v);
        let meta = event.metadata();
        self.0.push(LogLine {
            ts: Utc::now(),
            level: meta.level().to_string(),
            target: meta.target().to_string(),
            message: format!("{}{}", v.message, v.fields),
        });
    }
}

pub struct LogOptions {
    /// `RUST_LOG`-style filter; defaults to `info` for our crates.
    pub filter: Option<String>,
    pub stdout: bool,
}

/// Keeps the file writer alive; drop it at shutdown to flush.
pub struct LogHandle {
    pub buffer: LogBuffer,
    id: u64,
    _guard: tracing_appender::non_blocking::WorkerGuard,
}

impl Drop for LogHandle {
    fn drop(&mut self) {
        if let Some(g) = GLOBAL.get() {
            let mut slot = g.file.write();
            if slot.as_ref().is_some_and(|(id, _)| *id == self.id) {
                *slot = None;
            }
        }
    }
}

/// The subscriber is process-wide and installed once; each [`init`] only points its
/// file layer at the current data directory. Engines restarted in the same process
/// (local → remote → local in a UI) thus keep logging, to the same buffer.
struct Global {
    buffer: LogBuffer,
    file: FileSlot,
}

static GLOBAL: OnceLock<Global> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Default)]
struct FileSlot(Arc<RwLock<Option<(u64, NonBlocking)>>>);

impl std::ops::Deref for FileSlot {
    type Target = RwLock<Option<(u64, NonBlocking)>>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a> MakeWriter<'a> for FileSlot {
    type Writer = SlotWriter;
    fn make_writer(&'a self) -> SlotWriter {
        SlotWriter(self.read().as_ref().map(|(_, w)| w.clone()))
    }
}

/// Writes to the current log file, or nowhere between two engines.
struct SlotWriter(Option<NonBlocking>);

impl std::io::Write for SlotWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match &mut self.0 {
            Some(w) => w.write(buf),
            None => Ok(buf.len()),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match &mut self.0 {
            Some(w) => w.flush(),
            None => Ok(()),
        }
    }
}

/// Starts logging to `paths`. The filter and stdout options of the first call win.
pub fn init(paths: &Paths, opts: LogOptions) -> LogHandle {
    let filter_str = opts
        .filter
        .or_else(|| std::env::var("RUST_LOG").ok())
        .unwrap_or_else(|| {
            "warn,tm_twitch=info,tm_core=info,tm_modules=info,tm_server=info,twitchminer=info"
                .into()
        });
    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("twitchminer")
        .filename_suffix("log")
        .max_log_files(14)
        .build(paths.logs_dir())
        .expect("log directory is writable");
    let (file_writer, guard) = tracing_appender::non_blocking(appender);
    let global = GLOBAL.get_or_init(|| {
        let buffer = LogBuffer::default();
        let file = FileSlot::default();
        let file_layer = tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(file.clone());
        let ansi = std::io::IsTerminal::is_terminal(&std::io::stdout());
        let stdout_layer = opts.stdout.then(|| {
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_ansi(ansi)
                .compact()
        });
        let _ = tracing_subscriber::registry()
            .with(EnvFilter::new(filter_str))
            .with(file_layer)
            .with(stdout_layer)
            .with(BufferLayer(buffer.clone()))
            .try_init();
        Global { buffer, file }
    });
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    *global.file.write() = Some((id, file_writer));
    LogHandle {
        buffer: global.buffer.clone(),
        id,
        _guard: guard,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restarts_keep_logging() {
        let dir = |n: &str| {
            let d = std::env::temp_dir().join(format!("tm-log-{}-{n}", std::process::id()));
            Paths::at(&d).unwrap()
        };
        let opts = || LogOptions {
            filter: Some("info".into()),
            stdout: false,
        };
        let (a, b) = (dir("a"), dir("b"));
        let first = init(&a, opts());
        tracing::info!("first engine");
        drop(first);
        let second = init(&b, opts());
        tracing::info!("second engine");
        let lines: Vec<String> = second
            .buffer
            .recent(10)
            .into_iter()
            .map(|l| l.message)
            .collect();
        assert!(lines.iter().any(|m| m == "second engine"), "{lines:?}");
        drop(second);
        let written = std::fs::read_dir(b.logs_dir())
            .unwrap()
            .flatten()
            .map(|e| std::fs::read_to_string(e.path()).unwrap_or_default())
            .collect::<String>();
        assert!(written.contains("second engine"));
        let _ = std::fs::remove_dir_all(&a.root);
        let _ = std::fs::remove_dir_all(&b.root);
    }
}
