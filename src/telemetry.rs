use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::runtime::Tokio;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

static OTEL_INIT: OnceLock<()> = OnceLock::new();

/// Initialize tracing with the default stderr writer. Use this for
/// non-interactive commands (`volt agent-run --print`, `volt doctor`,
/// `volt workflow`, etc.) where the user expects to see logs inline.
///
/// For the interactive TUI, prefer [`init_otel_for_tui`] which routes
/// logs to a file so they don't bleed into the alternate screen.
pub fn init_otel(service_name: &str) {
    let stderr: Box<dyn Write + Send + Sync> = Box::new(std::io::stderr());
    let shared = Arc::new(Mutex::new(stderr));
    init_otel_with_writer(service_name, shared);
}

/// Initialize tracing with a file writer under `<volt-log-dir>/<service>.log`.
/// Used by the TUI so DB warnings, OTel exporter setup, and worker
/// progress don't pollute the chat. The user can `tail -f` the file if
/// they want to see what's happening.
pub fn init_otel_for_tui(service_name: &str, log_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(log_dir)?;
    let path = log_dir.join(format!("{}.log", service_name));
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    // `LineWriter` flushes on newlines so timestamps reach the log
    // promptly. The file is wrapped in `Arc<Mutex<>>` because
    // `MakeWriter` requires `Sync` access from multiple threads.
    let line_writer: Box<dyn Write + Send + Sync> = Box::new(LineWriter::new(file));
    let shared = Arc::new(Mutex::new(line_writer));
    init_otel_with_writer(service_name, shared);
    eprintln!("[tui logs] {}", path.display());
    Ok(())
}

fn init_otel_with_writer(service_name: &str, writer: Arc<Mutex<Box<dyn Write + Send + Sync>>>) {
    OTEL_INIT.get_or_init(|| {
        let provider = build_provider(service_name);

        let tracer = provider.tracer(service_name.to_string());
        let _ = opentelemetry::global::set_tracer_provider(provider);

        let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);
        let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

        // `MakeWriter` factory: each log call clones the Arc, locks
        // the mutex briefly, writes, and drops. Cheap because Mutex
        // contention is negligible at logging throughput.
        let make_writer = move || SharedWriter(writer.clone());
        let subscriber = tracing_subscriber::registry()
            .with(env_filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .with_target(false)
                    .with_writer(make_writer),
            )
            .with(telemetry);

        // `try_init` instead of `init` so this is fully idempotent —
        // a sibling library (Dioxus desktop, ort, etc.) may have set
        // a global subscriber already. We just skip ours in that case
        // and let the existing one win.
        let _ = subscriber.try_init();
    });
}

/// `MakeWriter` impl that locks the shared writer for the duration of
/// the log call. Cheap because each log line is one event.
struct SharedWriter(Arc<Mutex<Box<dyn Write + Send + Sync>>>);

impl Write for SharedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut g = self
            .0
            .lock()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        g.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let mut g = self
            .0
            .lock()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        g.flush()
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for SharedWriter {
    type Writer = SharedWriter;
    fn make_writer(&'a self) -> Self::Writer {
        SharedWriter(self.0.clone())
    }
}

/// Thin wrapper that calls `flush()` on every newline so the user sees
/// log output promptly when tailing the file.
struct LineWriter<W: Write> {
    inner: W,
}

impl<W: Write> LineWriter<W> {
    fn new(inner: W) -> Self {
        Self { inner }
    }
}

impl<W: Write> Write for LineWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        if buf.contains(&b'\n') {
            let _ = self.inner.flush();
        }
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn build_provider(service_name: &str) -> opentelemetry_sdk::trace::TracerProvider {
    if let Ok(endpoint) = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
        let exporter = match opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_endpoint(&endpoint)
            .build()
        {
            Ok(e) => e,
            Err(e) => {
                eprintln!(
                    "[otel] failed to build OTLP exporter, falling back to stdout: {}",
                    e
                );
                return opentelemetry_sdk::trace::TracerProvider::builder()
                    .with_simple_exporter(opentelemetry_stdout::SpanExporter::default())
                    .build();
            }
        };

        let resource = opentelemetry_sdk::Resource::new(vec![opentelemetry::KeyValue::new(
            "service.name",
            service_name.to_string(),
        )]);

        eprintln!("[otel] OTLP exporter -> {}", endpoint);
        opentelemetry_sdk::trace::TracerProvider::builder()
            .with_batch_exporter(exporter, Tokio)
            .with_resource(resource)
            .build()
    } else {
        eprintln!("[otel] stdout exporter (set OTEL_EXPORTER_OTLP_ENDPOINT for OTLP)");
        opentelemetry_sdk::trace::TracerProvider::builder()
            .with_simple_exporter(opentelemetry_stdout::SpanExporter::default())
            .build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `LineWriter` should flush on every newline so the user sees log
    /// output promptly when tailing the file.
    #[test]
    fn line_writer_writes_all_bytes() {
        let mut w = LineWriter::new(Vec::new());
        w.write_all(b"abc\ndef\n").unwrap();
        w.flush().unwrap();
        assert_eq!(w.inner, b"abc\ndef\n".to_vec());
    }

    /// `LineWriter` should pass through partial writes (no newlines)
    /// without dropping data — `tracing_subscriber` may write the
    /// timestamp, level, target, and message as separate chunks.
    #[test]
    fn line_writer_preserves_partial_writes() {
        let mut w = LineWriter::new(Vec::new());
        w.write_all(b"2026-06-04 ").unwrap();
        w.write_all(b"INFO").unwrap();
        w.write_all(b" hello\n").unwrap();
        assert_eq!(w.inner, b"2026-06-04 INFO hello\n".to_vec());
    }
}
