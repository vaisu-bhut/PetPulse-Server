use axum::{
    extract::{Extension, Path},
    response::IntoResponse,
    Json,
};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set, ModelTrait};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::entities::{emergency_contact, prelude::*, EmergencyContact};

#[derive(Deserialize)]
pub struct CreateEmergencyContactRequest {
    pub contact_type: String,
    pub name: String,
    pub phone: String,
    pub email: Option<String>,
    pub address: Option<String>,
    pub notes: Option<String>,
    pub priority: Option<i32>,
}

#[derive(Deserialize)]
pub struct UpdateEmergencyContactRequest {
    pub contact_type: Option<String>,
    pub name: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub address: Option<String>,
    pub notes: Option<String>,
    pub priority: Option<i32>,
    pub is_active: Option<bool>,
}

#[derive(Serialize)]
pub struct EmergencyContactResponse {
    pub id: i32,
    pub user_id: i32,
    pub contact_type: String,
    pub name: String,
    pub phone: String,
    pub email: Option<String>,
    pub address: Option<String>,
    pub notes: Option<String>,
    pub priority: i32,
    pub is_active: bool,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

impl From<emergency_contact::Model> for EmergencyContactResponse {
    fn from(model: emergency_contact::Model) -> Self {
        Self {
            id: model.id,
            user_id: model.user_id,
            contact_type: model.contact_type,
            name: model.name,
            phone: model.phone,
            email: model.email,
            address: model.address,
            notes: model.notes,
            priority: model.priority,
            is_active: model.is_active,
            created_at: model.created_at,
            updated_at: model.updated_at,
        }
    }
}

// GET /emergency-contacts - List all emergency contacts for authenticated user
pub async fn list_emergency_contacts(
    Extension(db): Extension<DatabaseConnection>,
    Extension(user_id): Extension<i32>,
) -> impl IntoResponse {
    match EmergencyContact::find()
        .filter(emergency_contact::Column::UserId.eq(user_id))
        .all(&db)
        .await
    {
        Ok(contacts) => {
            let response: Vec<EmergencyContactResponse> =
                contacts.into_iter().map(|c| c.into()).collect();
            (axum::http::StatusCode::OK, Json(response)).into_response()
        }
        Err(e) => {
            error!("Failed to fetch emergency contacts: {}", e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch emergency contacts",
            )
                .into_response()
        }
    }
}

// POST /emergency-contacts - Create new emergency contact
pub async fn create_emergency_contact(
    Extension(db): Extension<DatabaseConnection>,
    Extension(user_id): Extension<i32>,
    Json(payload): Json<CreateEmergencyContactRequest>,
) -> impl IntoResponse {
    let now = chrono::Utc::now().naive_utc();

    let active_model = emergency_contact::ActiveModel {
        user_id: Set(user_id),
        contact_type: Set(payload.contact_type),
        name: Set(payload.name),
        phone: Set(payload.phone),
        email: Set(payload.email),
        address: Set(payload.address),
        notes: Set(payload.notes),
        priority: Set(payload.priority.unwrap_or(0)),
        is_active: Set(true),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };

    match active_model.insert(&db).await {
        Ok(contact) => {
            info!("Created emergency contact: {}", contact.id);
            let response: EmergencyContactResponse = contact.into();
            (axum::http::StatusCode::CREATED, Json(response)).into_response()
        }
        Err(e) => {
            error!("Failed to create emergency contact: {}", e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to create emergency contact",
            )
                .into_response()
        }
    }
}

// PATCH /emergency-contacts/:id - Update emergency contact
pub async fn update_emergency_contact(
    Extension(db): Extension<DatabaseConnection>,
    Extension(user_id): Extension<i32>,
    Path(contact_id): Path<i32>,
    Json(payload): Json<UpdateEmergencyContactRequest>,
) -> impl IntoResponse {
    // Verify contact belongs to user
    let contact = match EmergencyContact::find_by_id(contact_id)
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
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Database error",
            )
                .into_response();
        }
    };

    let mut active_model: emergency_contact::ActiveModel = contact.into();

    if let Some(contact_type) = payload.contact_type {
        active_model.contact_type = Set(contact_type);
    }
    if let Some(name) = payload.name {
        active_model.name = Set(name);
    }
    if let Some(phone) = payload.phone {
        active_model.phone = Set(phone);
    }
    if let Some(email) = payload.email {
        active_model.email = Set(Some(email));
    }
    if let Some(address) = payload.address {
        active_model.address = Set(Some(address));
    }
    if let Some(notes) = payload.notes {
        active_model.notes = Set(Some(notes));
    }
    if let Some(priority) = payload.priority {
        active_model.priority = Set(priority);
    }
    if let Some(is_active) = payload.is_active {
        active_model.is_active = Set(is_active);
    }
    active_model.updated_at = Set(chrono::Utc::now().naive_utc());

    match active_model.update(&db).await {
        Ok(contact) => {
            info!("Updated emergency contact: {}", contact.id);
            let response: EmergencyContactResponse = contact.into();
            (axum::http::StatusCode::OK, Json(response)).into_response()
        }
        Err(e) => {
            error!("Failed to update emergency contact: {}", e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to update emergency contact",
            )
                .into_response()
        }
    }
}

// DELETE /emergency-contacts/:id - Delete emergency contact
pub async fn delete_emergency_contact(
    Extension(db): Extension<DatabaseConnection>,
    Extension(user_id): Extension<i32>,
    Path(contact_id): Path<i32>,
) -> impl IntoResponse {
    // Verify contact belongs to user
    let contact = match EmergencyContact::find_by_id(contact_id)
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
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Database error",
            )
                .into_response();
        }
    };

    match contact.delete(&db).await {
        Ok(_) => {
            info!("Deleted emergency contact: {}", contact_id);
            (
                axum::http::StatusCode::OK,
                Json(serde_json::json!({"message": "Emergency contact deleted"})),
            )
                .into_response()
        }
        Err(e) => {
            error!("Failed to delete emergency contact: {}", e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to delete emergency contact",
            )
                .into_response()
        }
    }
}
