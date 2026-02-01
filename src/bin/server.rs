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

    // Initialize Metrics
    petpulse_server::metrics::init_metrics(&db).await;

    // Use app logic directly here
    let app = app(db, redis_client, gcs_client, prometheus_layer, metric_handle);

    let addr = SocketAddr::from(([0, 0, 0, 0], 8000));
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
        .route("/login", post(api::auth::login))
        .route("/webhook/alert", post(api::webhook::handle_alert));

    let protected_routes = Router::new()
        .route(
            "/users",
            get(api::user::get_user)
                .patch(api::user::update_user)
                .delete(api::user::delete_user),
        )
        .route("/pets", get(api::pet::list_user_pets).post(api::pet::create_pet))
        .route(
            "/pets/:id",
            get(api::pet::get_pet)
                .patch(api::pet::update_pet)
                .delete(api::pet::delete_pet),
        )
        .route("/videos", get(api::video::list_user_videos))
        .route("/pets/:id/videos", get(api::video::list_pet_videos))
        .route("/videos/:id/stream", get(api::video::serve_video))
        .route(
            "/pets/:id/upload_video",
            post(api::daily_digest::upload_video),
        )
        .route(
            "/internal/generate_daily_digest",
            post(api::daily_digest::generate_daily_digest),
        )
        // Alert routes - protected
        .route("/alerts", get(api::critical_alerts::list_user_alerts))
        .route("/alerts/:id", get(api::critical_alerts::get_alert))
        .route("/pets/:id/alerts", get(api::critical_alerts::list_pet_alerts))
        .route("/alerts/:id/acknowledge", post(api::critical_alerts::acknowledge_alert))
        .route("/alerts/:id/resolve", post(api::critical_alerts::resolve_alert))
        // Emergency Contacts routes - protected
        .route("/emergency-contacts", get(api::emergency_contacts::list_emergency_contacts).post(api::emergency_contacts::create_emergency_contact))
        .route("/emergency-contacts/:id", axum::routing::patch(api::emergency_contacts::update_emergency_contact).delete(api::emergency_contacts::delete_emergency_contact))
        // Quick Actions routes - protected
        .route("/alerts/:alert_id/quick-actions", post(api::quick_actions::create_quick_action).get(api::quick_actions::list_alert_quick_actions))
        // Daily digest routes - protected
        .route("/pets/:id/digests", get(api::daily_digest::list_pet_digests))
        .route_layer(axum::middleware::from_fn(api::middleware::auth_middleware));

    Router::new()
        .route("/health", get(health_check))
        .merge(auth_routes)
        .merge(protected_routes)
        // Critical Alert Routes (public for Grafana dashboard)
        .route("/api/alerts/critical", get(api::critical_alerts::get_pending_critical_alerts))
        .layer(Extension(db))
        .layer(Extension(redis_client))
        .layer(Extension(gcs_client))
        .layer(tower_cookies::CookieManagerLayer::new())
        .layer(prometheus_layer)
        .layer(
            tower_http::trace::TraceLayer::new_for_http()
                .make_span_with(|request: &axum::http::Request<axum::body::Body>| {
                    let matched_path = request
                        .extensions()
                        .get::<axum::extract::MatchedPath>()
                        .map(|matched| matched.as_str());

                    // Dynamic Span Name: "METHOD /path" (e.g., "POST /register")
                    let span_name = if let Some(path) = matched_path {
                        format!("{} {}", request.method(), path)
                    } else {
                        format!("{} {}", request.method(), request.uri().path())
                    };
                    
                    // Simple IP extraction
                    let user_ip = request
                        .headers()
                        .get("x-forwarded-for")
                        .and_then(|v| v.to_str().ok())
                        .or_else(|| {
                            request
                                .headers()
                                .get("x-real-ip")
                                .and_then(|v| v.to_str().ok())
                        })
                        .unwrap_or("unknown");

                    // Create span with explicit fields for business logic to "fill in" later
                    tracing::info_span!(
                        "request",
                        "otel.name" = span_name, // Override OpenTelemetry Span Name
                        user_ip = user_ip,
                        method = ?request.method(),
                        uri = ?request.uri(),
                        // Fields to be populated by handlers
                        table = tracing::field::Empty,
                        action = tracing::field::Empty,
                        user_id = tracing::field::Empty,
                        user_email = tracing::field::Empty, // Add email field
                        pet_id = tracing::field::Empty,
                        business_event = tracing::field::Empty,
                        error = tracing::field::Empty,
                        // status and latency recorded later
                        status = tracing::field::Empty,
                        latency = tracing::field::Empty,
                    )
                })
                .on_request(|_request: &axum::http::Request<axum::body::Body>, _span: &tracing::Span| {
                    // Disable default "started processing request" log to reduce noise
                })
                .on_response(|response: &axum::http::Response<_>, latency: std::time::Duration, span: &tracing::Span| {
                    // We can't easily access request details here unless stored in span or extensions.
                    // The span already captures method/uri from make_span_with.
                    // However, to make them appear "first" or top-level in the JSON event (not nested in span),
                    // we would need to pass them down or rely on the formatter flattening.
                    // Since we enabled flatten_event(true), span fields might still be separated.
                    // Let's rely on the Span fields for context, but ensure the message is clear.
                    
                    // To strictly satisfy "Request body starts with API endpoint", we'll rely on the field order
                    // in the macro, though JSON key order is not guaranteed.
                    
                    span.record("status", tracing::field::display(response.status()));
                    span.record("latency", tracing::field::debug(latency));
                    
                    tracing::info!(
                        "request completed"
                    );
                }))
        .layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin(
                    "http://localhost:3003"
                        .parse::<axum::http::HeaderValue>()
                        .unwrap()
                )
                .allow_methods([
                    axum::http::Method::GET,
                    axum::http::Method::POST,
                    axum::http::Method::PATCH,
                    axum::http::Method::DELETE,
                ])
                .allow_headers([axum::http::header::CONTENT_TYPE])
                .allow_credentials(true)
        )
        .route("/metrics", get(|| async move { metric_handle.render() }))
        .layer(axum::extract::DefaultBodyLimit::max(100 * 1024 * 1024))
}
