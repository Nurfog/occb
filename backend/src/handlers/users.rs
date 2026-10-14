//! Endpoints de ejemplo protegidos (demuestran el patrón anti-SQLi).
//!
//! - `get_user`: `Path<Uuid>` (parseo estricto) + bind.
//! - `list_users`: paginación acotada + ORDER BY de lista blanca + LIKE con
//!   escape. El `format!` SOLO interpola constantes de la lista blanca.

use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    Extension, Json,
};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    auth::Claims,
    error::AppError,
    models::{escape_like, normalize_list, ListQuery, UserPublic},
    state::AppState,
};

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

pub async fn list_users(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    Query(q): Query<ListQuery>,
) -> Result<impl IntoResponse, AppError> {
    let (page, per_page, sort_col, sort_dir) = normalize_list(&q);
    let offset = (page - 1) * per_page;
    let pattern = q.q.as_deref().map(|s| format!("%{}%", escape_like(s)));

    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM users
         WHERE ($1::text IS NULL
            OR email::text ILIKE $1 ESCAPE '\\'
            OR name ILIKE $1 ESCAPE '\\')",
    )
    .bind(pattern.as_deref())
    .fetch_one(&state.db)
    .await?;

    // sort_col/sort_dir son constantes de lista blanca (ver normalize_list).
    let sql = format!(
        "SELECT id, email, name, created_at FROM users
         WHERE ($1::text IS NULL
            OR email::text ILIKE $1 ESCAPE '\\'
            OR name ILIKE $1 ESCAPE '\\')
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
