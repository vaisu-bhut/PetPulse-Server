use axum::{
    extract::{Extension, Path},
    response::IntoResponse,
    Json,
};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set, QueryOrder};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{error, info};
use uuid::Uuid;
use crate::entities::{alerts, prelude::*};

#[derive(Serialize)]
pub struct AlertResponse {
    pub id: Uuid,
    pub pet_id: i32,
    pub alert_type: String,
    pub severity_level: String,
    pub message: Option<String>,
    pub critical_indicators: Option<serde_json::Value>,
    pub recommended_actions: Option<serde_json::Value>,
    pub created_at: chrono::NaiveDateTime,
    pub outcome: Option<String>,
    pub user_response: Option<String>,
    pub user_acknowledged_at: Option<chrono::NaiveDateTime>,
}

#[derive(Deserialize)]
pub struct AcknowledgeRequest {
    pub response: String,
}

// GET /alerts/critical
pub async fn get_pending_critical_alerts(
    Extension(db): Extension<DatabaseConnection>,
) ->  impl IntoResponse {
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
            let response: Vec<AlertResponse> = alerts.into_iter().map(|alert| AlertResponse {
                id: alert.id,
                pet_id: alert.pet_id,
                alert_type: alert.alert_type,
                severity_level: alert.severity_level,
                message: alert.message,
                critical_indicators: alert.critical_indicators,
                recommended_actions: alert.recommended_actions,
                created_at: alert.created_at,
                outcome: alert.outcome,
                user_response: alert.user_response,
                user_acknowledged_at: alert.user_acknowledged_at,
            }).collect();
            
            (axum::http::StatusCode::OK, Json(response)).into_response()
        }
        Err(e) => {
            error!("Failed to fetch critical alerts: {}", e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch alerts").into_response()
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
             return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response()
        }
    };

    let mut active_model: alerts::ActiveModel = alert.into();
    active_model.user_acknowledged_at = Set(Some(chrono::Utc::now().naive_utc()));
    active_model.user_response = Set(Some(payload.response));
    active_model.outcome = Set(Some("Acknowledged by User".to_string()));

    match active_model.update(&db).await {
        Ok(_) => (axum::http::StatusCode::OK, Json(serde_json::json!({"status": "acknowledged"}))).into_response(),
        Err(e) => {
            error!("Failed to acknowledge alert: {}", e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to update alert").into_response()
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
             return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response()
        }
    };

    let mut active_model: alerts::ActiveModel = alert.into();
    active_model.outcome = Set(Some("Resolved".to_string())); // Standardized string

    match active_model.update(&db).await {
        Ok(_) => (axum::http::StatusCode::OK, Json(serde_json::json!({"status": "resolved"}))).into_response(),
        Err(e) => {
             error!("Failed to resolve alert: {}", e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to update alert").into_response()
        }
    }
}
