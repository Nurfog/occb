//! Gating de módulos DLC por licencia (SaaS).
//!
//! - `enforce_module(state, slug, req, next)` se invoca desde un closure
//!   `route_layer(from_fn(...))` (ver router `flow` en `routes.rs`), tras el
//!   JWT: exige licencia activa que cubra `slug` (o `'*'`) Y que el módulo
//!   esté `enabled` en el catálogo.
//! - Sin cobertura -> 402 `module_locked` (el frontend muestra "DLC
//!   bloqueado", no un 403 genérico). Ver `AppError::ModuleLocked`.
//! - Los módulos base (`crm`, `privacy`) no se gatean: van incluidos en el
//!   despliegue. Solo los DLC (hoy `flow`) usan esta capa.

use axum::{extract::Request, middleware::Next, response::Response};

use crate::{
    auth::{Claims, tenant_scope},
    error::AppError,
    state::AppState,
};

/// ¿Hay licencia activa que cubra `slug` para `user_id`? (fase 2)
///
/// - Superadmin: siempre sí (platform).
/// - Licencia platform (`tenant_id NULL`): cubre a todos.
/// - Licencia de empresa: solo a usuarios de ese tenant.
/// - Catálogo `enabled = FALSE` (kill switch): nadie.
pub async fn module_licensed(
    state: &AppState,
    slug: &str,
    user_id: &uuid::Uuid,
) -> Result<bool, AppError> {
    let catalog_on: bool = sqlx::query_scalar("SELECT enabled FROM modules WHERE slug = $1")
        .bind(slug)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| AppError::Internal)?
        .unwrap_or(false);
    if !catalog_on {
        return Ok(false);
    }
    let is_super: bool = sqlx::query_scalar("SELECT is_superadmin FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| AppError::Internal)?
        .unwrap_or(false);
    if is_super {
        return Ok(true);
    }
    let tenant = tenant_scope(state, user_id).await?;
    let covered: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM licenses \
         WHERE revoked_at IS NULL AND (expires_at IS NULL OR expires_at > now()) \
         AND ($1 = ANY(modules) OR '*' = ANY(modules)) \
         AND (tenant_id IS NULL OR ($2::uuid IS NOT NULL AND tenant_id = $2)))",
    )
    .bind(slug)
    .bind(tenant)
    .fetch_one(&state.db)
    .await
    .map_err(|_| AppError::Internal)?;
    Ok(covered)
}

/// Gate ejecutado desde un closure `route_layer(from_fn(...))` (ver router
/// `flow` en `routes.rs`): exige licencia tras el JWT.
/// Sin cobertura -> 402 `module_locked` para UX de DLC en el frontend.
pub async fn enforce_module(
    state: AppState,
    slug: &'static str,
    req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let sub = req
        .extensions()
        .get::<Claims>()
        .map(|c| c.sub)
        .ok_or(AppError::Unauthorized)?;
    if module_licensed(&state, slug, &sub).await? {
        Ok(next.run(req).await)
    } else {
        Err(AppError::ModuleLocked {
            module: slug.to_string(),
        })
    }
}
