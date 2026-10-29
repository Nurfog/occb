//! Endpoints de ejemplo protegidos (demuestran el patrón anti-SQLi).
//!
//! - `get_user`: `Path<Uuid>` (parseo estricto) + bind.
//! - `list_users`: paginación acotada + ORDER BY de lista blanca + LIKE con
//!   escape. El `format!` SOLO interpola constantes de la lista blanca.

use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    response::IntoResponse,
};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    auth::Claims,
    error::AppError,
    models::{ListQuery, UserPublic, escape_like, normalize_list},
    state::AppState,
};

pub async fn get_user(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let user: Option<UserPublic> = sqlx::query_as::<_, UserPublic>(
        "SELECT u.id, u.email, u.name, p.name AS profile, u.created_at \
         FROM users u JOIN profiles p ON p.id = u.profile_id \
         WHERE u.id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?;
    user.map(Json).ok_or(AppError::NotFound)
}

pub async fn list_users(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    Query(q): Query<ListQuery>,
) -> Result<impl IntoResponse, AppError> {
    let (page, per_page, sort_col, sort_dir) = normalize_list(&q);
    let offset = (page - 1) * per_page;
    let pattern = q.q.as_deref().map(|s| format!("%{}%", escape_like(s)));

    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM users u \
         WHERE ($1::text IS NULL \
            OR u.email::text ILIKE $1 ESCAPE '\\' \
            OR u.name ILIKE $1 ESCAPE '\\')",
    )
    .bind(pattern.as_deref())
    .fetch_one(&state.db)
    .await?;

    // sort_col/sort_dir son constantes de lista blanca (ver normalize_list).
    let sql = format!(
        "SELECT u.id, u.email, u.name, p.name AS profile, u.created_at \
         FROM users u JOIN profiles p ON p.id = u.profile_id \
         WHERE ($1::text IS NULL \
            OR u.email::text ILIKE $1 ESCAPE '\\' \
            OR u.name ILIKE $1 ESCAPE '\\') \
         ORDER BY {sort_col} {sort_dir} LIMIT $2 OFFSET $3"
    );
    let users = sqlx::query_as::<_, UserPublic>(&sql)
        .bind(pattern.as_deref())
        .bind(per_page)
        .bind(offset)
        .fetch_all(&state.db)
        .await?;

    let body: Value = json!({ "data": users, "page": page, "per_page": per_page, "total": total });
    Ok(Json(body))
}
