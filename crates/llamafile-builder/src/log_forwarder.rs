/// A tracing subscriber layer that broadcasts log events over the server's
/// SSE channel so the web UI can display them in real-time.
///
/// Enabled only when the server is running (`init()` is called by `server::run()`).
/// For all other subcommands the layer is a no-op — `EVENT_TX` stays unset.
use std::sync::OnceLock;
use tracing::field::{Field, Visit};
use tracing::Subscriber;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

static EVENT_TX: OnceLock<tokio::sync::broadcast::Sender<String>> = OnceLock::new();

/// Called once by `server::run()` to wire up log forwarding.
pub fn init(tx: tokio::sync::broadcast::Sender<String>) {
    let _ = EVENT_TX.set(tx);
}

/// A tracing layer that forwards events to the broadcast channel.
pub struct BroadcastLogLayer;

impl<S: Subscriber> Layer<S> for BroadcastLogLayer {
    /// Request TRACE-level interest from the registry so DEBUG/TRACE events
    /// reach this layer even when the fmt layer is filtered at INFO.
    fn max_level_hint(&self) -> Option<LevelFilter> {
        Some(LevelFilter::TRACE)
    }

    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let tx = match EVENT_TX.get() {
            Some(tx) => tx,
            None => return,
        };

        let target = event.metadata().target();
        // Filter out noisy HTTP-client internals.
        if target.starts_with("hyper::")
            || target.starts_with("reqwest::")
            || target.starts_with("rustls::")
            || target.starts_with("h2::")
            || target.starts_with("axum::")
            || target.starts_with("tower::")
        {
            return;
        }

        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);

        // Filter noisy connection-pool messages by content.
        if visitor.message.contains("pooling idle connection")
            || visitor.message.contains("reuse idle connection")
            || visitor.message.contains("connecting to")
            || visitor.message.contains("connected to")
        {
            return;
        }

        let level = event.metadata().level().as_str().to_lowercase();

        // Build field object so the frontend can identify structured events
        // (e.g. download progress via `bytes_downloaded` / `bytes_total`).
        let mut fields = serde_json::Map::with_capacity(visitor.fields.len());
        for (k, v) in &visitor.fields {
            fields.insert(k.clone(), v.clone());
        }

        let payload = serde_json::json!({
            "stage": "log",
            "level": level,
            "target": target,
            "message": visitor.message,
            "fields": fields,
        })
        .to_string();

        let _ = tx.send(payload);
    }
}

// ============================================================================
// Message extraction (same pattern as llamafile-builder-tui/src/log_layer.rs)
// ============================================================================

#[derive(Default)]
struct MessageVisitor {
    message: String,
    fields: Vec<(String, serde_json::Value)>,
}

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let name = field.name();
        if name == "message" {
            let s = format!("{:?}", value);
            self.message = if s.starts_with('"') && s.ends_with('"') {
                s[1..s.len() - 1].to_string()
            } else {
                s
            };
        } else if !name.starts_with("log.") {
            self.fields
                .push((name.to_string(), serde_json::Value::String(format!("{:?}", value))));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        let name = field.name();
        if name == "message" {
            self.message = value.to_string();
        } else if !name.starts_with("log.") {
            self.fields
                .push((name.to_string(), serde_json::Value::String(value.to_string())));
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        if !field.name().starts_with("log.") {
            self.fields
                .push((field.name().to_string(), serde_json::Value::Number(value.into())));
        }
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        if !field.name().starts_with("log.") {
            self.fields
                .push((field.name().to_string(), serde_json::Value::Number(value.into())));
        }
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        if !field.name().starts_with("log.") {
            let num = serde_json::Number::from_f64(value)
                .unwrap_or_else(|| serde_json::Number::from(0));
            self.fields
                .push((field.name().to_string(), serde_json::Value::Number(num)));
        }
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        if !field.name().starts_with("log.") {
            self.fields
                .push((field.name().to_string(), serde_json::Value::Bool(value)));
        }
    }
}
