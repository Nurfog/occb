//! Autenticación: solo `Authorization: Bearer <access JWT>`.
//!
//! - El token NUNCA viaja en query params (quedaría en logs/historial).
//! - Los claims verificados se inyectan en la request para los handlers.

use axum::{
    extract::{Request, State},
    http::header,
    middleware::Next,
    response::Response,
};

use crate::{auth::verify_access_token, error::AppError, state::AppState};

pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(AppError::Unauthorized)?;
    let claims = verify_access_token(token, &state.config)?;
    // Empresa suspendida = sus usuarios no entran (401 genérico, sin fugas).
    // Superadmin y usuarios sin tenant (platform) pasan siempre.
    let blocked: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM users u JOIN tenants t ON t.id = u.tenant_id \
         WHERE u.id = $1 AND u.is_superadmin = FALSE AND t.status = 'suspendida')",
    )
    .bind(claims.sub)
    .fetch_one(&state.db)
    .await
    .map_err(|_| AppError::Internal)?;
    if blocked {
        // Genérico a propósito: no revela si la empresa existe o su estado.
        return Err(AppError::Unauthorized);
    }
    req.extensions_mut().insert(claims);
    Ok(next.run(req).await)
}
