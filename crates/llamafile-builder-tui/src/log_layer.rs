use std::sync::mpsc;
use tracing::field::{Field, Visit};
use tracing::Subscriber;
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

use llamafile_builder_core::BuildPhase;

use crate::app::{BuildSignals, DownloadFields, LogEntry, LogLevel};

/// A tracing layer that forwards events to the TUI as `LogEntry` values.
///
/// In addition to formatting the message, the layer extracts well-known
/// structured fields (`phase`, `bytes_downloaded`, `bytes_total`,
/// `filename`) from the tracing event and attaches them to the entry as
/// `BuildSignals`. The main loop reads those signals to drive build/download
/// status — never the message text.
pub struct TuiLogLayer {
    sender: mpsc::Sender<LogEntry>,
}

impl TuiLogLayer {
    pub fn new(sender: mpsc::Sender<LogEntry>) -> Self {
        Self { sender }
    }
}

fn tracing_to_log_level(level: &tracing::Level) -> LogLevel {
    match *level {
        tracing::Level::ERROR => LogLevel::Error,
        tracing::Level::WARN => LogLevel::Warn,
        tracing::Level::INFO => LogLevel::Info,
        _ => LogLevel::Debug,
    }
}

impl<S: Subscriber> Layer<S> for TuiLogLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        // Filter out noisy connection pool / TLS messages
        let target = event.metadata().target();
        if target.starts_with("hyper::") || target.starts_with("reqwest::") || target.starts_with("rustls::") {
            return;
        }

        // Also filter by message content for libraries that don't set proper targets
        let mut peek = PeekVisitor::default();
        event.record(&mut peek);
        if peek.message.contains("pooling idle connection")
            || peek.message.contains("reuse idle connection")
            || peek.message.contains("connecting to")
            || peek.message.contains("connected to")
        {
            return;
        }

        let level = tracing_to_log_level(event.metadata().level());
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);

        let msg = if visitor.message.is_empty() {
            visitor
                .fields
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            let fields_str = if visitor.fields.is_empty() {
                String::new()
            } else {
                format!(
                    " {}",
                    visitor
                        .fields
                        .iter()
                        .map(|(k, v)| format!("{}={}", k, v))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            };
            format!("{}{}", visitor.message, fields_str)
        };

        let message = format!("[{:5}] {}", level.as_str(), msg);
        let signals = visitor.into_signals();
        let _ = self.sender.send(LogEntry { level, message, signals });
    }
}

#[derive(Default)]
struct PeekVisitor {
    message: String,
}

impl Visit for PeekVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
        }
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        }
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
    fields: Vec<(String, String)>,
    // Structured signal extraction
    phase: Option<BuildPhase>,
    bytes_downloaded: Option<u64>,
    bytes_total: Option<u64>,
    filename: Option<String>,
    download_finished: bool,
}

impl MessageVisitor {
    fn into_signals(self) -> BuildSignals {
        let download = match (self.bytes_downloaded, self.bytes_total) {
            (Some(d), Some(t)) => Some(DownloadFields {
                filename: self.filename,
                bytes_downloaded: d,
                bytes_total: t,
            }),
            _ => None,
        };
        BuildSignals {
            phase: self.phase,
            download,
            download_finished: self.download_finished,
        }
    }

    fn note_message_for_finish(&mut self, msg: &str) {
        // The cache and download layers don't yet emit a structured
        // "download terminated" field. Until they do, recognise the small
        // closed set of messages that mean the same thing. Limited to known
        // exact phrases (no substring sniffing) so message tweaks elsewhere
        // can't accidentally clear the progress display.
        if msg == "download complete" || msg == "already cached" || msg == "already in HF cache" {
            self.download_finished = true;
        }
    }
}

/// Strip a single layer of surrounding double quotes, if present.
/// `format!("{:?}", "foo")` yields `"\"foo\""`; we want `"foo"`.
fn unquote(s: String) -> String {
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s[1..s.len() - 1].to_string()
    } else {
        s
    }
}

impl Visit for MessageVisitor {
    /// Critical: in `tracing-core`, `DisplayValue<T>::record` (the `%`
    /// formatter) calls `visitor.record_debug(self)` with a `Debug` impl
    /// that delegates to `Display`. Same for `DebugValue<T>` (the `?`
    /// formatter). So *every* tracing event field emitted with `%` or
    /// `?` lands here — including `phase` and `filename`. Anything not
    /// extracted in `record_debug` is invisible to `BuildSignals`.
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let name = field.name();
        let formatted = format!("{:?}", value);

        if name == "message" {
            self.message = unquote(formatted);
            self.note_message_for_finish(&self.message.clone());
            return;
        }

        // Structured signal extraction. `BuildPhase::from_str` accepts
        // both the kebab-case Display form and the PascalCase Debug form,
        // so callers using either `%` or `?` with the enum work.
        match name {
            "phase" => {
                if let Some(p) = BuildPhase::from_str(&formatted) {
                    self.phase = Some(p);
                }
            }
            "filename" => {
                self.filename = Some(unquote(formatted.clone()));
            }
            _ => {}
        }

        if !name.starts_with("log.") {
            self.fields.push((name.to_string(), formatted));
        }
    }

    /// `record_str` fires for `&str` values recorded *without* a
    /// formatter — e.g. `Span::current().record("filename", &s.as_str())`
    /// or string literals passed bare. Keeps the same field detection so
    /// span-recorded values populate signals too.
    fn record_str(&mut self, field: &Field, value: &str) {
        let name = field.name();
        match name {
            "message" => {
                self.message = value.to_string();
                self.note_message_for_finish(value);
            }
            "phase" => {
                if let Some(p) = BuildPhase::from_str(value) {
                    self.phase = Some(p);
                }
                self.fields.push((name.to_string(), value.to_string()));
            }
            "filename" => {
                self.filename = Some(value.to_string());
                self.fields.push((name.to_string(), value.to_string()));
            }
            _ if name.starts_with("log.") => {}
            _ => {
                self.fields
                    .push((name.to_string(), value.to_string()));
            }
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        let name = field.name();
        match name {
            "bytes_downloaded" => self.bytes_downloaded = Some(value),
            "bytes_total" => self.bytes_total = Some(value),
            _ => {}
        }
        self.fields
            .push((name.to_string(), value.to_string()));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields
            .push((field.name().to_string(), value.to_string()));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.fields
            .push((field.name().to_string(), format!("{:.1}", value)));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields
            .push((field.name().to_string(), value.to_string()));
    }
}
