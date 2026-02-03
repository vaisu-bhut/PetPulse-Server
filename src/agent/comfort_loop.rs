use crate::entities::alerts;
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, Set,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{error, info};
use uuid::Uuid;

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
    gemini: crate::gemini::GeminiClient,
}

impl ComfortLoop {
    pub async fn new(db: DatabaseConnection) -> Self {
        Self {
            db,
            notifier: TwilioNotifier::new().await,
            gemini: crate::gemini::GeminiClient::new(),
        }
    }

    pub async fn process_alert(&self, payload: AlertPayload) {
        info!("Processing alert: {:?}", payload);

        let alert_uuid = Uuid::new_v4();

        // 1. Persist Initial Alert
        // Parse pet_id from string to i32 (as per schema)
        let db_pet_id = payload.pet_id.parse::<i32>().unwrap_or_else(|e| {
            error!(
                "Failed to parse pet_id '{}': {}. Using 1 as fallback.",
                payload.pet_id, e
            );
            1
        });

        // Extract detailed fields from payload or context
        let severity_level = payload
            .severity_level
            .clone()
            .or_else(|| {
                payload.context.as_ref().and_then(|c| {
                    c.get("severity_level")
                        .and_then(|v| v.as_str().map(String::from))
                })
            })
            .unwrap_or_else(|| "low".to_string());

        // 2a. Check recent alert count for escalation (Last 1 hour)
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
                0 // Default to 0 so current alert makes it 1
            }
        };

        // Include current alert in count for logic
        let current_alert_count = recent_alert_count + 1;

        info!(
            "Alert count for pet_id={}, type={} in last hour: {} (including current)",
            db_pet_id,
            payload.alert_type.to_string(),
            current_alert_count
        );

        // 2b. Force Severity Escalation (5th+ alert = High)
        let final_severity = if current_alert_count >= 5 && severity_level != "critical" {
            info!(
                "Escalating alert {} to HIGH severity due to repetition (count: {})",
                alert_uuid, current_alert_count
            );
            "high".to_string()
        } else {
            severity_level
        };

        // 3. Persist Alert (now that we have final severity)
        let critical_indicators = payload.critical_indicators.clone().or_else(|| {
            payload.context.as_ref().and_then(|c| {
                c.get("critical_indicators")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
            })
        });

        let recommended_actions = payload.recommended_actions.clone().or_else(|| {
            payload.context.as_ref().and_then(|c| {
                c.get("recommended_actions")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
            })
        });

        let active_model = alerts::ActiveModel {
            id: Set(alert_uuid),
            pet_id: Set(db_pet_id),
            alert_type: Set(payload.alert_type.to_string()),
            severity: Set(match final_severity.as_str() {
                "critical" => "critical".to_string(),
                "high" => "high".to_string(),
                _ => payload.severity.clone(),
            }),
            message: Set(payload.message.clone()),
            severity_level: Set(final_severity.clone()),
            critical_indicators: Set(critical_indicators
                .clone()
                .map(|v| serde_json::to_value(v).unwrap_or(serde_json::Value::Null))),
            recommended_actions: Set(recommended_actions
                .clone()
                .map(|v| serde_json::to_value(v).unwrap_or(serde_json::Value::Null))),
            payload: Set(serde_json::to_value(&payload).unwrap_or_default()),
            created_at: Set(chrono::Utc::now().naive_utc()),
            ..Default::default()
        };

        if let Err(e) = alerts::Entity::insert(active_model).exec(&self.db).await {
            error!("Failed to insert alert into DB: {}", e);
            return;
        }

        info!("Alert {} persisted to database", alert_uuid);

        // 4. Decide Intervention (escalating based on count)
        let intervention = self
            .decide_intervention(&payload, current_alert_count, &final_severity)
            .await;

        // 4. Execute Action
        self.execute_action(&intervention, &payload).await;

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
        if final_severity == "critical" {
            crate::metrics::increment_critical_alerts(db_pet_id);
            // Trigger Critical Notification Branch
            self.handle_critical_alert(
                &payload,
                alert_uuid,
                &critical_indicators,
                &recommended_actions,
            )
            .await;

            // Also generate Quick Actions for Critical
            self.generate_quick_actions(alert_uuid, db_pet_id, "critical")
                .await;

            return; // Skip normal monitoring/resolution loop for critical alerts
        }

        // Handle High Severity (Persistent) - Generate Quick Actions
        if final_severity == "high" {
            self.generate_quick_actions(alert_uuid, db_pet_id, "high")
                .await;
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
        recommended_actions: &Option<Vec<String>>,
    ) {
        info!("🚨 HANDLING CRITICAL ALERT: {}", alert_uuid);

        // Fetch owner email and name from DB
        let db_pet_id = payload.pet_id.parse::<i32>().unwrap_or(1);
        let owner_info = match crate::entities::pet::Entity::find_by_id(db_pet_id)
            .find_also_related(crate::entities::user::Entity)
            .one(&self.db)
            .await
        {
            Ok(Some((pet, Some(user)))) => Some((user.email, user.name, pet.name)),
            _ => None,
        };

        let (owner_email, owner_name, pet_name) = owner_info.unwrap_or_else(|| {
            (
                std::env::var("OWNER_EMAIL").unwrap_or("test@example.com".to_string()),
                "Pet Owner".to_string(),
                "Your Pet".to_string(),
            )
        });

        let owner_phone = std::env::var("OWNER_PHONE").unwrap_or("+15550000000".to_string());

        let video_link = if let Some(vid) = &payload.video_id {
            // In a real scenario, generate a signed URL here.
            // For now, use a direct link placeholder
            format!("https://petpulse.dashboard/videos/{}", vid)
        } else {
            "https://petpulse.dashboard".to_string()
        };

        // Send Notifications
        self.notifier
            .notify_critical_alert(
                &owner_email,
                &owner_phone,
                &pet_name,
                "CRITICAL",
                payload
                    .message
                    .as_deref()
                    .unwrap_or("Critical health indicator detected"),
                critical_indicators.as_deref().unwrap_or(&[]),
                recommended_actions.as_deref().unwrap_or(&[]),
                &video_link,
            )
            .await;

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

    async fn decide_intervention(
        &self,
        payload: &AlertPayload,
        alert_count: u64,
        severity_level: &str,
    ) -> Intervention {
        // If critical, immediately escalate to Notification (handled in main loop branching, but good for safety)
        if severity_level == "critical" {
            return Intervention::NotifyUser(NotificationLevel::Critical);
        }

        // Standard Escalation Logic
        info!(
            "Deciding intervention for alert_type={:?}, alert_count={}",
            payload.alert_type, alert_count
        );
        match alert_count {
            0..=2 => match payload.alert_type {
                // 1st and 2nd alert
                AlertType::Pacing | AlertType::Restlessness => {
                    Intervention::AdjustEnvironment(EnvironmentAction::DimLights)
                }
                AlertType::Vocalization | AlertType::AttentionSeeking => {
                    Intervention::PlayCalmingMusic
                }
                AlertType::UnusualBehavior => Intervention::PlayCalmingMusic,
                _ => Intervention::LogOnly,
            },
            3 => match payload.alert_type {
                // 3rd alert
                AlertType::Pacing | AlertType::Restlessness => Intervention::PlayOwnerVoice,
                AlertType::Vocalization => Intervention::DispenseTreat,
                _ => Intervention::PlayOwnerVoice,
            },
            4 => {
                // 4th alert - Notify User AND Last Autonomous Action
                info!("Alert escalation: 4th alert - Notifying user and taking final autonomous action");
                // We return a composite or just notify for now as per "user preference" request implies notification is key.
                // But user asked for "autonomous agent one last time".
                // Let's assume we do PlayOwnerVoice + Notify.
                // Limitation: Current Intervention enum is single-choice.
                // Workaround: We will execute the autonomous action here manually, and return NotifyUser.
                let autonomous_backup = Intervention::PlayOwnerVoice;
                self.execute_action(&autonomous_backup, payload).await;

                Intervention::NotifyUser(NotificationLevel::Standard)
            }
            _ => {
                // 5+ alerts - High Severity (Controlled by final_severity logic)
                // Just notify, but strict.
                info!("Alert escalation: 5+ alerts (High Severity) - Notifying user");
                Intervention::NotifyUser(NotificationLevel::Standard)
            }
        }
    }

    async fn generate_quick_actions(&self, alert_id: Uuid, pet_id: i32, severity: &str) {
        use crate::entities::{emergency_contact, quick_action};

        // 1. Get Pet and User info
        let pet = match crate::entities::pet::Entity::find_by_id(pet_id)
            .one(&self.db)
            .await
        {
            Ok(Some(p)) => p,
            _ => {
                error!("Pet not found for quick actions");
                return;
            }
        };

        // 2. Get Emergency Contacts
        let contacts = match emergency_contact::Entity::find()
            .filter(emergency_contact::Column::UserId.eq(pet.user_id))
            .all(&self.db)
            .await
        {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to fetch contacts: {}", e);
                return;
            }
        };

        if contacts.is_empty() {
            info!("No emergency contacts found for quick actions.");
            return;
        }

        for contact in contacts {
            // 3. De-duplication: Check if there's a PENDING action for this contact
            let pending_action = quick_action::Entity::find()
                .filter(quick_action::Column::EmergencyContactId.eq(contact.id))
                .filter(quick_action::Column::Status.eq("pending"))
                .one(&self.db)
                .await
                .unwrap_or(None);

            if let Some(existing) = pending_action {
                info!("Skipping quick action generation for contact {} (Action {} is already pending)", contact.id, existing.id);
                continue;
            }

            // 4. Generate Personalized Content with Gemini
            let contact_name = &contact.name;
            let pet_name = &pet.name;
            let prompt = format!(
                "Write a concise, urgent message from a pet monitoring system regarding {}. \
                The recipient is {}, who is a {}. Severity: {}. \
                The pet is showing unusual behavior. \
                Generate a JSON object with two fields: 'sms_text' (short, <160 chars) and 'email_body' (polite, informative). \
                Do not use markdown.",
                pet_name, contact_name, contact.contact_type, severity
            );

            let message_content = match self.gemini.generate_text(&prompt).await {
                Ok(text) => text,
                Err(e) => {
                    error!("Gemini generation failed: {}", e);
                    // Fallback
                    format!(
                        r#"{{"sms_text": "PetPulse Alert: {} needs attention.", "email_body": "Please check on {}."}}"#,
                        pet_name, pet_name
                    )
                }
            };

            // 5. Create Quick Action
            // We store the JSON in the `message` field so frontend can parse both formats
            let active_action = quick_action::ActiveModel {
                id: Set(Uuid::new_v4()),
                alert_id: Set(alert_id),
                emergency_contact_id: Set(contact.id),
                action_type: Set("message".to_string()), // Generic type, content has formats
                message: Set(message_content),
                status: Set("pending".to_string()),
                created_at: Set(chrono::Utc::now().naive_utc()),
                ..Default::default()
            };

            if let Err(e) = quick_action::Entity::insert(active_action)
                .exec(&self.db)
                .await
            {
                error!("Failed to generate quick action: {}", e);
            }
        }
        info!(
            "Generated quick actions for alert {} (Severity: {})",
            alert_id, severity
        );
    }

    async fn execute_action(&self, action: &Intervention, payload: &AlertPayload) {
        info!("Executing intervention: {:?}", action);
        // TODO: Call Smart Home API / IoT Hub
        match action {
            Intervention::PlayCalmingMusic => info!("🎶 Action: Playing calming music playlist"),
            Intervention::PlayOwnerVoice => info!("🗣️ Action: Playing owner voice note"),
            Intervention::DispenseTreat => info!("🍬 Action: Dispensing treat"),
            Intervention::AdjustEnvironment(env_action) => {
                info!("💡 Action: Adjusting environment: {:?}", env_action)
            }
            Intervention::NotifyUser(level) => {
                info!("📱 Action: Notifying user (Level: {:?})", level);
                
                // Fetch owner email from DB
                let db_pet_id = payload.pet_id.parse::<i32>().unwrap_or(1);
                let owner_info = match crate::entities::pet::Entity::find_by_id(db_pet_id)
                    .find_also_related(crate::entities::user::Entity)
                    .one(&self.db)
                    .await
                {
                    Ok(Some((_, Some(user)))) => Some((user.email, user.name)),
                    _ => None,
                };

                let (owner_email, owner_name) = owner_info.unwrap_or_else(|| {
                    (std::env::var("OWNER_EMAIL").unwrap_or("test@example.com".to_string()), "Pet Owner".to_string())
                });
                
                let owner_phone = std::env::var("OWNER_PHONE").unwrap_or("+15550000000".to_string());
                
                let severity_str = match level {
                    NotificationLevel::Critical => "CRITICAL",
                    NotificationLevel::Standard => "HIGH",
                };
                
                let video_link = payload.video_id.as_ref()
                    .map(|v| format!("https://petpulse.dashboard/videos/{}", v))
                    .unwrap_or_else(|| "https://petpulse.dashboard".to_string());

                self.notifier.notify_critical_alert(
                    &owner_email,
                    &owner_phone,
                    &owner_name,
                    severity_str,
                    payload.message.as_deref().unwrap_or("Alert triggered"),
                    &[],
                    &[],
                    &video_link
                ).await;
            }
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
