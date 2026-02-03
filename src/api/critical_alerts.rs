use crate::entities::{alerts, pet, prelude::*};
use axum::{
    extract::{Extension, Path, Query},
    response::IntoResponse,
    Json,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, Set,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{error, info};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct PaginationParams {
    #[serde(default = "default_page")]
    pub page: u64,
    #[serde(default = "default_page_size")]
    pub page_size: u64,
    pub severity_level: Option<String>,
}

fn default_page() -> u64 {
    1
}
fn default_page_size() -> u64 {
    10
}

#[derive(Serialize)]
pub struct AlertResponse {
    pub id: Uuid,
    pub pet_id: i32,
    pub pet_name: Option<String>,
    pub alert_type: String,
    pub severity_level: String,
    pub message: Option<String>,
    pub critical_indicators: Option<serde_json::Value>,
    pub recommended_actions: Option<serde_json::Value>,
    pub created_at: chrono::NaiveDateTime,
    pub outcome: Option<String>,
    pub user_response: Option<String>,
    pub user_acknowledged_at: Option<chrono::NaiveDateTime>,
    pub user_notified_at: Option<chrono::NaiveDateTime>,
    pub notification_sent: bool,

    pub notification_channels: Option<serde_json::Value>,
    pub intervention_action: Option<String>,
    pub video_id: Option<String>,
}

#[derive(Serialize)]
pub struct AlertListResponse {
    pub alerts: Vec<AlertResponse>,
    pub total: u64,
    pub page: u64,
    pub page_size: u64,
}

#[derive(Deserialize)]
pub struct AcknowledgeRequest {
    pub response: String,
}

// GET /alerts - List all alerts for authenticated user
pub async fn list_user_alerts(
    Extension(db): Extension<DatabaseConnection>,
    Extension(user_id): Extension<i32>,
    Query(params): Query<PaginationParams>,
) -> impl IntoResponse {
    // Get all pets for this user first
    let user_pets = match pet::Entity::find()
        .filter(pet::Column::UserId.eq(user_id))
        .all(&db)
        .await
    {
        Ok(pets) => pets,
        Err(e) => {
            error!("Failed to fetch user pets: {}", e);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch pets",
            )
                .into_response();
        }
    };

    let pet_ids: Vec<i32> = user_pets.iter().map(|p| p.id).collect();

    if pet_ids.is_empty() {
        return (
            axum::http::StatusCode::OK,
            Json(AlertListResponse {
                alerts: vec![],
                total: 0,
                page: params.page,
                page_size: params.page_size,
            }),
        )
            .into_response();
    }

    // Build query
    let mut query = Alerts::find().filter(alerts::Column::PetId.is_in(pet_ids.clone()));

    if let Some(severity) = &params.severity_level {
        query = query.filter(alerts::Column::SeverityLevel.eq(severity));
    }

    query = query.order_by_desc(alerts::Column::CreatedAt);

    // Get total count
    let total = match query.clone().count(&db).await {
        Ok(count) => count,
        Err(e) => {
            error!("Failed to count alerts: {}", e);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to count alerts",
            )
                .into_response();
        }
    };

    // Fetch paginated results using paginate method
    let paginator = query.paginate(&db, params.page_size);
    let alerts_result = paginator.fetch_page(params.page - 1).await;

    match alerts_result {
        Ok(alerts) => {
            // Create a map of pet_id to pet_name for quick lookup
            let pet_map: std::collections::HashMap<i32, String> =
                user_pets.into_iter().map(|p| (p.id, p.name)).collect();

            let response: Vec<AlertResponse> = alerts
                .into_iter()
                .map(|alert| AlertResponse {
                    id: alert.id,
                    pet_id: alert.pet_id,
                    pet_name: pet_map.get(&alert.pet_id).cloned(),
                    alert_type: alert.alert_type,
                    severity_level: alert.severity_level,
                    message: alert.message,
                    critical_indicators: alert.critical_indicators,
                    recommended_actions: alert.recommended_actions,
                    created_at: alert.created_at,
                    outcome: alert.outcome,
                    user_response: alert.user_response,
                    user_acknowledged_at: alert.user_acknowledged_at,
                    user_notified_at: alert.user_notified_at,
                    notification_sent: alert.notification_sent,

                    notification_channels: alert.notification_channels,
                    intervention_action: alert.intervention_action,
                    video_id: alert
                        .payload
                        .get("video_id")
                        .and_then(|v| v.as_str().map(String::from)),
                })
                .collect();

            (
                axum::http::StatusCode::OK,
                Json(AlertListResponse {
                    alerts: response,
                    total,
                    page: params.page,
                    page_size: params.page_size,
                }),
            )
                .into_response()
        }
        Err(e) => {
            error!("Failed to fetch alerts: {}", e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch alerts",
            )
                .into_response()
        }
    }
}

