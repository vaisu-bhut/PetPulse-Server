use axum::{
    routing::{get, post},
    Extension, Router,
};
use petpulse_server::{api, migrator};
use sea_orm::{Database, DatabaseConnection};
use std::net::SocketAddr;


#[tokio::main]
async fn main() {
    // Load .env if present (dotenvy)
    dotenvy::dotenv().ok();

    petpulse_server::telemetry::init_telemetry("petpulse-server");

    let (prometheus_layer, metric_handle) = axum_prometheus::PrometheusMetricLayer::pair();

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

    // Run migrations
    use sea_orm_migration::MigratorTrait;
    migrator::Migrator::up(&db, None)
        .await
        .expect("Failed to run migrations");

    // Use app logic directly here
    let app = app(db, redis_client, gcs_client, prometheus_layer, metric_handle);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::info!("listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn health_check() -> &'static str {
    "OK"
}

fn app(
    db: DatabaseConnection,
    redis_client: redis::Client,
    gcs_client: google_cloud_storage::client::Client,
    prometheus_layer: axum_prometheus::PrometheusMetricLayer<'static>,
    metric_handle: metrics_exporter_prometheus::PrometheusHandle,
) -> Router {
    let auth_routes = Router::new()
        .route("/register", post(api::auth::register))
        .route("/login", post(api::auth::login));

    let protected_routes = Router::new()
        .route(
            "/users",
            get(api::user::get_user)
                .patch(api::user::update_user)
                .delete(api::user::delete_user),
        )
        .route("/pets", post(api::pet::create_pet))
        .route(
            "/pets/:id",
            get(api::pet::get_pet)
                .patch(api::pet::update_pet)
                .delete(api::pet::delete_pet),
        )
        .route(
            "/pets/:id/upload_video",
            post(api::daily_digest::upload_video),
        )
        .route(
            "/internal/generate_daily_digest",
            post(api::daily_digest::generate_daily_digest),
        )
        .route_layer(axum::middleware::from_fn(api::middleware::auth_middleware));

    Router::new()
        .route("/health", get(health_check))
        .merge(auth_routes)
        .merge(protected_routes)
        .layer(Extension(db))
        .layer(Extension(redis_client))
        .layer(Extension(gcs_client))
        .layer(tower_cookies::CookieManagerLayer::new())
        .layer(prometheus_layer)
        .route("/metrics", get(|| async move { metric_handle.render() }))
}
