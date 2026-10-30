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
    Extension(claims): Extension<Claims>,
    Query(params): Query<AuditFilter>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("audit.view")
        .map_err(|_| AppError::Forbidden)?;
    params.validate()?;
    let scope = crate::auth::tenant_scope(&state, &claims.sub).await?;
    let per_page = params.per_page_clamped();
    let offset = params.offset();

    let logs = sqlx::query_as::<_, AuditLog>(
        "SELECT a.id, a.user_id, a.action, a.entity_type, a.entity_id, a.old_values, a.new_values, \
            a.ip_address::text, a.created_at FROM audit_log a \
          LEFT JOIN users u ON u.id = a.user_id \
         WHERE ($1::text IS NULL OR a.entity_type = $1) \
           AND ($2::uuid IS NULL OR a.entity_id = $2) \
           AND ($3::uuid IS NULL OR a.user_id = $3) \
           AND ($6::uuid IS NULL OR u.tenant_id = $6) \
         ORDER BY a.created_at DESC LIMIT $4 OFFSET $5",
    )
    .bind(params.entity_type.as_deref())
    .bind(params.entity_id)
    .bind(params.user_id)
    .bind(per_page)
    .bind(offset)
    .bind(scope)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(logs))
}

pub async fn get_entity_history(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(claims): Extension<Claims>,
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
    let scope = crate::auth::tenant_scope(&state, &claims.sub).await?;
    // La entidad debe resolverse a una tabla con tenant_id; tipo desconocido -> 404.
    // (nombres fijos por rama: sin interpolación de input en el SQL).
    let tabla: &str = match entity_type.as_str() {
        "contact" => "contacts",
        "company" => "companies",
        "deal" => "deals",
        "lead" => "leads",
        "activity" => "activities",
        _ => return Err(AppError::NotFound),
    };
    // Platform (scope None) mantiene el comportamiento legacy; empresa solo
    // ve el historial si la entidad es de su tenant (no visible -> 404).
    if scope.is_some() {
        let visible: bool = sqlx::query_scalar(&format!(
            "SELECT EXISTS(SELECT 1 FROM {tabla} WHERE id = $1 AND tenant_id = $2)"
        ))
        .bind(entity_id)
        .bind(scope)
        .fetch_one(&state.db)
        .await?;
        if !visible {
            return Err(AppError::NotFound);
        }
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
