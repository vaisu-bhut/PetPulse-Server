use sea_orm::Database;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use petpulse_server::worker;

#[tokio::main]
async fn main() {
    // Load .env if present (dotenvy)
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Database Connection
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let db = Database::connect(&database_url).await.expect("Failed to connect to database");

    // Redis Connection
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string());
    let redis_client = redis::Client::open(redis_url).expect("Invalid Redis URL");

    // GCS Client
    let gcs_config = google_cloud_storage::client::ClientConfig::default().with_auth().await.unwrap();
    let gcs_client = google_cloud_storage::client::Client::new(gcs_config);

    tracing::info!("Starting background worker...");
    
    // Start Workers
    worker::start_workers(redis_client, db, 3, gcs_client).await;

    // Keep the main process alive
    match tokio::signal::ctrl_c().await {
        Ok(()) => tracing::info!("Shutting down worker process"),
        Err(err) => tracing::error!("Unable to listen for shutdown signal: {}", err),
    }
}
