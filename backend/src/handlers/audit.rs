//! Endpoints de auditoría (lectura) + helper de escritura.
//!
//! Solo-autenticado (`Extension<Claims>`); filtros siempre con binds y
//! LIMIT/OFFSET i64 bindeados.

use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    response::IntoResponse,
};
use sqlx::PgPool;
use uuid::Uuid;
use validator::Validate;

use crate::{
    auth::Claims,
    error::AppError,
    middleware::auth_ext::UserPermissions,
    models::audit::{AuditFilter, AuditLog},
    state::AppState,
};

/// Inserta una fila de auditoría. Devuelve `sqlx::Error` para que los
/// llamantes puedan ignorarla (`let _ = ...`) sin convertir a 5xx.
pub async fn insert_audit_log(
    pool: &PgPool,
    user_id: Option<Uuid>,
    action: &str,
    entity_type: &str,
    entity_id: Uuid,
    old_values: Option<serde_json::Value>,
    new_values: Option<serde_json::Value>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO audit_log (user_id, action, entity_type, entity_id, old_values, new_values) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(user_id)
    .bind(action)
    .bind(entity_type)
    .bind(entity_id)
    .bind(old_values)
    .bind(new_values)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_audit_logs(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(_claims): Extension<Claims>,
    Query(params): Query<AuditFilter>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("audit.view")
        .map_err(|_| AppError::Forbidden)?;
    params.validate()?;
    let per_page = params.per_page_clamped();
    let offset = params.offset();

    let logs = sqlx::query_as::<_, AuditLog>(
        "SELECT id, user_id, action, entity_type, entity_id, old_values, new_values, \
            ip_address::text, created_at FROM audit_log \
         WHERE ($1::text IS NULL OR entity_type = $1) \
           AND ($2::uuid IS NULL OR entity_id = $2) \
           AND ($3::uuid IS NULL OR user_id = $3) \
         ORDER BY created_at DESC LIMIT $4 OFFSET $5",
    )
    .bind(params.entity_type.as_deref())
    .bind(params.entity_id)
    .bind(params.user_id)
    .bind(per_page)
    .bind(offset)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(logs))
}

pub async fn get_entity_history(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(_claims): Extension<Claims>,
    Path((entity_type, entity_id)): Path<(String, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("audit.view")
        .map_err(|_| AppError::Forbidden)?;
    if entity_type.is_empty() || entity_type.len() > 50 {
        return Err(AppError::Validation(
            "entity_type de 1 a 50 caracteres".into(),
        ));
    }
    let logs = sqlx::query_as::<_, AuditLog>(
        "SELECT id, user_id, action, entity_type, entity_id, old_values, new_values, \
            ip_address::text, created_at FROM audit_log \
         WHERE entity_type = $1 AND entity_id = $2 ORDER BY created_at DESC",
    )
    .bind(&entity_type)
    .bind(entity_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(logs))
}
