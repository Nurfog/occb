//! Grupo C — oauth/integraciones (google/microsoft/github + estado whatsapp).
//!
//! - `connect`: URL authorize con state anti-CSRF single-use TTL 10 min en
//!   memoria (cap 1000). Sin config de provider -> 503.
//! - `callback` (pública): verifica state, intercambia code→tokens por reqwest
//!   (30 s vía `state.http_client`), trae userinfo, cifra con AES-GCM y hace
//!   upsert en `user_integrations`, redirige al frontend.
//! - `list_integrations`: desde `user_integrations` + catálogo
//!   google/microsoft/github/whatsapp (whatsapp desde `whatsapp_config`).
//! - `disconnect`: borra la fila (whatsapp desactiva la config).

use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect},
};
use serde::{Deserialize, Serialize};

use crate::{
    auth::Claims,
    error::AppError,
    services::{
        crypto::{encrypt_to_hex, require_key},
        oauth as oauth_svc,
    },
    state::AppState,
};

#[derive(Debug, Serialize)]
pub struct IntegrationStatus {
    pub provider: String,
    pub connected: bool,
    pub connected_at: Option<String>,
    pub provider_email: Option<String>,
    pub provider_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

fn known_provider(provider: &str) -> bool {
    oauth_svc::is_known_oauth_provider(provider)
}

/// GET /api/v1/integrations/{provider}/connect.
pub async fn connect(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(provider): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !known_provider(&provider) {
        return Err(AppError::NotFound);
    }
    if provider.len() > 50 {
        return Err(AppError::Validation("proveedor inválido".into()));
    }
    let url = oauth_svc::build_authorize_url(
        &state.config.oauth,
        &state.config.backend_url,
        &provider,
        claims.sub,
    )
    .await?;
    Ok(Json(serde_json::json!({ "auth_url": url })))
}

/// GET /api/v1/integrations/{provider}/callback — pública.
pub async fn callback(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Query(query): Query<CallbackQuery>,
) -> Result<impl IntoResponse, AppError> {
    if !known_provider(&provider) {
        return Err(AppError::NotFound);
    }

    if let Some(err) = &query.error {
        let reason: String = err
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .take(50)
            .collect();
        let redirect = format!(
            "{}/settings?integration={}&status=error&reason={}",
            state.config.frontend_url, provider, reason
        );
        return Ok(Redirect::to(&redirect).into_response());
    }

    let code = query
        .code
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::Validation("falta `code`".into()))?;
    if code.len() > 4096 {
        return Err(AppError::Validation("`code` inválido".into()));
    }
    let state_param = query
        .state
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::Validation("falta `state`".into()))?;
    if state_param.len() > 256 {
        return Err(AppError::Validation("`state` inválido".into()));
    }

    // Verifica state single-use con TTL.
    let pending = oauth_svc::take_state(state_param)
        .await
        .ok_or(AppError::Unauthorized)?;
    if pending.provider != provider {
        return Err(AppError::Unauthorized);
    }

    // Sin config de provider -> 503 (no 500).
    if !oauth_svc::is_configured(&state.config.oauth, &provider) {
        tracing::warn!("oauth callback sin config de provider");
        return Err(AppError::Unavailable);
    }
    let (client_id, client_secret) = oauth_svc::client_credentials(&state.config.oauth, &provider)
        .ok_or(AppError::Unavailable)?;
    let eps = oauth_svc::endpoints(&provider).ok_or(AppError::NotFound)?;
    let redirect_uri = oauth_svc::oauth_redirect_uri(&state.config.backend_url, &provider);

    // Intercambia code→tokens (reqwest 30 s vía http_client compartido).
    let auth_code = "authorization_code".to_string();
    let code_owned = code.to_string();
    let mut form_params: Vec<(&str, &String)> = vec![
        ("client_id", &client_id),
        ("client_secret", &client_secret),
        ("code", &code_owned),
        ("redirect_uri", &redirect_uri),
    ];
    if provider != "github" {
        form_params.push(("grant_type", &auth_code));
    }
    let token_resp = state
        .http_client
        .post(eps.token_url)
        .form(&form_params)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| {
            tracing::error!("oauth: fallo intercambio code→tokens");
            let _ = e;
            AppError::Internal
        })?;
    if !token_resp.status().is_success() {
        tracing::error!(
            "oauth: token endpoint status={}",
            token_resp.status().as_u16()
        );
        return Err(AppError::Internal);
    }
    let token_body: serde_json::Value = token_resp.json().await.map_err(|e| {
        tracing::error!("oauth: token response no-JSON");
        let _ = e;
        AppError::Internal
    })?;
    let access_token = token_body["access_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            tracing::error!("oauth token response sin access_token");
            AppError::Internal
        })?
        .to_string();
    let refresh_token = token_body["refresh_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let expires_in = token_body["expires_in"].as_i64();

    // Trae userinfo del proveedor.
    let userinfo_resp = state
        .http_client
        .get(eps.userinfo_url)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| {
            tracing::error!("oauth: fallo userinfo");
            let _ = e;
            AppError::Internal
        })?;
    if !userinfo_resp.status().is_success() {
        tracing::error!("oauth: userinfo status={}", userinfo_resp.status().as_u16());
        return Err(AppError::Internal);
    }
    let userinfo: serde_json::Value = userinfo_resp.json().await.map_err(|e| {
        tracing::error!("oauth: userinfo no-JSON");
        let _ = e;
        AppError::Internal
    })?;
    let provider_email = userinfo
        .get("email")
        .and_then(|v| v.as_str())
        .or_else(|| userinfo.get("mail").and_then(|v| v.as_str()))
        .or_else(|| userinfo.get("userPrincipalName").and_then(|v| v.as_str()))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.len() <= 320);
    let provider_name = userinfo
        .get("name")
        .and_then(|v| v.as_str())
        .or_else(|| userinfo.get("displayName").and_then(|v| v.as_str()))
        .or_else(|| userinfo.get("login").and_then(|v| v.as_str()))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.len() <= 255);

    let expires_at = expires_in
        .map(|secs| chrono::Utc::now() + chrono::Duration::seconds(secs.clamp(0, 31_536_000)));

    // Cifra tokens con AES-GCM (fail-closed: sin clave -> 503).
    let enc_key = require_key(
        state.config.token_encryption_key.as_ref(),
        state.config.env.is_prod(),
    )?;
    let enc_access = encrypt_to_hex(&enc_key, &access_token)?;
    let enc_refresh: Option<String> = match refresh_token {
        Some(rt) => Some(encrypt_to_hex(&enc_key, &rt)?),
        None => None,
    };

    // Upsert en user_integrations.
    sqlx::query(
        r#"
        INSERT INTO user_integrations (user_id, provider, access_token, refresh_token, token_expires_at, provider_email, provider_name)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ON CONFLICT (user_id, provider)
        DO UPDATE SET
            access_token = EXCLUDED.access_token,
            refresh_token = COALESCE(EXCLUDED.refresh_token, user_integrations.refresh_token),
            token_expires_at = EXCLUDED.token_expires_at,
            provider_email = EXCLUDED.provider_email,
            provider_name = EXCLUDED.provider_name,
            updated_at = NOW()
        "#,
    )
    .bind(pending.user_id)
    .bind(&provider)
    .bind(&enc_access)
    .bind(&enc_refresh)
    .bind(expires_at)
    .bind(&provider_email)
    .bind(&provider_name)
    .execute(&state.db)
    .await?;

    let redirect = format!(
        "{}/settings?integration={}&status=connected",
        state.config.frontend_url, provider
    );
    Ok(Redirect::to(&redirect).into_response())
}

