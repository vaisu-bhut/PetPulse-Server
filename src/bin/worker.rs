use petpulse_server::worker;
use sea_orm::Database;

#[tokio::main]
async fn main() {
    // Load .env if present (dotenvy)
    dotenvy::dotenv().ok();

    petpulse_server::telemetry::init_telemetry("petpulse-worker");

    let (prometheus_layer, metric_handle) = axum_prometheus::PrometheusMetricLayer::pair();

    // Spawn metrics server
    tokio::spawn(async move {
        let app = axum::Router::new()
            .route(
                "/metrics",
                axum::routing::get(|| async move { metric_handle.render() }),
            )
            .layer(prometheus_layer);
        let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 9091));
        tracing::info!("Metrics server listening on {}", addr);
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        axum::serve(listener, app).await.unwrap();
    });

    // Database Connection
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let db = Database::connect(&database_url)
        .await
        .expect("Failed to connect to database");

    // Redis Connection
    let redis_url =
        std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string());
    let redis_client = redis::Client::open(redis_url).expect("Invalid Redis URL");

    // GCS Client
    let gcs_config = google_cloud_storage::client::ClientConfig::default()
        .with_auth()
        .await
        .unwrap();
    let gcs_client = google_cloud_storage::client::Client::new(gcs_config);

    tracing::info!("Starting background worker...");

    // Start Video Workers (3 concurrent)
    worker::start_workers(redis_client.clone(), db.clone(), 3, gcs_client).await;

    // Start Digest Workers (3 concurrent, stateless)
    worker::start_digest_workers(redis_client.clone(), db.clone(), 3).await;

    // Keep the main process alive
    match tokio::signal::ctrl_c().await {
        Ok(()) => tracing::info!("Shutting down worker process"),
        Err(err) => tracing::error!("Unable to listen for shutdown signal: {}", err),
    }
}
