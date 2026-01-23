use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use tower_cookies::Cookies;

pub async fn auth_middleware(cookies: Cookies, mut request: Request, next: Next) -> Response {
    if let Some(cookie) = cookies.get("petpulse_user") {
        if let Ok(user_id) = cookie.value().parse::<i32>() {
            request.extensions_mut().insert(user_id);
            return next.run(request).await;
        }
    }
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error": "Unauthorized"})),
    )
        .into_response()
}