/// GET /api/v1/integrations — lista google/microsoft/github/whatsapp con estado.
pub async fn list_integrations(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Vec<IntegrationStatus>>, AppError> {
    let rows = sqlx::query_as::<_, IntegrationRow>(
        "SELECT provider, created_at, provider_email, provider_name FROM user_integrations WHERE user_id = $1 ORDER BY provider",
    )
    .bind(claims.sub)
    .fetch_all(&state.db)
    .await?;

    let mut result: Vec<IntegrationStatus> = ["google", "microsoft", "github", "whatsapp"]
        .iter()
        .map(|p| IntegrationStatus {
            provider: p.to_string(),
            connected: false,
            connected_at: None,
            provider_email: None,
            provider_name: None,
        })
        .collect();

    for row in rows {
        if let Some(item) = result.iter_mut().find(|r| r.provider == row.provider) {
            item.connected = true;
            item.connected_at = Some(row.created_at.format("%Y-%m-%dT%H:%M:%SZ").to_string());
            item.provider_email = row.provider_email;
            item.provider_name = row.provider_name;
        }
    }

    // whatsapp: singleton activo en whatsapp_config.
    let wa: Option<(chrono::DateTime<chrono::Utc>,)> = sqlx::query_as(
        "SELECT created_at FROM whatsapp_config WHERE is_active = true ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_optional(&state.db)
    .await?;
    if let Some((created_at,)) = wa
        && let Some(item) = result.iter_mut().find(|r| r.provider == "whatsapp")
    {
        item.connected = true;
        item.connected_at = Some(created_at.format("%Y-%m-%dT%H:%M:%SZ").to_string());
    }

    Ok(Json(result))
}

/// DELETE /api/v1/integrations/{provider}/disconnect.
pub async fn disconnect(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(provider): Path<String>,
) -> Result<StatusCode, AppError> {
    if provider == "whatsapp" {
        let result = sqlx::query(
            "UPDATE whatsapp_config SET is_active = false, updated_at = NOW() WHERE is_active = true",
        )
        .execute(&state.db)
        .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound);
        }
        return Ok(StatusCode::NO_CONTENT);
    }
    if !known_provider(&provider) {
        return Err(AppError::NotFound);
    }

    let result = sqlx::query("DELETE FROM user_integrations WHERE user_id = $1 AND provider = $2")
        .bind(claims.sub)
        .bind(&provider)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, sqlx::FromRow)]
struct IntegrationRow {
    provider: String,
    created_at: chrono::DateTime<chrono::Utc>,
    provider_email: Option<String>,
    provider_name: Option<String>,
}
