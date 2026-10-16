//! Webhooks salientes: CRUD + entregas (requiere permisos `webhooks.*`).
//!
//! Adaptación de origen sin nuevas dependencias:
//! - Modelos locales (`event` como TEXT validado contra lista blanca; origen
//!   usaba el enum PG `webhook_event`, inexistente en este esquema).
//! - Guard SSRF solo-std (`is_webhook_url_allowed`, misma política que origen
//!   pero sin `reqwest::Url`): solo `http(s)` público, bloquea loopback,
//!   link-local, RFC1918 y nombres de una sola etiqueta.
//! - Las tablas `webhooks`/`webhook_deliveries` NO existen en
//!   `backend/migrations/` (no se pueden tocar): si faltan (`42P01`) se
//!   responde `503 Unavailable` en vez de 500.
//! - Todas las queries con binds; `validator` en inputs; firma HMAC y reintentos
//!   los hace `services::webhook_worker` cuando se cablee (ver su stub).

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{error::AppError, middleware::auth_ext::UserPermissions, state::AppState};

/// Eventos aceptados (lista blanca; mismos que el enum de origen en snake_case).
const ALLOWED_EVENTS: &[&str] = &[
    "deal_created",
    "deal_updated",
    "deal_deleted",
    "contact_created",
    "contact_updated",
    "contact_deleted",
];

fn is_undefined_table(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(db) if db.code().as_deref() == Some("42P01"))
}

fn map_db(e: sqlx::Error) -> AppError {
    if is_undefined_table(&e) {
        tracing::warn!("tablas de webhooks ausentes (migración pendiente)");
        return AppError::Unavailable;
    }
    tracing::error!("db error: {e}");
    AppError::Internal
}

