//! Grupo C — calendar: CRUD local + OAuth/sync Google/Microsoft.
//!
//! - Solo-autenticado (`Extension<Claims>`, `sub: Uuid` directo, sin parse).
//! - `connection_status` lee `calendar_tokens`/`user_integrations`.
//! - Callbacks y `get_auth_url` reutilizan la lógica OAuth (state anti-CSRF,
//!   intercambio code→tokens por `http_client`, cifrado AES-GCM).
//! - Sin config de provider o sin token -> 503. Sin secretos en logs.

use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use uuid::Uuid;

use crate::{
    auth::Claims,
    error::AppError,
    middleware::auth_ext::UserPermissions,
    models::{
        CalendarConnectionStatus, CalendarEvent, CalendarQuery, CreateCalendarEvent,
        UpdateCalendarEvent,
    },
    services::{
        crypto::{decrypt_from_hex, encrypt_to_hex, require_key},
        oauth as oauth_svc,
    },
    state::AppState,
};

/// GET /api/v1/calendar/status.
pub async fn connection_status(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(claims): Extension<Claims>,
) -> Result<Json<CalendarConnectionStatus>, AppError> {
    perms
        .require("calendar.view")
        .map_err(|_| AppError::Forbidden)?;
    let user_id = claims.sub;
    let google_cal: bool = sqlx::query_scalar(
        "SELECT true FROM calendar_tokens WHERE provider = 'google' AND user_id = $1 LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .unwrap_or(false);
    let google_int: bool = sqlx::query_scalar(
        "SELECT true FROM user_integrations WHERE provider = 'google' AND user_id = $1 LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .unwrap_or(false);
    let ms_cal: bool = sqlx::query_scalar(
        "SELECT true FROM calendar_tokens WHERE provider = 'microsoft' AND user_id = $1 LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .unwrap_or(false);
    let ms_int: bool = sqlx::query_scalar(
        "SELECT true FROM user_integrations WHERE provider = 'microsoft' AND user_id = $1 LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .unwrap_or(false);
    Ok(Json(CalendarConnectionStatus {
        google: google_cal || google_int,
        microsoft: ms_cal || ms_int,
    }))
}

async fn exchange_calendar_code(
    state: &AppState,
    provider: &str,
    code: &str,
    user_id: Uuid,
) -> Result<(), AppError> {
    if !oauth_svc::is_configured(&state.config.oauth, provider) {
        tracing::warn!("calendar callback sin config de provider");
        return Err(AppError::Unavailable);
    }
    let (client_id, client_secret) = oauth_svc::client_credentials(&state.config.oauth, provider)
        .ok_or(AppError::Unavailable)?;
    let eps = oauth_svc::endpoints(provider).ok_or(AppError::NotFound)?;
    let redirect_uri = oauth_svc::calendar_redirect_uri(&state.config.backend_url, provider);
    let code_owned = code.to_string();
    let token_resp = state
        .http_client
        .post(eps.token_url)
        .form(&[
            ("code", code_owned.as_str()),
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await
        .map_err(|e| {
            tracing::error!("calendar: fallo intercambio code→tokens");
            let _ = e;
            AppError::Internal
        })?;
    if !token_resp.status().is_success() {
        tracing::error!(
            "calendar: token endpoint status={}",
            token_resp.status().as_u16()
        );
        return Err(AppError::Internal);
    }
    let tokens: serde_json::Value = token_resp.json().await.map_err(|e| {
        tracing::error!("calendar: token response no-JSON");
        let _ = e;
        AppError::Internal
    })?;
    let access_token = tokens["access_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            tracing::error!("calendar token response sin access_token");
            AppError::Internal
        })?;
    let refresh_token = tokens["refresh_token"].as_str().filter(|s| !s.is_empty());
    let expires_in = tokens["expires_in"]
        .as_i64()
        .unwrap_or(3600)
        .clamp(0, 31_536_000);
    let expires_at = Utc::now() + chrono::Duration::seconds(expires_in);

    let enc_key = require_key(
        state.config.token_encryption_key.as_ref(),
        state.config.env.is_prod(),
    )?;
    let enc_access = encrypt_to_hex(&enc_key, access_token)?;
    let enc_refresh: Option<String> = match refresh_token {
        Some(rt) => Some(encrypt_to_hex(&enc_key, rt)?),
        None => None,
    };

    sqlx::query(
        "INSERT INTO calendar_tokens (user_id, provider, access_token, refresh_token, expires_at)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (user_id, provider) DO UPDATE SET
            access_token = EXCLUDED.access_token,
            refresh_token = COALESCE(EXCLUDED.refresh_token, calendar_tokens.refresh_token),
            expires_at = EXCLUDED.expires_at,
            updated_at = NOW()",
    )
    .bind(user_id)
    .bind(provider)
    .bind(&enc_access)
    .bind(&enc_refresh)
    .bind(expires_at)
    .execute(&state.db)
    .await?;
    Ok(())
}

/// GET /api/v1/calendar/{provider}/callback — provider fijo google.
pub async fn google_callback(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(claims): Extension<Claims>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<StatusCode, AppError> {
    perms
        .require("calendar.create")
        .map_err(|_| AppError::Forbidden)?;
    let code = params
        .get("code")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::Validation("falta `code`".into()))?;
    if code.len() > 4096 {
        return Err(AppError::Validation("`code` inválido".into()));
    }
    // State opcional (si el frontend lo envía, se verifica single-use).
    if let Some(st) = params
        .get("state")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        if st.len() > 256 {
            return Err(AppError::Validation("`state` inválido".into()));
        }
        let pending = oauth_svc::take_state(st)
            .await
            .ok_or(AppError::Unauthorized)?;
        if pending.provider != "google" || pending.user_id != claims.sub {
            return Err(AppError::Unauthorized);
        }
    }
    exchange_calendar_code(&state, "google", code, claims.sub).await?;
    Ok(StatusCode::OK)
}

/// GET /api/v1/calendar/microsoft/callback — provider fijo microsoft.
pub async fn microsoft_callback(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(claims): Extension<Claims>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<StatusCode, AppError> {
    perms
        .require("calendar.create")
        .map_err(|_| AppError::Forbidden)?;
    let code = params
        .get("code")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::Validation("falta `code`".into()))?;
    if code.len() > 4096 {
        return Err(AppError::Validation("`code` inválido".into()));
    }
    if let Some(st) = params
        .get("state")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        if st.len() > 256 {
            return Err(AppError::Validation("`state` inválido".into()));
        }
        let pending = oauth_svc::take_state(st)
            .await
            .ok_or(AppError::Unauthorized)?;
        if pending.provider != "microsoft" || pending.user_id != claims.sub {
            return Err(AppError::Unauthorized);
        }
    }
    exchange_calendar_code(&state, "microsoft", code, claims.sub).await?;
    Ok(StatusCode::OK)
}

/// GET /api/v1/calendar/auth/{provider} — reutiliza oauth connect (calendar).
pub async fn get_auth_url(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(claims): Extension<Claims>,
    Path(provider): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    perms
        .require("calendar.view")
        .map_err(|_| AppError::Forbidden)?;
    let url = oauth_svc::build_calendar_authorize_url(
        &state.config.oauth,
        &state.config.backend_url,
        provider.trim(),
        claims.sub,
    )
    .await?;
    Ok(Json(serde_json::json!({ "url": url, "auth_url": url })))
}

/// GET /api/v1/calendar/events — lista del usuario (SQL puro).
pub async fn list_events(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(claims): Extension<Claims>,
    Query(params): Query<CalendarQuery>,
) -> Result<Json<Vec<CalendarEvent>>, AppError> {
    perms
        .require("calendar.view")
        .map_err(|_| AppError::Forbidden)?;
    let user_id = claims.sub;
    let start = params
        .start
        .unwrap_or_else(|| Utc::now() - chrono::Duration::days(30));
    let end = params
        .end
        .unwrap_or_else(|| Utc::now() + chrono::Duration::days(60));

    let events = if let Some(ref provider) = params.provider {
        sqlx::query_as::<_, CalendarEvent>(
            "SELECT id, user_id, provider, external_id, title, description, location, start_time, end_time, all_day, attendees, entity_type, entity_id, created_at, updated_at
             FROM calendar_events WHERE user_id = $1 AND start_time >= $2 AND end_time <= $3 AND provider = $4 ORDER BY start_time",
        )
        .bind(user_id)
        .bind(start)
        .bind(end)
        .bind(provider)
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as::<_, CalendarEvent>(
            "SELECT id, user_id, provider, external_id, title, description, location, start_time, end_time, all_day, attendees, entity_type, entity_id, created_at, updated_at
             FROM calendar_events WHERE user_id = $1 AND start_time >= $2 AND end_time <= $3 ORDER BY start_time",
        )
        .bind(user_id)
        .bind(start)
        .bind(end)
        .fetch_all(&state.db)
        .await?
    };

    Ok(Json(events))
}

/// POST /api/v1/calendar/events.
pub async fn create_event(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(claims): Extension<Claims>,
    Json(input): Json<CreateCalendarEvent>,
) -> Result<(StatusCode, Json<CalendarEvent>), AppError> {
    perms
        .require("calendar.create")
        .map_err(|_| AppError::Forbidden)?;
    if input.title.trim().is_empty() {
        return Err(AppError::Validation("título requerido".into()));
    }
    if input.end_time < input.start_time {
        return Err(AppError::Validation(
            "`end_time` anterior a `start_time`".into(),
        ));
    }
    let event = sqlx::query_as::<_, CalendarEvent>(
        "INSERT INTO calendar_events (user_id, provider, title, description, location, start_time, end_time, all_day, attendees, entity_type, entity_id)
         VALUES ($1, 'local', $2, $3, $4, $5, $6, $7, $8, $9, $10)
         RETURNING id, user_id, provider, external_id, title, description, location, start_time, end_time, all_day, attendees, entity_type, entity_id, created_at, updated_at",
    )
    .bind(claims.sub)
    .bind(input.title.trim())
    .bind(&input.description)
    .bind(&input.location)
    .bind(input.start_time)
    .bind(input.end_time)
    .bind(input.all_day.unwrap_or(false))
    .bind(input.attendees.map(|a| serde_json::json!(a)))
    .bind(&input.entity_type)
    .bind(input.entity_id)
    .fetch_one(&state.db)
    .await?;

    Ok((StatusCode::CREATED, Json(event)))
}

/// PUT /api/v1/calendar/events/{id} (solo propios).
pub async fn update_event(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(claims): Extension<Claims>,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateCalendarEvent>,
) -> Result<Json<CalendarEvent>, AppError> {
    perms
        .require("calendar.edit")
        .map_err(|_| AppError::Forbidden)?;
    let event = sqlx::query_as::<_, CalendarEvent>(
        "UPDATE calendar_events SET
            title = COALESCE($3, title),
            description = COALESCE($4, description),
            location = COALESCE($5, location),
            start_time = COALESCE($6, start_time),
            end_time = COALESCE($7, end_time),
            all_day = COALESCE($8, all_day),
            attendees = COALESCE($9, attendees),
            updated_at = NOW()
         WHERE id = $1 AND user_id = $2
         RETURNING id, user_id, provider, external_id, title, description, location, start_time, end_time, all_day, attendees, entity_type, entity_id, created_at, updated_at",
    )
    .bind(id)
    .bind(claims.sub)
    .bind(&input.title)
    .bind(&input.description)
    .bind(&input.location)
    .bind(input.start_time)
    .bind(input.end_time)
    .bind(input.all_day)
    .bind(input.attendees.map(|a| serde_json::json!(a)))
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    Ok(Json(event))
}

/// DELETE /api/v1/calendar/events/{id} (solo propios).
pub async fn delete_event(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(claims): Extension<Claims>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    perms
        .require("calendar.delete")
        .map_err(|_| AppError::Forbidden)?;
    let result = sqlx::query("DELETE FROM calendar_events WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(claims.sub)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    Ok(StatusCode::NO_CONTENT)
}

fn parse_google_time(s: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = s.parse::<DateTime<Utc>>() {
        return Some(dt);
    }
    // Fecha sola (all-day): "2026-10-16" -> medianoche UTC.
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d") {
        return Some(d.and_hms_opt(0, 0, 0)?.and_utc());
    }
    None
}

/// POST /api/v1/calendar/sync/google — sincroniza desde Google Calendar.
pub async fn sync_google(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(claims): Extension<Claims>,
) -> Result<Json<serde_json::Value>, AppError> {
    perms
        .require("calendar.create")
        .map_err(|_| AppError::Forbidden)?;
    // Token en calendar_tokens con fallback a user_integrations.
    let cal_token: Option<(String,)> = sqlx::query_as(
        "SELECT access_token FROM calendar_tokens WHERE provider = 'google' AND user_id = $1 LIMIT 1",
    )
    .bind(claims.sub)
    .fetch_optional(&state.db)
    .await?;
    let enc_access = match cal_token {
        Some((t,)) => t,
        None => {
            let integ: Option<(String,)> = sqlx::query_as(
                "SELECT access_token FROM user_integrations WHERE provider = 'google' AND user_id = $1 LIMIT 1",
            )
            .bind(claims.sub)
            .fetch_optional(&state.db)
            .await?;
            match integ {
                Some((t,)) => t,
                None => return Err(AppError::Unavailable),
            }
        }
    };
    if enc_access.trim().is_empty() {
        return Err(AppError::Unavailable);
    }
    let enc_key = require_key(
        state.config.token_encryption_key.as_ref(),
        state.config.env.is_prod(),
    )?;
    let access_token = decrypt_from_hex(&enc_key, &enc_access).map_err(|_| {
        tracing::error!("calendar: token indescifrable");
        AppError::Internal
    })?;

    let now = Utc::now();
    let min_time = (now - chrono::Duration::days(90)).to_rfc3339();
    let max_time = (now + chrono::Duration::days(90)).to_rfc3339();

    let response = state
        .http_client
        .get("https://www.googleapis.com/calendar/v3/calendars/primary/events")
        .bearer_auth(&access_token)
        .query(&[
            ("timeMin", min_time.as_str()),
            ("timeMax", max_time.as_str()),
            ("singleEvents", "true"),
            ("orderBy", "startTime"),
            ("maxResults", "250"),
        ])
        .send()
        .await
        .map_err(|e| {
            tracing::error!("calendar: fallo Google Calendar API");
            let _ = e;
            AppError::Internal
        })?;
    if !response.status().is_success() {
        tracing::error!("calendar: google status={}", response.status().as_u16());
        return Err(AppError::Internal);
    }
    let data: serde_json::Value = response.json().await.map_err(|e| {
        tracing::error!("calendar: google response no-JSON");
        let _ = e;
        AppError::Internal
    })?;
    let items = data["items"].as_array().ok_or_else(|| {
        tracing::error!("calendar sync: sin array items");
        AppError::Internal
    })?;
    let mut synced = 0;
    for item in items {
        let external_id = item["id"].as_str().unwrap_or("").trim();
        if external_id.is_empty() || external_id.len() > 255 {
            continue;
        }
        let title = item["summary"].as_str().unwrap_or("Untitled").trim();
        if title.is_empty() {
            continue;
        }
        let title: String = title.chars().take(500).collect();
        let description = item["description"].as_str().map(|s| s.to_string());
        let location = item["location"].as_str().map(|s| s.to_string());
        let start_raw = item["start"]["dateTime"]
            .as_str()
            .or_else(|| item["start"]["date"].as_str());
        let end_raw = item["end"]["dateTime"]
            .as_str()
            .or_else(|| item["end"]["date"].as_str());
        let (Some(start_time), Some(end_time)) = (
            start_raw.and_then(parse_google_time),
            end_raw.and_then(parse_google_time),
        ) else {
            continue;
        };
        if end_time < start_time {
            continue;
        }
        let all_day = item["start"]["date"].as_str().is_some();
        let res = sqlx::query(
            "INSERT INTO calendar_events (user_id, provider, external_id, title, description, location, start_time, end_time, all_day)
             VALUES ($1, 'google', $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT DO NOTHING",
        )
        .bind(claims.sub)
        .bind(external_id)
        .bind(&title)
        .bind(&description)
        .bind(&location)
        .bind(start_time)
        .bind(end_time)
        .bind(all_day)
        .execute(&state.db)
        .await;
        if res.is_ok() {
            synced += 1;
        }
    }

    Ok(Json(serde_json::json!({ "synced": synced })))
}
