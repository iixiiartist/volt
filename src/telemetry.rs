use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::runtime::Tokio;
use std::sync::OnceLock;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

static OTEL_INIT: OnceLock<()> = OnceLock::new();

pub fn init_otel(service_name: &str) {
    OTEL_INIT.get_or_init(|| {
        let provider = build_provider(service_name);

        let tracer = provider.tracer(service_name.to_string());
        let _ = opentelemetry::global::set_tracer_provider(provider);

        let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);
        let subscriber = tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .with_target(false)
                    .with_writer(std::io::stderr),
            )
            .with(telemetry);

        subscriber.init();
    });
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
