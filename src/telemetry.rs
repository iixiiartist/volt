use opentelemetry::trace::TracerProvider as _;
use std::sync::OnceLock;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

static OTEL_INIT: OnceLock<()> = OnceLock::new();

pub fn init_otel(service_name: &str) {
    OTEL_INIT.get_or_init(|| {
        let provider = opentelemetry_sdk::trace::TracerProvider::builder()
            .with_simple_exporter(opentelemetry_stdout::SpanExporter::default())
            .build();

        let tracer = provider.tracer(service_name.to_string());
        let _ = opentelemetry::global::set_tracer_provider(provider);

        let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);
        let subscriber = tracing_subscriber::registry()
            .with(tracing_subscriber::fmt::layer().with_target(false))
            .with(telemetry);

        subscriber.init();
        eprintln!("[otel] initialized for '{}'", service_name);
    });
}
