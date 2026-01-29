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
    // Phase 3 & 4: Critical Alert Fields
    pub severity_level: Option<String>, 
    pub critical_indicators: Option<Vec<String>>,
    pub recommended_actions: Option<Vec<String>>,
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

use crate::notifications::TwilioNotifier;
use sea_orm::ActiveValue::NotSet;

// Intervention Logic
pub struct ComfortLoop {
    db: DatabaseConnection,
    notifier: TwilioNotifier,
}

impl ComfortLoop {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { 
            db, 
            notifier: TwilioNotifier::new() 
        }
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

        // Extract detailed fields from payload or context
        let severity_level = payload.severity_level.clone()
            .or_else(|| payload.context.as_ref()
                .and_then(|c| c.get("severity_level").and_then(|v| v.as_str().map(String::from))))
            .unwrap_or_else(|| "low".to_string());

        let critical_indicators = payload.critical_indicators.clone()
            .or_else(|| payload.context.as_ref()
                .and_then(|c| c.get("critical_indicators").and_then(|v| serde_json::from_value(v.clone()).ok())));

        let recommended_actions = payload.recommended_actions.clone()
            .or_else(|| payload.context.as_ref()
                .and_then(|c| c.get("recommended_actions").and_then(|v| serde_json::from_value(v.clone()).ok())));

        let active_model = alerts::ActiveModel {
            id: Set(alert_uuid),
            pet_id: Set(db_pet_id),
            alert_type: Set(payload.alert_type.to_string()),
            severity: Set(payload.severity.clone()),
            message: Set(payload.message.clone()),
            severity_level: Set(severity_level.clone()),
            critical_indicators: Set(critical_indicators.clone().map(|v| serde_json::to_value(v).unwrap_or(serde_json::Value::Null))),
            recommended_actions: Set(recommended_actions.clone().map(|v| serde_json::to_value(v).unwrap_or(serde_json::Value::Null))),
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
        
        // Phase 4: Handle Critical Alerts specifically
        if severity_level == "critical" {
            // Trigger Critical Notification Branch
            self.handle_critical_alert(&payload, alert_uuid, &critical_indicators, &recommended_actions).await;
            return; // Skip normal monitoring/resolution loop for critical alerts
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

    async fn handle_critical_alert(
        &self, 
        payload: &AlertPayload, 
        alert_uuid: Uuid,
        critical_indicators: &Option<Vec<String>>,
        recommended_actions: &Option<Vec<String>>
    ) {
        info!("🚨 HANDLING CRITICAL ALERT: {}", alert_uuid);
        
        let owner_email = std::env::var("OWNER_EMAIL").unwrap_or("test@example.com".to_string());
        let owner_phone = std::env::var("OWNER_PHONE").unwrap_or("+15550000000".to_string());
        
        let video_link = if let Some(vid) = &payload.video_id {
            // In a real scenario, generate a signed URL here.
            // For now, use a direct link placeholder
            format!("https://petpulse.dashboard/videos/{}", vid)
        } else {
            "https://petpulse.dashboard".to_string()
        };

        // Send Notifications
        self.notifier.notify_critical_alert(
            &owner_email,
            &owner_phone,
            "Your Pet",
            "CRITICAL",
            payload.message.as_deref().unwrap_or("Critical health indicator detected"),
            critical_indicators.as_deref().unwrap_or(&[]),
            recommended_actions.as_deref().unwrap_or(&[]),
            &video_link
        ).await;

        // Update Database Tracking
        let update_model = alerts::ActiveModel {
            id: Set(alert_uuid),
            notification_sent: Set(true),
            notification_channels: Set(Some(serde_json::json!(["email", "sms"]))),
            user_notified_at: Set(Some(chrono::Utc::now().naive_utc())),
            intervention_action: Set(Some("CRITICAL_NOTIFICATION_SENT".to_string())),
            outcome: Set(Some("Waiting for user acknowledgement".to_string())),
            ..Default::default()
        };
        
        if let Err(e) = alerts::Entity::update(update_model).exec(&self.db).await {
            error!("Failed to update alert notification status: {}", e);
        }
    }

    async fn decide_intervention(&self, payload: &AlertPayload, alert_count: u64) -> Intervention {
        // If critical, immediately escalate to Notification (handled in main loop branching, but good for safety)
        if let Some(severity) = &payload.severity_level {
            if severity == "critical" {
                return Intervention::NotifyUser(NotificationLevel::Critical);
            }
        }
        
        // Standard Escalation Logic
        info!("Deciding intervention for alert_type={:?}, alert_count={}", payload.alert_type, alert_count);
        match alert_count {
            0..=1 => match payload.alert_type {
                AlertType::Pacing | AlertType::Restlessness => Intervention::AdjustEnvironment(EnvironmentAction::DimLights), 
                AlertType::Vocalization | AlertType::AttentionSeeking => Intervention::PlayCalmingMusic,
                AlertType::UnusualBehavior => Intervention::PlayCalmingMusic,
                _ => Intervention::LogOnly,
            },
            2..=3 => match payload.alert_type {
                AlertType::Pacing | AlertType::Restlessness => Intervention::PlayOwnerVoice,
                AlertType::Vocalization => Intervention::DispenseTreat,
                _ => Intervention::PlayOwnerVoice,
            },
            _ => {
                 // 4+ alerts - strong intervention
                 info!("Alert escalation: {} alerts in last hour, flagging for user notification", alert_count);
                 Intervention::NotifyUser(NotificationLevel::Standard)
            }
        }
    }

    async fn execute_action(&self, action: &Intervention) {
        info!("Executing intervention: {:?}", action);
        // TODO: Call Smart Home API / IoT Hub
        match action {
            Intervention::PlayCalmingMusic => info!("🎶 Action: Playing calming music playlist"),
            Intervention::PlayOwnerVoice => info!("🗣️ Action: Playing owner voice note"),
            Intervention::DispenseTreat => info!("🍬 Action: Dispensing treat"),
            Intervention::AdjustEnvironment(env_action) => info!("💡 Action: Adjusting environment: {:?}", env_action),
            Intervention::NotifyUser(level) => info!("📱 Action: Notifying user (Level: {:?})", level),
            Intervention::LogOnly => info!("📝 Action: Logging alert only"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub enum Intervention {
    PlayCalmingMusic,
    PlayOwnerVoice,
    DispenseTreat,
    AdjustEnvironment(EnvironmentAction),
    NotifyUser(NotificationLevel),
    LogOnly,
}

#[derive(Debug, Clone, Serialize)]
pub enum EnvironmentAction {
    DimLights,
    WarmTemperature,
}

#[derive(Debug, Clone, Serialize)]
pub enum NotificationLevel {
    Standard,
    Critical,
}
