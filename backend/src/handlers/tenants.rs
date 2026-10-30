//! Dashboard del superadmin: empresas (tenants) y sus administradores.
//!
//! - Todo exige `superadmin` (root instalado u otro promovido), NO basta
//!   `admin.access`: un admin de empresa nunca gestiona tenants.
//! - Alta: `POST /tenants` crea en `pendiente`; `PATCH` la habilita
//!   (`activa`) o suspende. Suspender bloquea login+API de sus usuarios.
//! - `POST /tenants/{id}/admins` crea el admin de la empresa (perfil
//!   `Administrador` + `tenant_id`). Borrado de tenant solo vacío (si no,
//!   suspender).
//! - Todas las queries con binds; validación de inputs; auditoría best-effort.

use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use uuid::Uuid;
use validator::Validate;

use crate::{
    auth::Claims,
    error::AppError,
    middleware::auth_ext::require_superadmin,
    models::{
        UserPublic,
        tenant::{
            CreateTenantAdminInput, CreateTenantInput, Tenant, TenantWithCounts, UpdateTenantInput,
            puede_transitar,
        },
    },
    password,
    state::AppState,
};

/// Lista empresas con conteo de usuarios (superadmin).
pub async fn list_tenants(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
) -> Result<impl IntoResponse, AppError> {
    require_superadmin(&state, &_claims.sub).await?;
    let tenants = sqlx::query_as::<_, TenantWithCounts>(
        "SELECT t.id, t.name, t.slug, t.status, t.contact_name, t.contact_email,
            (SELECT COUNT(*) FROM users u WHERE u.tenant_id = t.id) AS users,
            t.created_at
         FROM tenants t ORDER BY t.created_at DESC",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(tenants))
}

/// Da de alta una empresa en `pendiente` (superadmin).
pub async fn create_tenant(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    Json(input): Json<CreateTenantInput>,
) -> Result<impl IntoResponse, AppError> {
    require_superadmin(&state, &_claims.sub).await?;
    input.validate()?;
    let slug = input.slug_or_derived();
    if slug.len() < 2 {
        return Err(AppError::Validation("slug inválido".into()));
    }
    let tenant = sqlx::query_as::<_, Tenant>(
        "INSERT INTO tenants (name, slug, contact_name, contact_email) \
         VALUES ($1, $2, $3, $4) \
         RETURNING id, name, slug, status, contact_name, contact_email, created_at, updated_at",
    )
    .bind(input.name.trim())
    .bind(&slug)
    .bind(
        input
            .contact_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty()),
    )
    .bind(
        input
            .contact_email
            .as_deref()
            .map(|e| e.trim().to_lowercase())
            .filter(|s| !s.is_empty()),
    )
    .fetch_one(&state.db)
    .await?;
    let _ = crate::handlers::audit::insert_audit_log(
        &state.db,
        Some(_claims.sub),
        "tenant_created",
        "tenant",
        tenant.id,
        None,
        Some(serde_json::json!({ "slug": slug })),
    )
    .await;
    Ok((StatusCode::CREATED, Json(tenant)))
}

/// Cambia el estado de una empresa (superadmin). Transiciones estrictas.
pub async fn set_tenant_status(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateTenantInput>,
) -> Result<impl IntoResponse, AppError> {
    require_superadmin(&state, &_claims.sub).await?;
    input.validate()?;
    let current: Option<String> = sqlx::query_scalar("SELECT status FROM tenants WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    let Some(de) = current else {
        return Err(AppError::NotFound);
    };
    if !puede_transitar(&de, &input.status) {
        return Err(AppError::Validation(format!(
            "transición inválida: {de} -> {}",
            input.status
        )));
    }
    sqlx::query("UPDATE tenants SET status = $1 WHERE id = $2")
        .bind(&input.status)
        .bind(id)
        .execute(&state.db)
        .await?;
    let _ = crate::handlers::audit::insert_audit_log(
        &state.db,
        Some(_claims.sub),
        "tenant_status",
        "tenant",
        id,
        Some(serde_json::json!({ "from": de })),
        Some(serde_json::json!({ "status": input.status })),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// Borra una empresa vacía (superadmin). Con usuarios -> 422 (suspender).
pub async fn delete_tenant(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    require_superadmin(&state, &_claims.sub).await?;
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE tenant_id = $1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    if users > 0 {
        return Err(AppError::Validation(
            "la empresa tiene usuarios: suspéndela en vez de borrarla".into(),
        ));
    }
    let n = sqlx::query("DELETE FROM tenants WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Usuarios de una empresa (superadmin).
pub async fn list_tenant_users(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    require_superadmin(&state, &_claims.sub).await?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tenants WHERE id = $1)")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    if !exists {
        return Err(AppError::NotFound);
    }
    let users = sqlx::query_as::<_, UserPublic>(
        "SELECT id, email, name, created_at FROM users WHERE tenant_id = $1 ORDER BY created_at",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(users))
}

/// Crea el administrador de una empresa (superadmin). La empresa debe estar
/// `activa`: no se dan accesos a empresas pendientes o suspendidas.
pub async fn create_tenant_admin(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    Path(id): Path<Uuid>,
    Json(input): Json<CreateTenantAdminInput>,
) -> Result<impl IntoResponse, AppError> {
    require_superadmin(&state, &_claims.sub).await?;
    input.validate()?;
    let status: Option<String> = sqlx::query_scalar("SELECT status FROM tenants WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    match status.as_deref() {
        None => return Err(AppError::NotFound),
        Some("activa") => {}
        Some(other) => {
            return Err(AppError::Validation(format!(
                "empresa en estado '{other}': habilítala antes de crear su admin"
            )));
        }
    }
    let admin_profile: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM profiles WHERE name = 'Administrador' LIMIT 1")
            .fetch_optional(&state.db)
            .await
            .map_err(|_| AppError::Internal)?;
    let email = input.email.trim().to_lowercase();
    let name = input.name.trim().to_string();
    let mut parts = name.splitn(2, ' ');
    let first = parts.next().unwrap_or("").to_string();
    let last = parts.next().unwrap_or("").to_string();
    let hash = password::hash_password(&input.password)?;
    let user = sqlx::query_as::<_, UserPublic>(
        "INSERT INTO users (email, name, first_name, last_name, profile_id, tenant_id, password_hash) \
         VALUES ($1, $2, NULLIF($3,''), NULLIF($4,''), $5, $6, $7) \
         RETURNING id, email, name, created_at",
    )
    .bind(&email)
    .bind(&name)
    .bind(&first)
    .bind(&last)
    .bind(admin_profile)
    .bind(id)
    .bind(&hash)
    .fetch_one(&state.db)
    .await?;
    let _ = crate::handlers::audit::insert_audit_log(
        &state.db,
        Some(_claims.sub),
        "tenant_admin_created",
        "tenant",
        id,
        None,
        Some(serde_json::json!({ "email": email })),
    )
    .await;
    Ok((StatusCode::CREATED, Json(user)))
}
