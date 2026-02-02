use axum::{
    extract::{Extension, Path},
    response::IntoResponse,
    Json,
};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};
use tracing::{error, info};
use uuid::Uuid;

use crate::entities::{alerts, emergency_contact, pet, quick_action, prelude::*, EmergencyContact, QuickAction};

#[derive(Deserialize)]
pub struct CreateQuickActionRequest {
    pub emergency_contact_id: i32,
    pub action_type: String,
    pub message: String,
    pub video_clip_ids: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct QuickActionResponse {
    pub id: Uuid,
    pub alert_id: Uuid,
    pub emergency_contact_id: i32,
    pub contact_name: String,
    pub contact_phone: String,
    pub action_type: String,
    pub message: String,
    pub video_clips: Option<serde_json::Value>,
    pub status: String,
    pub sent_at: Option<chrono::NaiveDateTime>,
    pub acknowledged_at: Option<chrono::NaiveDateTime>,
    pub error_message: Option<String>,
    pub created_at: chrono::NaiveDateTime,
}

// POST /alerts/:alert_id/quick-actions - Create and execute quick action
pub async fn create_quick_action(
    Extension(db): Extension<DatabaseConnection>,
    Extension(user_id): Extension<i32>,
    Path(alert_id): Path<Uuid>,
    Json(payload): Json<CreateQuickActionRequest>,
) -> impl IntoResponse {
    // Verify alert belongs to user's pet
    let alert = match Alerts::find_by_id(alert_id).one(&db).await {
        Ok(Some(a)) => a,
        Ok(None) => {
            return (axum::http::StatusCode::NOT_FOUND, "Alert not found").into_response()
        }
        Err(e) => {
            error!("Failed to fetch alert: {}", e);
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Database error")
                .into_response();
        }
    };

    // Get pet to verify ownership
    match Pet::find_by_id(alert.pet_id).one(&db).await {
        Ok(Some(p)) if p.user_id == user_id => {},
        Ok(Some(_)) => return (axum::http::StatusCode::FORBIDDEN, "Not your alert").into_response(),
        Ok(None) => return (axum::http::StatusCode::NOT_FOUND, "Pet not found").into_response(),
        Err(e) => {
            error!("Failed to fetch pet: {}", e);
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Database error")
                .into_response();
        }
    };

    // Verify emergency contact belongs to user
    let contact = match EmergencyContact::find_by_id(payload.emergency_contact_id)
        .one(&db)
        .await
    {
        Ok(Some(c)) if c.user_id == user_id => c,
        Ok(Some(_)) => {
            return (
                axum::http::StatusCode::FORBIDDEN,
                "Not your emergency contact",
            )
                .into_response()
        }
        Ok(None) => {
            return (
                axum::http::StatusCode::NOT_FOUND,
                "Emergency contact not found",
            )
                .into_response()
        }
        Err(e) => {
            error!("Failed to fetch emergency contact: {}", e);
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Database error")
                .into_response();
        }
    };

    // Create quick action
    let now = chrono::Utc::now().naive_utc();
    let video_clips_json = payload
        .video_clip_ids
        .map(|ids| serde_json::to_value(ids).ok())
        .flatten();

    let active_model = quick_action::ActiveModel {
        id: Set(Uuid::new_v4()),
        alert_id: Set(alert_id),
        emergency_contact_id: Set(payload.emergency_contact_id),
        action_type: Set(payload.action_type.clone()),
        message: Set(payload.message.clone()),
        video_clips: Set(video_clips_json.clone()),
        status: Set("pending".to_string()),
        sent_at: Set(None),
        acknowledged_at: Set(None),
        error_message: Set(None),
        created_at: Set(now),
    };

    let action = match active_model.insert(&db).await {
        Ok(a) => a,
        Err(e) => {
            error!("Failed to create quick action: {}", e);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to create quick action",
            )
                .into_response();
        }
    };

    // TODO: Actually send the message via SMS/Email (Twilio integration)
    // For now, just mark as "sent" immediately
    let mut active_action: quick_action::ActiveModel = action.clone().into();
    active_action.status = Set("sent".to_string());
    active_action.sent_at = Set(Some(now));

