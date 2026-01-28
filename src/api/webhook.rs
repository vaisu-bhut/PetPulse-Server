use axum::{
    Json,
    response::IntoResponse,
    http::StatusCode,
};
use tracing::{info, error};
use crate::agent::comfort_loop::AlertPayload;

pub async fn handle_alert(
    Json(payload): Json<AlertPayload>,
) -> impl IntoResponse {
    info!("Received alert webhook: alert_type={:?}, pet_id={}", payload.alert_type, payload.pet_id);

    // Forward to Agent Service
    // In a real K8s env, "petpulse_agent" or "agent" service name
    // For docker-compose, "agent" service name, port 3002
    let agent_url = std::env::var("AGENT_SERVICE_URL")
        .unwrap_or_else(|_| "http://agent:3002/alert".to_string());
    
    // Spawn a tokio task to not block response
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        match client.post(&agent_url).json(&payload).send().await {
            Ok(resp) => {
                if !resp.status().is_success() {
                    error!("Agent service returned error: {}", resp.status());
                } else {
                    info!("Successfully forwarded alert to Agent service");
                }
            },
            Err(e) => {
                error!("Failed to forward alert to Agent service: {}", e);
            }
        }
    });

    (StatusCode::OK, "Alert received and forwarding")
}

