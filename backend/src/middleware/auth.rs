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
    req.extensions_mut().insert(claims);
    Ok(next.run(req).await)
}
