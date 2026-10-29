//! Endpoints de administración (requieren `admin.access`).

use axum::{Extension, Json, extract::State, response::IntoResponse};
use serde_json::{Value, json};

use crate::{auth::Claims, middleware::authorize::UserPermissions, state::AppState};

pub async fn ping(
    State(_state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Extension(perms): Extension<UserPermissions>,
) -> impl IntoResponse {
    let body: Value = json!({
        "status": "ok",
        "user_id": claims.sub,
        "permissions": perms.0,
    });
    Json(body)
}
