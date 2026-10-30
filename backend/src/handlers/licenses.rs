//! Licencias por key para módulos DLC (SaaS) + catálogo público.
//!
//! - `GET /api/v1/modules` (con JWT): catálogo con `licensed` por módulo para
//!   que el frontend muestre/oculte DLCs. No expone keys ni hashes.
//! - Admin (`admin.access`): emitir (la key en claro sale UNA vez en la
//!   respuesta), listar (sin hashes), revocar, encender/apagar módulos.
//! - Todas las queries con binds; validación de slugs contra el catálogo.

use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use chrono::{Duration, Utc};
use serde::Serialize;
use uuid::Uuid;
use validator::Validate;

use crate::{
    auth::Claims,
    error::AppError,
    middleware::auth_ext::UserPermissions,
    models::license::{
        IssueLicenseInput, License, ModuleInfo, UpdateModuleInput, hash_license_key,
        license_prefix, new_license_key,
    },
    state::AppState,
};

/// Catálogo con estado de licencia (para gating del frontend).
pub async fn list_modules(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
) -> Result<impl IntoResponse, AppError> {
    let modules = sqlx::query_as::<_, ModuleInfo>(
        "SELECT m.slug, m.name, m.description, m.version, m.enabled, m.route_prefix,
            EXISTS(SELECT 1 FROM licenses l \
             WHERE l.revoked_at IS NULL AND (l.expires_at IS NULL OR l.expires_at > now()) \
             AND (m.slug = ANY(l.modules) OR '*' = ANY(l.modules))) AS licensed
         FROM modules m ORDER BY m.slug",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(modules))
}

#[derive(Debug, Serialize)]
pub struct IssuedLicense {
    pub id: Uuid,
    pub label: String,
    pub key_prefix: String,
    pub modules: Vec<String>,
    /// La key en claro: se muestra UNA sola vez. No se almacena.
    pub key: String,
    pub seats: Option<i32>,
    pub expires_at: Option<chrono::DateTime<Utc>>,
}

/// Emite una licencia (admin). La key en claro solo sale en esta respuesta.
pub async fn issue_license(
    State(state): State<AppState>,
    perms: UserPermissions,
    Json(input): Json<IssueLicenseInput>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("admin.access")
        .map_err(|_| AppError::Forbidden)?;
    input.validate()?;
    let modules = input.validated_modules()?;
    // Los slugs deben existir en el catálogo (salvo '*' = plan total).
    for m in modules.iter().filter(|m| *m != "*") {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM modules WHERE slug = $1)")
                .bind(m)
                .fetch_one(&state.db)
                .await?;
        if !exists {
            return Err(AppError::Validation(format!("módulo desconocido: {m}")));
        }
    }
    let key = new_license_key();
    let expires_at = input
        .expires_in_days
        .map(|d| Utc::now() + Duration::days(d));
    let row: (
        Uuid,
        String,
        Vec<String>,
        Option<i32>,
        Option<chrono::DateTime<Utc>>,
    ) = sqlx::query_as(
        "INSERT INTO licenses (label, key_hash, key_prefix, modules, seats, expires_at, metadata) \
             VALUES ($1, $2, $3, $4, $5, $6, COALESCE($7, '{}')) \
             RETURNING id, label, modules, seats, expires_at",
    )
    .bind(input.label.trim())
    .bind(hash_license_key(&key))
    .bind(license_prefix(&key))
    .bind(&modules)
    .bind(input.seats)
    .bind(expires_at)
    .bind(input.metadata.clone())
    .fetch_one(&state.db)
    .await?;
    let _ = crate::handlers::audit::insert_audit_log(
        &state.db,
        None,
        "license_issued",
        "license",
        row.0,
        None,
        Some(serde_json::json!({ "label": row.1, "modules": row.2 })),
    )
    .await;
    Ok((
        StatusCode::CREATED,
        Json(IssuedLicense {
            id: row.0,
            label: row.1,
            key_prefix: license_prefix(&key),
            modules: row.2,
            key,
            seats: row.3,
            expires_at: row.4,
        }),
    ))
}

/// Lista licencias (admin). Sin hashes ni keys.
pub async fn list_licenses(
    State(state): State<AppState>,
    perms: UserPermissions,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("admin.access")
        .map_err(|_| AppError::Forbidden)?;
    let licenses = sqlx::query_as::<_, License>(
        "SELECT id, label, key_prefix, modules, seats, expires_at, \
            revoked_at, metadata, created_at, updated_at \
         FROM licenses ORDER BY created_at DESC",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(licenses))
}

/// Revoca una licencia (admin). Idempotente: revocar dos veces -> 404 la 2ª.
pub async fn revoke_license(
    State(state): State<AppState>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("admin.access")
        .map_err(|_| AppError::Forbidden)?;
    let n =
        sqlx::query("UPDATE licenses SET revoked_at = now() WHERE id = $1 AND revoked_at IS NULL")
            .bind(id)
            .execute(&state.db)
            .await?
            .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound);
    }
    let _ = crate::handlers::audit::insert_audit_log(
        &state.db,
        None,
        "license_revoked",
        "license",
        id,
        None,
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// Enciende/apaga un módulo del catálogo (admin, kill switch).
pub async fn set_module(
    State(state): State<AppState>,
    perms: UserPermissions,
    Path(slug): Path<String>,
    Json(input): Json<UpdateModuleInput>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("admin.access")
        .map_err(|_| AppError::Forbidden)?;
    input.validate()?;
    let n = sqlx::query("UPDATE modules SET enabled = $1 WHERE slug = $2")
        .bind(input.enabled)
        .bind(slug.trim())
        .execute(&state.db)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}
