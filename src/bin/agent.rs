use axum::{
    routing::{get, post},
    Json, Router,
};
use std::net::SocketAddr;
use petpulse_server::agent::comfort_loop::{ComfortLoop, AlertPayload};
use std::sync::Arc;
use tokio::sync::mpsc;
use sea_orm::Database;
use tracing::error;

struct AppState {
    tx: mpsc::Sender<AlertPayload>,
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();
    
    tracing::info!("Starting PetPulse Agent Service...");

    // Database Connection
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let db = Database::connect(&database_url)
        .await
        .expect("Failed to connect to database");

    // Create Channel for Task Queue
    let (tx, mut rx) = mpsc::channel::<AlertPayload>(100);

    // Initialize Comfort Loop Logic (Shared)
    let comfort_loop = Arc::new(ComfortLoop::new(db));

    // Spawn Dispatcher Task with Concurrency Limit

    let loop_logic = comfort_loop.clone();
    tokio::spawn(async move {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(2));
        while let Some(payload) = rx.recv().await {
            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let logic = loop_logic.clone();
            tokio::spawn(async move {
                logic.process_alert(payload).await;
                drop(permit);
            });
        }
    });

    let state = Arc::new(AppState {
        tx,
    });

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/alert", post(handle_alert))
        .route("/alert/critical", post(handle_alert))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3002));
    tracing::info!("Agent listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn health_check() -> &'static str {
    "OK"
}

async fn handle_alert(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Json(payload): Json<AlertPayload>,
) -> &'static str {
    tracing::info!("Received alert webhook: alert_type={:?}, pet_id={}", payload.alert_type, payload.pet_id);
    
    // Send to channel, don't wait for processing
    match state.tx.send(payload).await {
        Ok(_) => "Queued",
        Err(_) => {
            error!("Failed to queue alert - channel closed");
            "Error"
        }
    }
}

