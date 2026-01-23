use crate::entities::user;
use axum::{
    extract::{Extension, Json},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, IntoActiveModel, Set};
use serde_json::json;

#[derive(serde::Deserialize)]
pub struct UpdateUserRequest {
    name: Option<String>,
    email: Option<String>,
}

pub async fn get_user(
    Extension(db): Extension<DatabaseConnection>,
    Extension(user_id): Extension<i32>,
) -> Response {
    match user::Entity::find_by_id(user_id).one(&db).await {
        Ok(Some(u)) => (
            StatusCode::OK,
            Json(json!({"id": u.id, "email": u.email, "name": u.name, "created_at": u.created_at})),
        )
            .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "User not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn update_user(
    Extension(db): Extension<DatabaseConnection>,
    Extension(user_id): Extension<i32>,
    Json(payload): Json<UpdateUserRequest>,
) -> Response {
    let user = match user::Entity::find_by_id(user_id).one(&db).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "User not found"})),
            )
                .into_response()
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
                .into_response()
        }
    };

    let mut active_user = user.into_active_model();
    if let Some(name) = payload.name {
        active_user.name = Set(name);
    }
    if let Some(email) = payload.email {
        active_user.email = Set(email);
    }
    active_user.updated_at = Set(chrono::Utc::now().naive_utc());

    match active_user.update(&db).await {
        Ok(u) => (
            StatusCode::OK,
            Json(json!({"id": u.id, "email": u.email, "name": u.name})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn delete_user(
    Extension(db): Extension<DatabaseConnection>,
    Extension(user_id): Extension<i32>,
) -> Response {
    match user::Entity::delete_by_id(user_id).exec(&db).await {
        Ok(res) if res.rows_affected == 0 => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "User not found"})),
        )
            .into_response(),
        Ok(_) => (StatusCode::OK, Json(json!({"message": "User deleted"}))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}
