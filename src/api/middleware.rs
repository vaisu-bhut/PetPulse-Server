use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use tower_cookies::Cookies;

use crate::entities::user;
use axum::extract::Extension;
use sea_orm::{DatabaseConnection, EntityTrait};

pub async fn auth_middleware(
    Extension(db): Extension<DatabaseConnection>,
    cookies: Cookies,
    mut request: Request,
    next: Next,
) -> Response {
    if let Some(cookie) = cookies.get("petpulse_user") {
        if let Ok(user_id) = cookie.value().parse::<i32>() {
            // Check DB for email to log
            if let Ok(Some(user)) = user::Entity::find_by_id(user_id).one(&db).await {
                request.extensions_mut().insert(user_id);
                // Record email and user_id to span
                tracing::Span::current()
                    .record("user_id", user_id)
                    .record("user_email", &user.email);

                return next.run(request).await;
            }
        }
    }
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error": "Unauthorized"})),
    )
        .into_response()
}
