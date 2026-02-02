use google_cloud_pubsub::client::{Client, ClientConfig};
use google_cloud_googleapis::pubsub::v1::PubsubMessage;
use serde::{Deserialize, Serialize};
use std::env;
use tracing::{error, info};

#[derive(Debug, Serialize, Deserialize)]
pub struct AlertEmailPayload {
    pub email: String,
    pub pet_name: String,
    pub message: String,
    pub severity: String,
    pub id: String,
    pub title: Option<String>,
}

#[derive(Clone)]
pub struct PubSubClient {
    client: Client,
    topic_name: String,
}

impl PubSubClient {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let config = ClientConfig::default().with_auth().await?;
        let client = Client::new(config).await?;
        
        let environment = env::var("ENVIRONMENT").unwrap_or_else(|_| "preview".to_string());
        let topic_name = format!("alert-email-topic-{}", environment);

        Ok(Self {
            client,
            topic_name,
        })
    }

    pub async fn publish_email_alert(&self, payload: AlertEmailPayload) {
        let topic = self.client.topic(&self.topic_name);
        
        // Ensure topic exists (optional, usually handled by infra)
        // if !topic.exists(None).await.unwrap_or(false) { ... }

        let publisher = topic.new_publisher(None);
        
        let json_payload = match serde_json::to_string(&payload) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to serialize alert payload: {}", e);
                return;
            }
        };

        let message = PubsubMessage {
            data: json_payload.into_bytes(),
            ..Default::default()
        };

        let awaiter = publisher.publish(message).await;
        
        // Wait for message to be sent
        match awaiter.get().await {
            Ok(id) => info!("Published alert email to Pub/Sub: message_id={}", id),
            Err(e) => error!("Failed to publish alert email: {}", e),
        }
    }
}