/// Guard SSRF: solo URLs `http(s)` públicas (puerto origen sin `reqwest`).
pub fn is_webhook_url_allowed(url: &str) -> bool {
    let rest = if let Some(r) = url.strip_prefix("https://") {
        r
    } else if let Some(r) = url.strip_prefix("http://") {
        r
    } else {
        return false;
    };
    let without_path = rest.split(['/', '?', '#']).next().unwrap_or("");
    let hostport = without_path.rsplit('@').next().unwrap_or("");
    let host = hostport
        .split(':')
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches(|c| c == '[' || c == ']')
        .to_lowercase();
    if host.is_empty() {
        return false;
    }
    if host == "169.254.169.254"
        || host == "127.0.0.1"
        || host == "::1"
        || host == "localhost"
        || host == "0.0.0.0"
    {
        return false;
    }
    if host.starts_with("10.") || host.starts_with("192.168.") || host == "10" {
        return false;
    }
    if let Some(second) = host.strip_prefix("172.").and_then(|s| s.split('.').next())
        && let Ok(n) = second.parse::<u8>()
        && (16..=31).contains(&n)
    {
        return false;
    }
    // Nombres de una sola etiqueta (p. ej. `intranet`) se bloquean por
    // conservadurismo: los webhooks legítimos usan FQDN.
    if !host.contains('.') {
        return false;
    }
    true
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Webhook {
    pub id: Uuid,
    pub url: String,
    pub event: String,
    pub secret: Option<String>,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct WebhookDelivery {
    pub id: Uuid,
    pub webhook_id: Uuid,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub status: String,
    pub attempts: i32,
    pub next_attempt_at: DateTime<Utc>,
    pub response_status: Option<i32>,
    pub response_body: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateWebhookInput {
    #[validate(url(message = "URL inválida"), length(max = 2000))]
    pub url: String,
    #[validate(length(min = 1, max = 100))]
    pub event: String,
    #[validate(length(max = 255))]
    pub secret: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateWebhookInput {
    #[validate(url(message = "URL inválida"), length(max = 2000))]
    pub url: Option<String>,
    pub active: Option<bool>,
    #[validate(length(max = 255))]
    pub secret: Option<String>,
}

fn check_event(event: &str) -> Result<(), AppError> {
    if ALLOWED_EVENTS.contains(&event) {
        Ok(())
    } else {
        Err(AppError::Validation(format!(
            "evento inválido: {event} (esperado uno de {})",
            ALLOWED_EVENTS.join(", ")
        )))
    }
}

pub async fn list_webhooks(
    State(state): State<AppState>,
    perms: UserPermissions,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("webhooks.view")
        .map_err(|_| AppError::Forbidden)?;
    let webhooks = sqlx::query_as::<_, Webhook>(
        "SELECT id, url, event::text AS event, secret, active, created_at, updated_at
         FROM webhooks ORDER BY created_at DESC",
    )
    .fetch_all(&state.db)
    .await
    .map_err(map_db)?;
    Ok(Json(webhooks))
}

pub async fn create_webhook(
    State(state): State<AppState>,
    perms: UserPermissions,
    Json(input): Json<CreateWebhookInput>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("webhooks.manage")
        .map_err(|_| AppError::Forbidden)?;
    input.validate()?;
    check_event(input.event.trim())?;
    // Fallo rápido: el dispatcher rechazaría URLs no públicas (guard SSRF).
    if !is_webhook_url_allowed(input.url.trim()) {
        return Err(AppError::Validation(
            "la URL del webhook debe ser http(s) pública".to_string(),
        ));
    }
    let webhook = sqlx::query_as::<_, Webhook>(
        "INSERT INTO webhooks (url, event, secret) VALUES ($1, $2::webhook_event, $3)
         RETURNING id, url, event::text AS event, secret, active, created_at, updated_at",
    )
    .bind(input.url.trim())
    .bind(input.event.trim())
    .bind(input.secret.as_deref())
    .fetch_one(&state.db)
    .await
    .map_err(map_db)?;
    Ok((StatusCode::CREATED, Json(webhook)))
}

pub async fn update_webhook(
    State(state): State<AppState>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateWebhookInput>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("webhooks.manage")
        .map_err(|_| AppError::Forbidden)?;
    input.validate()?;
    if let Some(url) = input.url.as_deref()
        && !is_webhook_url_allowed(url.trim())
    {
        return Err(AppError::Validation(
            "la URL del webhook debe ser http(s) pública".to_string(),
        ));
    }
    let existing = sqlx::query_as::<_, Webhook>(
        "SELECT id, url, event::text AS event, secret, active, created_at, updated_at
         FROM webhooks WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db)?
    .ok_or(AppError::NotFound)?;

    let new_url = input.url.as_deref().unwrap_or(&existing.url);
    let new_active = input.active.unwrap_or(existing.active);
    let webhook = sqlx::query_as::<_, Webhook>(
        "UPDATE webhooks SET url = $1, active = $2, secret = COALESCE($3, secret),
         updated_at = now() WHERE id = $4
         RETURNING id, url, event::text AS event, secret, active, created_at, updated_at",
    )
    .bind(new_url.trim())
    .bind(new_active)
    .bind(input.secret.as_deref())
    .bind(id)
    .fetch_one(&state.db)
    .await
    .map_err(map_db)?;
    Ok(Json(webhook))
}

pub async fn list_deliveries(
    State(state): State<AppState>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("webhooks.view")
        .map_err(|_| AppError::Forbidden)?;
    let exists: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM webhooks WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .map_err(map_db)?;
    if exists.is_none() {
        return Err(AppError::NotFound);
    }
    let deliveries = sqlx::query_as::<_, WebhookDelivery>(
        "SELECT id, webhook_id, event_type, payload, status::text AS status, attempts,
                next_attempt_at, response_status, response_body, created_at, updated_at
         FROM webhook_deliveries WHERE webhook_id = $1
         ORDER BY created_at DESC LIMIT 100",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await
    .map_err(map_db)?;
    Ok(Json(deliveries))
}

pub async fn delete_webhook(
    State(state): State<AppState>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("webhooks.manage")
        .map_err(|_| AppError::Forbidden)?;
    let result = sqlx::query("DELETE FROM webhooks WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(map_db)?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}
