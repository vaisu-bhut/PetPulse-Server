use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{trace as sdktrace, Resource};
use opentelemetry_semantic_conventions::resource;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub fn init_telemetry(service_name: &str) {
    let log_format = std::env::var("RUST_LOG_FORMAT").unwrap_or_else(|_| "text".to_string());
    let otlp_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok();

    // EnvFilter
    // Suppress DB debug logs (sqlx, sea_orm) by setting them to warn. Default to info.
    let env_filter = tracing_subscriber::EnvFilter::new(
        std::env::var("RUST_LOG").unwrap_or_else(|_| "info,petpulse_server=info,sqlx=warn,sea_orm=warn".into()),
    );

    // Registry
    let registry = tracing_subscriber::registry().with(env_filter);

    // OTLP Tracing Layer
    let otel_layer = if let Some(endpoint) = otlp_endpoint {
        let resource = Resource::new(vec![KeyValue::new(
            resource::SERVICE_NAME,
            service_name.to_string(),
        )]);

        let tracer = opentelemetry_otlp::new_pipeline()
            .tracing()
            .with_exporter(
                opentelemetry_otlp::new_exporter()
                    .tonic()
                    .with_endpoint(endpoint),
            )
            .with_trace_config(
                sdktrace::config()
                    .with_resource(resource)
                    .with_sampler(sdktrace::Sampler::AlwaysOn),
            )
            .install_batch(opentelemetry_sdk::runtime::Tokio)
            .expect("failed to install OpenTelemetry tracer");

        Some(tracing_opentelemetry::layer().with_tracer(tracer))
    } else {
        None
    };

    // Fmt Layer (JSON or Text)
    if log_format == "json" {
        // flatten_event(true) moves fields to top level.
        // without_time() removes timestamp.
        let fmt_layer = tracing_subscriber::fmt::layer()
            .json()
            .flatten_event(true)
            .without_time();
        registry.with(otel_layer).with(fmt_layer).init();
    } else {
        let fmt_layer = tracing_subscriber::fmt::layer();
        registry.with(otel_layer).with(fmt_layer).init();
    };
}