// GET /pets/:id/alerts - List alerts for specific pet
pub async fn list_pet_alerts(
    Extension(db): Extension<DatabaseConnection>,
    Extension(user_id): Extension<i32>,
    Path(pet_id): Path<i32>,
    Query(params): Query<PaginationParams>,
) -> impl IntoResponse {
    // Verify pet belongs to user
    let pet = match pet::Entity::find_by_id(pet_id).one(&db).await {
        Ok(Some(p)) if p.user_id == user_id => p,
        Ok(Some(_)) => return (axum::http::StatusCode::FORBIDDEN, "Not your pet").into_response(),
        Ok(None) => return (axum::http::StatusCode::NOT_FOUND, "Pet not found").into_response(),
        Err(e) => {
            error!("Failed to fetch pet: {}", e);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Database error",
            )
                .into_response();
        }
    };

    // Build query
    let mut query = Alerts::find().filter(alerts::Column::PetId.eq(pet_id));

    if let Some(severity) = &params.severity_level {
        query = query.filter(alerts::Column::SeverityLevel.eq(severity));
    }

    query = query.order_by_desc(alerts::Column::CreatedAt);

    // Get total count
    let total = match query.clone().count(&db).await {
        Ok(count) => count,
        Err(e) => {
            error!("Failed to count alerts: {}", e);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to count alerts",
            )
                .into_response();
        }
    };

    // Fetch paginated results using paginate method
    let paginator = query.paginate(&db, params.page_size);
    let alerts_result = paginator.fetch_page(params.page - 1).await;

    match alerts_result {
        Ok(alerts) => {
            let response: Vec<AlertResponse> = alerts
                .into_iter()
                .map(|alert| AlertResponse {
                    id: alert.id,
                    pet_id: alert.pet_id,
                    pet_name: Some(pet.name.clone()),
                    alert_type: alert.alert_type,
                    severity_level: alert.severity_level,
                    message: alert.message,
                    critical_indicators: alert.critical_indicators,
                    recommended_actions: alert.recommended_actions,
                    created_at: alert.created_at,
                    outcome: alert.outcome,
                    user_response: alert.user_response,
                    user_acknowledged_at: alert.user_acknowledged_at,
                    user_notified_at: alert.user_notified_at,
                    notification_sent: alert.notification_sent,
                    notification_channels: alert.notification_channels,
                    intervention_action: alert.intervention_action,
                    video_id: alert
                        .payload
                        .get("video_id")
                        .and_then(|v| v.as_str().map(String::from)),
                })
                .collect();

            (
                axum::http::StatusCode::OK,
                Json(AlertListResponse {
                    alerts: response,
                    total,
                    page: params.page,
                    page_size: params.page_size,
                }),
            )
                .into_response()
        }
        Err(e) => {
            error!("Failed to fetch alerts: {}", e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch alerts",
            )
                .into_response()
        }
    }
}

// GET /alerts/critical
pub async fn get_pending_critical_alerts(
    Extension(db): Extension<DatabaseConnection>,
) -> impl IntoResponse {
    // Fetch critical alerts that are NOT resolved
    // Logic: outcome IS NOT "resolved" (case insensitive check ideal, but simplistic for now)
    // Or just all critical alerts sorted by recency

    let alerts_result = Alerts::find()
        .filter(alerts::Column::SeverityLevel.eq("critical"))
        //.filter(alerts::Column::Outcome.ne("resolved")) // Simplification: fetch all for dashboard
        .order_by_desc(alerts::Column::CreatedAt)
        .all(&db)
        .await;

    match alerts_result {
        Ok(alerts) => {
            let response: Vec<AlertResponse> = alerts
                .into_iter()
                .map(|alert| AlertResponse {
                    id: alert.id,
                    pet_id: alert.pet_id,
                    pet_name: None,
                    alert_type: alert.alert_type,
                    severity_level: alert.severity_level,
                    message: alert.message,
                    critical_indicators: alert.critical_indicators,
                    recommended_actions: alert.recommended_actions,
                    created_at: alert.created_at,
                    outcome: alert.outcome,
                    user_response: alert.user_response,
                    user_acknowledged_at: alert.user_acknowledged_at,
                    user_notified_at: alert.user_notified_at,
                    notification_sent: alert.notification_sent,
                    notification_channels: alert.notification_channels,
                    intervention_action: alert.intervention_action,
                    video_id: alert
                        .payload
                        .get("video_id")
                        .and_then(|v| v.as_str().map(String::from)),
                })
                .collect();

            (axum::http::StatusCode::OK, Json(response)).into_response()
        }
        Err(e) => {
            error!("Failed to fetch critical alerts: {}", e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch alerts",
            )
                .into_response()
        }
    }
}

