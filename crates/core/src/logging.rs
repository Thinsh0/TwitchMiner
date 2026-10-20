//! Logging: rolling files in `<data>/logs`, optional stdout, and an in-memory
//! ring buffer + live stream that UIs can display.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::field::{Field, Visit};
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
    _guard: tracing_appender::non_blocking::WorkerGuard,
}

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
    let buffer = LogBuffer::default();

    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(file_writer);
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
    LogHandle {
        buffer,
        _guard: guard,
    }
}