    let updated_action = match active_action.update(&db).await {
        Ok(a) => a,
        Err(e) => {
            error!("Failed to update quick action status: {}", e);
            // Return the original action even if update fails
            action
        }
    };

    info!(
        "Created and executed quick action {} for alert {}",
        updated_action.id, alert_id
    );

    let response = QuickActionResponse {
        id: updated_action.id,
        alert_id: updated_action.alert_id,
        emergency_contact_id: updated_action.emergency_contact_id,
        contact_name: contact.name,
        contact_phone: contact.phone,
        action_type: updated_action.action_type,
        message: updated_action.message,
        video_clips: updated_action.video_clips,
        status: updated_action.status,
        sent_at: updated_action.sent_at,
        acknowledged_at: updated_action.acknowledged_at,
        error_message: updated_action.error_message,
        created_at: updated_action.created_at,
    };

    (axum::http::StatusCode::CREATED, Json(response)).into_response()
}

// GET /alerts/:alert_id/quick-actions - List quick actions for an alert
pub async fn list_alert_quick_actions(
    Extension(db): Extension<DatabaseConnection>,
    Extension(user_id): Extension<i32>,
    Path(alert_id): Path<Uuid>,
) -> impl IntoResponse {
    // Verify alert belongs to user's pet
    let alert = match Alerts::find_by_id(alert_id).one(&db).await {
        Ok(Some(a)) => a,
        Ok(None) => {
            return (axum::http::StatusCode::NOT_FOUND, "Alert not found").into_response()
        }
        Err(e) => {
            error!("Failed to fetch alert: {}", e);
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Database error")
                .into_response();
        }
    };

    match Pet::find_by_id(alert.pet_id).one(&db).await {
        Ok(Some(p)) if p.user_id == user_id => {},
        Ok(Some(_)) => return (axum::http::StatusCode::FORBIDDEN, "Not your alert").into_response(),
        Ok(None) => return (axum::http::StatusCode::NOT_FOUND, "Pet not found").into_response(),
        Err(e) => {
            error!("Failed to fetch pet: {}", e);
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Database error")
                .into_response();
        }
    };

    // Get quick actions
    let actions: Vec<quick_action::Model> = match QuickAction::find()
        .filter(quick_action::Column::AlertId.eq(alert_id))
        .all(&db)
        .await
    {
        Ok(a) => a,
        Err(e) => {
            error!("Failed to fetch quick actions: {}", e);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch quick actions",
            )
                .into_response();
        }
    };

    // Get emergency contacts for these actions
    let contact_ids: Vec<i32> = actions.iter().map(|a| a.emergency_contact_id).collect();
    let contacts: Vec<emergency_contact::Model> = match EmergencyContact::find()
        .filter(emergency_contact::Column::Id.is_in(contact_ids))
        .all(&db)
        .await
    {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to fetch emergency contacts: {}", e);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch contacts",
            )
                .into_response();
        }
    };

    let contact_map: std::collections::HashMap<i32, emergency_contact::Model> =
        contacts.into_iter().map(|c| (c.id, c)).collect();

    let response: Vec<QuickActionResponse> = actions
        .into_iter()
        .map(|action| {
            let contact = contact_map.get(&action.emergency_contact_id);
            QuickActionResponse {
                id: action.id,
                alert_id: action.alert_id,
                emergency_contact_id: action.emergency_contact_id,
                contact_name: contact.map(|c| c.name.clone()).unwrap_or_default(),
                contact_phone: contact.map(|c| c.phone.clone()).unwrap_or_default(),
                action_type: action.action_type,
                message: action.message,
                video_clips: action.video_clips,
                status: action.status,
                sent_at: action.sent_at,
                acknowledged_at: action.acknowledged_at,
                error_message: action.error_message,
                created_at: action.created_at,
            }
        })
        .collect();

    (axum::http::StatusCode::OK, Json(response)).into_response()
}
