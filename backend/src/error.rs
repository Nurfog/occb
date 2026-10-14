//! Errores del API: códigos estables, sin fugas de información.
//!
//! - 4xx: mensaje útil para el cliente.
//! - 5xx / sqlx: mensaje genérico; el detalle va solo al log (tracing::error).

use axum::{
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)] // taxonomía estable: variantes reservadas para futuros handlers (RBAC, 503 tipado)
pub enum AppError {
    #[error("datos inválidos: {0}")]
    Validation(String),
    #[error("no autenticado")]
    Unauthorized,
    #[error("sin permiso")]
    Forbidden,
    #[error("no encontrado")]
    NotFound,
    #[error("conflicto: el recurso ya existe")]
    Conflict,
    #[error("demasiadas peticiones, reintenta más tarde")]
    RateLimited { retry_after_secs: u64 },
    #[error("servicio no disponible")]
    Unavailable,
    #[error("error interno")]
    Internal,
}

impl AppError {
    fn status(&self) -> StatusCode {
        match self {
            Self::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Conflict => StatusCode::CONFLICT,
            Self::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::Validation(_) => "validation_error",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden => "forbidden",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::RateLimited { .. } => "rate_limited",
            Self::Unavailable => "unavailable",
            Self::Internal => "internal_error",
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        if matches!(self, Self::Internal) {
            // El detalle NUNCA sale al cliente; ya se logueó donde se originó.
            tracing::error!("error interno del servidor");
        }
        let status = self.status();
        let mut resp = (
            status,
            Json(json!({ "error": { "code": self.code(), "message": self.to_string() } })),
        )
            .into_response();
        if let Self::RateLimited { retry_after_secs } = self {
            resp.headers_mut().insert(
                header::RETRY_AFTER,
                HeaderValue::from_str(&retry_after_secs.to_string())
                    .unwrap_or(HeaderValue::from_static("60")),
            );
        }
        resp
    }
}

impl From<validator::ValidationErrors> for AppError {
    fn from(e: validator::ValidationErrors) -> Self {
        let msg = e
            .field_errors()
            .iter()
            .map(|(field, errs)| {
                let reasons: Vec<String> = errs.iter().map(|e| e.code.to_string()).collect();
                format!("{field}: {}", reasons.join(", "))
            })
            .collect::<Vec<_>>()
            .join("; ");
        Self::Validation(msg)
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        // Violación de unicidad -> 409, sin exponer el SQL.
        if let sqlx::Error::Database(db) = &e {
            if db.code().as_deref() == Some("23505") {
                return Self::Conflict;
            }
        }
        tracing::error!("db error: {e}");
        Self::Internal
    }
}
