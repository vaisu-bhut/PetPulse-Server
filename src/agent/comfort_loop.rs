use serde::{Deserialize, Serialize};
use tracing::{info, error};
use std::collections::HashMap;
use sea_orm::{DatabaseConnection, EntityTrait, Set, ColumnTrait, QueryFilter, QueryOrder, PaginatorTrait};
use uuid::Uuid;
use crate::entities::alerts;

// Core Alert Structures
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AlertPayload {
    pub alert_id: String,
    pub pet_id: String,
    pub alert_type: AlertType,
    pub severity: String,
    pub message: Option<String>,
    pub metric_value: Option<f64>,
    pub baseline_value: Option<f64>,
    pub deviation_factor: Option<f64>,
    // Context fields (can be populated by worker/server)
    pub video_id: Option<String>,
    pub timestamp: Option<String>,
    pub context: Option<serde_json::Value>,
    // Legacy Grafana fields (optional for backward compatibility)
    pub title: Option<String>,
    pub state: Option<String>,
    #[serde(rename = "evalMatches")]
    pub eval_matches: Option<Vec<EvalMatch>>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct EvalMatch {
    pub value: f64,
    pub metric: String,
    pub tags: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AlertType {
    // Behavioral alerts
    Pacing,
    Vocalization,
    PositionChanges,
    DoorProximity,
    Restlessness,
    AttentionSeeking,
    // Worker-detected alerts
    UnusualBehavior,
    ProcessingError,
    QueueDepthHigh,
    // Generic fallback
    Comfort,
}

impl ToString for AlertType {
    fn to_string(&self) -> String {
        match self {
            AlertType::Pacing => "pacing".to_string(),
            AlertType::Vocalization => "vocalization".to_string(),
            AlertType::PositionChanges => "position_changes".to_string(),
            AlertType::DoorProximity => "door_proximity".to_string(),
            AlertType::Restlessness => "restlessness".to_string(),
            AlertType::AttentionSeeking => "attention_seeking".to_string(),
            AlertType::UnusualBehavior => "unusual_behavior".to_string(),
            AlertType::ProcessingError => "processing_error".to_string(),
            AlertType::QueueDepthHigh => "queue_depth_high".to_string(),
            AlertType::Comfort => "comfort".to_string(),
        }
    }
}

// Intervention Logic
pub struct ComfortLoop {
    db: DatabaseConnection,
    _gemini_client: Option<()>, // Placeholder for Gemini Client
}

impl ComfortLoop {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db, _gemini_client: None }
    }

    pub async fn process_alert(&self, payload: AlertPayload) {
        info!("Processing alert: {:?}", payload);
        
        let alert_uuid = Uuid::new_v4();
        
        // 1. Persist Initial Alert
        // Parse pet_id from string to i32 (as per schema)
        let db_pet_id = payload.pet_id.parse::<i32>().unwrap_or_else(|e| {
            error!("Failed to parse pet_id '{}': {}. Using 1 as fallback.", payload.pet_id, e);
            1
        });

        let active_model = alerts::ActiveModel {
            id: Set(alert_uuid),
            pet_id: Set(db_pet_id),
            alert_type: Set(payload.alert_type.to_string()),
            severity: Set(payload.severity.clone()),
            message: Set(payload.message.clone()),
            payload: Set(serde_json::to_value(&payload).unwrap_or_default()),
            created_at: Set(chrono::Utc::now().naive_utc()),
            ..Default::default()
        };

        if let Err(e) = alerts::Entity::insert(active_model).exec(&self.db).await {
            error!("Failed to insert alert into DB: {}", e);
            return;
        }

        info!("Alert {} persisted to database", alert_uuid);

        // 2. Check recent alert count for escalation (last 1 hour)
        let one_hour_ago = chrono::Utc::now().naive_utc() - chrono::Duration::hours(1);
        let recent_alert_count = match alerts::Entity::find()
            .filter(alerts::Column::PetId.eq(db_pet_id))
            .filter(alerts::Column::AlertType.eq(payload.alert_type.to_string()))
            .filter(alerts::Column::CreatedAt.gte(one_hour_ago))
            .count(&self.db)
            .await
        {
            Ok(count) => count,
            Err(e) => {
                error!("Failed to count recent alerts: {}", e);
                1 // Default to first intervention
            }
        };
        
        info!(
            "Alert count for pet_id={}, type={} in last hour: {}",
            db_pet_id,
            payload.alert_type.to_string(),
            recent_alert_count
        );

        // 3. Decide Intervention (escalating based on count)
        let intervention = self.decide_intervention(&payload, recent_alert_count).await;
        
        // 4. Execute Action
        self.execute_action(&intervention).await;

        // 5. Update DB with Action
        let update_model = alerts::ActiveModel {
            id: Set(alert_uuid),
            intervention_action: Set(Some(format!("{:?}", intervention))),
            intervention_time: Set(Some(chrono::Utc::now().naive_utc())),
            ..Default::default()
        };
        
        if let Err(e) = alerts::Entity::update(update_model).exec(&self.db).await {
            error!("Failed to update alert intervention: {}", e);
        }

        // 6. Continuous Monitoring - wait and check for resolution
        info!("Monitoring for resolution... Checking for new normal videos.");
        tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
        
        // Check if new videos have been analyzed as normal (is_unusual = false)
        // We check if the latest video for this pet is NOT unusual
        use crate::entities::pet_video;
        let latest_video = pet_video::Entity::find()
            .filter(pet_video::Column::PetId.eq(db_pet_id))
            .filter(pet_video::Column::Status.eq("PROCESSED"))
            .order_by_desc(pet_video::Column::CreatedAt)
            .one(&self.db)
            .await
            .ok()
            .flatten();
        
        let outcome = if let Some(video) = latest_video {
            if !video.is_unusual {
                info!("Latest video shows normal behavior - alert resolved");
                "Resolution: Pet behavior returned to normal. Alert resolved."
            } else {
                info!("Latest video still shows unusual behavior - alert persists");
                "Alert persists: Unusual behavior continues. May trigger escalation on next alert."
            }
        } else {
            "No new video data available for resolution check."
        };
        
        info!("{}", outcome);

        // 7. Update DB with Outcome
        let outcome_model = alerts::ActiveModel {
            id: Set(alert_uuid),
            outcome: Set(Some(outcome.to_string())),
            ..Default::default()
        };
        if let Err(e) = alerts::Entity::update(outcome_model).exec(&self.db).await {
            error!("Failed to update alert outcome: {}", e);
        }
    }

    async fn decide_intervention(&self, payload: &AlertPayload, alert_count: u64) -> Intervention {
        // Escalating intervention based on alert count
        // 1st alert: Gentle intervention
        // 2nd alert: Moderate intervention  
        // 3rd+ alert: Strong intervention
        
        info!("Deciding intervention for alert_type={:?}, alert_count={}", payload.alert_type, alert_count);
        
        match alert_count {
            1 => {
                // First alert - gentle intervention
                match payload.alert_type {
                    AlertType::Pacing | AlertType::Restlessness => Intervention::DimLights,
                    AlertType::Vocalization | AlertType::AttentionSeeking => Intervention::PlayCalmingMusic,
                    AlertType::UnusualBehavior => Intervention::PlayCalmingMusic,
                    _ => Intervention::LogOnly,
                }
            },
            2 => {
                // Second alert - moderate intervention
                match payload.alert_type {
                    AlertType::Pacing | AlertType::Restlessness | AlertType::UnusualBehavior => Intervention::PlayOwnerVoice,
                    AlertType::Vocalization | AlertType::AttentionSeeking => Intervention::PlayOwnerVoice,
                    _ => Intervention::LogOnly,
                }
            },
            _ => {
                // Third+ alert - strong intervention (all types get owner voice)
                info!("Alert escalation: {} alerts in last hour, using strongest intervention", alert_count);
                match payload.alert_type {
                    AlertType::ProcessingError | AlertType::QueueDepthHigh => Intervention::LogOnly,
                    _ => Intervention::PlayOwnerVoice,
                }
            }
        }
    }

    async fn execute_action(&self, action: &Intervention) {
        info!("Executing intervention: {:?}", action);
        // TODO: Call Smart Home API / IoT Hub
        // User Assurance: "logs this Intervention"
        match action {
            Intervention::PlayCalmingMusic => {
                info!("Action: Playing calming music playlist");
            },
            Intervention::PlayOwnerVoice => {
                info!("Action: Playing owner voice note");
            },
            Intervention::DimLights => {
                info!("Action: Dimming lights to 50%");
            },
            Intervention::LogOnly => {
                info!("Action: Logging alert only");
            }
        }
    }
}

#[derive(Debug)]
enum Intervention {
    PlayCalmingMusic,
    PlayOwnerVoice,
    DimLights,
    LogOnly,
}
