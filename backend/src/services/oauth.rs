//! Helpers OAuth FASE 2: state anti-CSRF en memoria + endpoints por provider.
//!
//! - Store en memoria con TTL 10 min, single-use y cap 1000 (purga oportunista).
//! - Endpoints hardcodeados (la config solo aporta client_id/secret).
//! - Sin secretos en logs.

use std::collections::HashMap;
use std::sync::OnceLock;

use chrono::{DateTime, Utc};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{config::OauthConfig, error::AppError};

#[derive(Debug, Clone)]
pub struct PendingState {
    pub provider: String,
    pub user_id: Uuid,
    pub created_at: DateTime<Utc>,
}

pub const OAUTH_STATE_TTL_MINUTES: i64 = 10;
pub const OAUTH_STATE_MAX_ENTRIES: usize = 1000;

fn store() -> &'static Mutex<HashMap<String, PendingState>> {
    static STORE: OnceLock<Mutex<HashMap<String, PendingState>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn generate_state() -> String {
    use rand::{Rng, distributions::Alphanumeric};
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect()
}

pub fn is_known_oauth_provider(provider: &str) -> bool {
    matches!(provider, "google" | "microsoft" | "github")
}

pub fn is_configured(oauth: &OauthConfig, provider: &str) -> bool {
    match provider {
        "google" => oauth.google.configured(),
        "microsoft" => oauth.microsoft.configured(),
        "github" => oauth.github.configured(),
        _ => false,
    }
}

pub fn client_credentials(oauth: &OauthConfig, provider: &str) -> Option<(String, String)> {
    match provider {
        "google" => oauth
            .google
            .client_id
            .clone()
            .zip(oauth.google.client_secret.clone()),
        "microsoft" => oauth
            .microsoft
            .client_id
            .clone()
            .zip(oauth.microsoft.client_secret.clone()),
        "github" => oauth
            .github
            .client_id
            .clone()
            .zip(oauth.github.client_secret.clone()),
        _ => None,
    }
}

pub struct ProviderEndpoints {
    pub auth_url: &'static str,
    pub token_url: &'static str,
    pub userinfo_url: &'static str,
    pub scope: &'static str,
}

pub fn endpoints(provider: &str) -> Option<ProviderEndpoints> {
    match provider {
        "google" => Some(ProviderEndpoints {
            auth_url: "https://accounts.google.com/o/oauth2/v2/auth",
            token_url: "https://oauth2.googleapis.com/token",
            userinfo_url: "https://www.googleapis.com/oauth2/v2/userinfo",
            scope: "openid email profile https://www.googleapis.com/auth/calendar",
        }),
        "microsoft" => Some(ProviderEndpoints {
            auth_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
            token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
            userinfo_url: "https://graph.microsoft.com/v1.0/me",
            scope: "openid email profile https://graph.microsoft.com/Calendars.ReadWrite offline_access",
        }),
        "github" => Some(ProviderEndpoints {
            auth_url: "https://github.com/login/oauth/authorize",
            token_url: "https://github.com/login/oauth/access_token",
            userinfo_url: "https://api.github.com/user",
            scope: "read:user user:email",
        }),
        _ => None,
    }
}

/// Callback del flujo genérico de integraciones (user_integrations).
pub fn oauth_redirect_uri(backend_url: &str, provider: &str) -> String {
    format!(
        "{}/api/v1/integrations/{}/callback",
        backend_url.trim_end_matches('/'),
        provider
    )
}

/// Callback del flujo de calendario (calendar_tokens).
pub fn calendar_redirect_uri(backend_url: &str, provider: &str) -> String {
    format!(
        "{}/api/v1/calendar/{}/callback",
        backend_url.trim_end_matches('/'),
        provider
    )
}