// POST /alerts/:id/acknowledge
pub async fn acknowledge_alert(
    Extension(db): Extension<DatabaseConnection>,
    Path(alert_id): Path<Uuid>,
    Json(payload): Json<AcknowledgeRequest>,
) -> impl IntoResponse {
    let alert = match Alerts::find_by_id(alert_id).one(&db).await {
        Ok(Some(a)) => a,
        Ok(None) => return (axum::http::StatusCode::NOT_FOUND, "Alert not found").into_response(),
        Err(e) => {
            error!("Failed to fetch alert: {}", e);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Database error",
            )
                .into_response();
        }
    };

    let mut active_model: alerts::ActiveModel = alert.into();
    active_model.user_acknowledged_at = Set(Some(chrono::Utc::now().naive_utc()));
    active_model.user_response = Set(Some(payload.response));
    active_model.outcome = Set(Some("Acknowledged by User".to_string()));

    // Calculate duration
    if let Ok(Some(alert_ro)) = alerts::Entity::find_by_id(alert_id).one(&db).await {
        let duration = chrono::Utc::now()
            .naive_utc()
            .signed_duration_since(alert_ro.created_at);
        crate::metrics::record_acknowledgment_time(duration.num_seconds() as f64);
    }

    match active_model.update(&db).await {
        Ok(_) => (
            axum::http::StatusCode::OK,
            Json(serde_json::json!({"status": "acknowledged"})),
        )
            .into_response(),
        Err(e) => {
            error!("Failed to acknowledge alert: {}", e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to update alert",
            )
                .into_response()
        }
    }
}

// POST /alerts/:id/resolve
pub async fn resolve_alert(
    Extension(db): Extension<DatabaseConnection>,
    Path(alert_id): Path<Uuid>,
) -> impl IntoResponse {
    let alert = match Alerts::find_by_id(alert_id).one(&db).await {
        Ok(Some(a)) => a,
        Ok(None) => return (axum::http::StatusCode::NOT_FOUND, "Alert not found").into_response(),
        Err(e) => {
            error!("Failed to fetch alert: {}", e);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Database error",
            )
                .into_response();
        }
    };

    let mut active_model: alerts::ActiveModel = alert.into();
    active_model.outcome = Set(Some("Resolved".to_string())); // Standardized string

    match active_model.update(&db).await {
        Ok(_) => (
            axum::http::StatusCode::OK,
            Json(serde_json::json!({"status": "resolved"})),
        )
            .into_response(),
        Err(e) => {
            error!("Failed to resolve alert: {}", e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to update alert",
            )
                .into_response()
        }
    }
}

// GET /alerts/:id
pub async fn get_alert(
    Extension(db): Extension<DatabaseConnection>,
    Path(alert_id): Path<Uuid>,
) -> impl IntoResponse {
    let alert = match Alerts::find_by_id(alert_id).one(&db).await {
        Ok(Some(a)) => a,
        Ok(None) => return (axum::http::StatusCode::NOT_FOUND, "Alert not found").into_response(),
        Err(e) => {
            error!("Failed to fetch alert: {}", e);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Database error",
            )
                .into_response();
        }
    };

    let pet_name = match pet::Entity::find_by_id(alert.pet_id).one(&db).await {
        Ok(Some(p)) => Some(p.name),
        _ => None,
    };

    let response = AlertResponse {
        id: alert.id,
        pet_id: alert.pet_id,
        pet_name,
        alert_type: alert.alert_type,
        severity_level: alert.severity_level,
        message: alert.message,
        critical_indicators: alert.critical_indicators,
        recommended_actions: alert.recommended_actions,
        created_at: alert.created_at,
        outcome: alert.outcome,
        user_response: alert.user_response,
        user_acknowledged_at: alert.user_acknowledged_at,
        user_notified_at: alert.user_notified_at,
        notification_sent: alert.notification_sent,
        notification_channels: alert.notification_channels,
        intervention_action: alert.intervention_action,
        video_id: alert
            .payload
            .get("video_id")
            .and_then(|v| v.as_str().map(String::from)),
    };

    (axum::http::StatusCode::OK, Json(response)).into_response()
}
