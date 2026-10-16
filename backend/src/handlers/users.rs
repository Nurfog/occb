//! Detalle de usuario (patrón anti-SQLi).
//!
//! - `get_user`: `Path<Uuid>` (parseo estricto) + bind.
//! - El listado vive en `admin::admin_list_users` (requiere `admin.access`).

use axum::{
    Extension, Json,
    extract::{Path, State},
    response::IntoResponse,
};
use uuid::Uuid;

use crate::{auth::Claims, error::AppError, models::UserPublic, state::AppState};

pub async fn get_user(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let user: Option<UserPublic> = sqlx::query_as::<_, UserPublic>(
        "SELECT id, email, name, created_at FROM users WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?;
    user.map(Json).ok_or(AppError::NotFound)
}