pub fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub async fn insert_state(state_val: String, entry: PendingState) {
    let mut guard = store().lock().await;
    let now = Utc::now();
    let ttl = chrono::Duration::minutes(OAUTH_STATE_TTL_MINUTES);
    guard.retain(|_, v| now.signed_duration_since(v.created_at) < ttl);
    if guard.len() >= OAUTH_STATE_MAX_ENTRIES {
        let mut oldest: Vec<(String, DateTime<Utc>)> = guard
            .iter()
            .map(|(k, v)| (k.clone(), v.created_at))
            .collect();
        oldest.sort_by_key(|(_, t)| *t);
        let to_evict = guard.len().saturating_sub(OAUTH_STATE_MAX_ENTRIES - 1);
        for (k, _) in oldest.into_iter().take(to_evict) {
            guard.remove(&k);
        }
    }
    guard.insert(state_val, entry);
}

/// Consume el state (single-use). Devuelve `None` si no existe o expiró.
pub async fn take_state(state_param: &str) -> Option<PendingState> {
    let mut guard = store().lock().await;
    let pending = guard.remove(state_param)?;
    let ttl = chrono::Duration::minutes(OAUTH_STATE_TTL_MINUTES);
    if Utc::now().signed_duration_since(pending.created_at) >= ttl {
        return None;
    }
    Some(pending)
}

/// Construye la URL de autorización del flujo genérico (user_integrations).
pub async fn build_authorize_url(
    oauth: &OauthConfig,
    backend_url: &str,
    provider: &str,
    user_id: Uuid,
) -> Result<String, AppError> {
    if !is_known_oauth_provider(provider) {
        return Err(AppError::NotFound);
    }
    if !is_configured(oauth, provider) {
        tracing::warn!("oauth no configurado para provider");
        return Err(AppError::Unavailable);
    }
    let (client_id, _) = client_credentials(oauth, provider).ok_or(AppError::Unavailable)?;
    let eps = endpoints(provider).ok_or(AppError::NotFound)?;
    let redirect_uri = oauth_redirect_uri(backend_url, provider);
    let state_val = generate_state();
    insert_state(
        state_val.clone(),
        PendingState {
            provider: provider.to_string(),
            user_id,
            created_at: Utc::now(),
        },
    )
    .await;
    let consent = if provider == "google" {
        "&access_type=offline&prompt=consent"
    } else {
        ""
    };
    Ok(format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}{}",
        eps.auth_url,
        urlencode(&client_id),
        urlencode(&redirect_uri),
        urlencode(eps.scope),
        state_val,
        consent
    ))
}

/// Construye la URL de autorización del flujo de calendario (calendar_tokens).
pub async fn build_calendar_authorize_url(
    oauth: &OauthConfig,
    backend_url: &str,
    provider: &str,
    user_id: Uuid,
) -> Result<String, AppError> {
    if !matches!(provider, "google" | "microsoft") {
        return Err(AppError::Validation("proveedor desconocido".into()));
    }
    if !is_configured(oauth, provider) {
        tracing::warn!("oauth calendario no configurado para provider");
        return Err(AppError::Unavailable);
    }
    let (client_id, _) = client_credentials(oauth, provider).ok_or(AppError::Unavailable)?;
    let eps = endpoints(provider).ok_or(AppError::NotFound)?;
    let redirect_uri = calendar_redirect_uri(backend_url, provider);
    let state_val = generate_state();
    insert_state(
        state_val.clone(),
        PendingState {
            provider: provider.to_string(),
            user_id,
            created_at: Utc::now(),
        },
    )
    .await;
    let (scope, extra) = match provider {
        "google" => (
            "https://www.googleapis.com/auth/calendar",
            "&access_type=offline&prompt=consent",
        ),
        _ => (
            "https://graph.microsoft.com/Calendars.ReadWrite",
            "&response_mode=query",
        ),
    };
    let _ = eps;
    Ok(format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}{}",
        endpoints(provider).map(|e| e.auth_url).unwrap_or(""),
        urlencode(&client_id),
        urlencode(&redirect_uri),
        urlencode(scope),
        state_val,
        extra
    ))
}
