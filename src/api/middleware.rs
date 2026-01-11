use axum::{
    extract::Request,
    response::{IntoResponse, Response},
    http::StatusCode,
    middleware::Next,
    Json,
};
use tower_cookies::Cookies;
use serde_json::json;

pub async fn auth_middleware(
    cookies: Cookies,
    mut request: Request,
    next: Next,
) -> Response {
    if let Some(cookie) = cookies.get("petpulse_user") {
        if let Ok(user_id) = cookie.value().parse::<i32>() {
             request.extensions_mut().insert(user_id);
             return next.run(request).await;
        }
    }
    (StatusCode::UNAUTHORIZED, Json(json!({"error": "Unauthorized"}))).into_response()
}
