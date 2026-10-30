//! Auth: register / login / refresh (rotación) / logout / me.
//!
//! Seguridad aplicada:
//! - TODAS las queries parametrizadas (`$1...` + `.bind`), jamás `format!`
//!   con input. `23505` (email duplicado) -> 409.
//! - Anti-enumeración: login con email inexistente hace trabajo similar
//!   (hash dummy) y devuelve el mismo 401 genérico.
//! - Refresh opaco de 256 bits; en DB solo SHA-256; rotación atómica en
//!   transacción; reuso detectado y logueado (posible robo).
//! - Los tokens viajan en JSON (el frontend, proyecto aparte, los guarda en
//!   memoria; nunca en `localStorage` si hay XSS). Si el frontend pasa a
//!   cookies HttpOnly, añadir CSRF double-submit.

use axum::{Extension, Json, extract::State, http::StatusCode, response::IntoResponse};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use validator::Validate;

use crate::{
    auth::{Claims, create_access_token, hash_refresh_token, new_refresh_token},
    error::AppError,
    models::{LoginInput, RefreshInput, RegisterInput, UserPublic},
    password,
    state::AppState,
};

#[derive(Debug, Serialize)]
struct TokenPair {
    user: UserPublic,
    access_token: String,
    refresh_token: String,
    token_type: String,
    expires_in: i64,
}

#[derive(Debug, FromRow)]
pub(crate) struct UserRow {
    pub(crate) id: Uuid,
    pub(crate) email: String,
    pub(crate) name: String,
    password_hash: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct RefreshRow {
    id: Uuid,
    user_id: Uuid,
}

async fn issue_pair(state: &AppState, user: &UserRow) -> Result<(String, String), AppError> {
    let access = create_access_token(user.id, &state.config)?;
    let refresh = new_refresh_token();
    let expires_at = Utc::now() + Duration::days(state.config.refresh_token_ttl_days);
    sqlx::query("INSERT INTO refresh_tokens (user_id, token_hash, expires_at) VALUES ($1, $2, $3)")
        .bind(user.id)
        .bind(hash_refresh_token(&refresh))
        .bind(expires_at)
        .execute(&state.db)
        .await?;
    Ok((access, refresh))
}

fn pair_response(user: &UserRow, access: String, refresh: String, ttl_minutes: i64) -> TokenPair {
    TokenPair {
        user: UserPublic {
            id: user.id,
            email: user.email.clone(),
            name: user.name.clone(),
            created_at: user.created_at,
        },
        access_token: access,
        refresh_token: refresh,
        token_type: "Bearer".to_string(),
        expires_in: ttl_minutes * 60,
    }
}

pub async fn register(
    State(state): State<AppState>,
    Json(input): Json<RegisterInput>,
) -> Result<impl IntoResponse, AppError> {
    input.validate()?;
    // Solo-setup (contrato origen): el primer usuario es admin; después se
    // bloquea y los nuevos usuarios los crea un admin (POST /api/v1/users).
    // La creación vive en `create_first_admin` (compartida con el instalador).
    let (user, access, refresh) =
        create_first_admin(&state, &input.email, &input.name, &input.password).await?;
    Ok((
        StatusCode::CREATED,
        Json(pair_response(
            &user,
            access,
            refresh,
            state.config.access_token_ttl_minutes,
        )),
    ))
}

/// Crea el PRIMER usuario (root/admin). Reutilizado por `register` y por el
/// instalador (`handlers/setup.rs`): advisory lock anti-TOCTOU, perfil
/// `Administrador`, par de tokens. Si ya hay usuarios -> 403.
pub(crate) async fn create_first_admin(
    state: &AppState,
    email: &str,
    name: &str,
    password: &str,
) -> Result<(UserRow, String, String), AppError> {
    let mut tx = state.db.begin().await.map_err(|_| AppError::Internal)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('register'))")
        .execute(&mut *tx)
        .await
        .map_err(|_| AppError::Internal)?;
    let existing: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM users LIMIT 1")
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| AppError::Internal)?;
    if existing.is_some() {
        return Err(AppError::Forbidden);
    }
    let admin_profile: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM profiles WHERE name = 'Administrador' LIMIT 1")
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| AppError::Internal)?;

    let email = email.trim().to_lowercase();
    let name = name.trim().to_string();
    // Parte `name` en first/last para el esquema CRM (004).
    let mut parts = name.splitn(2, ' ');
    let first = parts.next().unwrap_or("").to_string();
    let last = parts.next().unwrap_or("").to_string();
    let hash = password::hash_password(password)?;
    let user = sqlx::query_as::<_, UserRow>(
        "INSERT INTO users (email, name, first_name, last_name, profile_id, password_hash, is_superadmin)
         VALUES ($1, $2, NULLIF($3,''), NULLIF($4,''), $5, $6, TRUE)
         RETURNING id, email, name, password_hash, created_at",
    )
    .bind(&email)
    .bind(&name)
    .bind(&first)
    .bind(&last)
    .bind(admin_profile)
    .bind(&hash)
    .fetch_one(&mut *tx)
    .await?;
    let admin_id = user.id;
    let access = crate::auth::create_access_token(user.id, &state.config)?;
    let refresh = crate::auth::new_refresh_token();
    let expires_at =
        chrono::Utc::now() + chrono::Duration::days(state.config.refresh_token_ttl_days);
    sqlx::query("INSERT INTO refresh_tokens (user_id, token_hash, expires_at) VALUES ($1, $2, $3)")
        .bind(admin_id)
        .bind(crate::auth::hash_refresh_token(&refresh))
        .bind(expires_at)
        .execute(&mut *tx)
        .await
        .map_err(|_| AppError::Internal)?;
    tx.commit().await.map_err(|_| AppError::Internal)?;
    Ok((user, access, refresh))
}

pub async fn login(
    State(state): State<AppState>,
    Json(input): Json<LoginInput>,
) -> Result<impl IntoResponse, AppError> {
    input.validate()?;
    let email = input.email.trim().to_lowercase();
    let user: Option<UserRow> = sqlx::query_as::<_, UserRow>(
        "SELECT id, email, name, password_hash, created_at FROM users WHERE email = $1",
    )
    .bind(&email)
    .fetch_optional(&state.db)
    .await?;
    let Some(user) = user else {
        // Mismo coste aproximado que un verify real + mismo 401: anti-enumeración.
        let _ = password::hash_password("00-mitiga-enumeracion-con-relleno-largo");
        return Err(AppError::Unauthorized);
    };
    if !password::verify_password(&user.password_hash, &input.password) {
        return Err(AppError::Unauthorized);
    }
    // Migración oportunista bcrypt → Argon2id (fire-and-forget, no bloquea login).
    if password::needs_rehash(&user.password_hash)
        && let Ok(new_hash) = password::hash_password(&input.password)
    {
        let _ = sqlx::query("UPDATE users SET password_hash = $1 WHERE id = $2")
            .bind(&new_hash)
            .bind(user.id)
            .execute(&state.db)
            .await;
    }
    let (access, refresh) = issue_pair(&state, &user).await?;
    Ok(Json(pair_response(
        &user,
        access,
        refresh,
        state.config.access_token_ttl_minutes,
    )))
}

#[derive(Debug, Deserialize)]
pub struct LogoutInput {
    refresh_token: String,
}

pub async fn refresh(
    State(state): State<AppState>,
    Json(input): Json<RefreshInput>,
) -> Result<impl IntoResponse, AppError> {
    input.validate()?;
    let hash = hash_refresh_token(&input.refresh_token);

    let mut tx = state.db.begin().await.map_err(|_| AppError::Internal)?;
    let row: Option<RefreshRow> = sqlx::query_as::<_, RefreshRow>(
        "SELECT id, user_id FROM refresh_tokens
         WHERE token_hash = $1 AND revoked_at IS NULL AND expires_at > now()",
    )
    .bind(&hash)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| AppError::Internal)?;

    let Some(row) = row else {
        let reused: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM refresh_tokens WHERE token_hash = $1 AND revoked_at IS NOT NULL)",
        )
        .bind(&hash)
        .fetch_one(&state.db)
        .await
        .unwrap_or(false);
        if reused {
            // Token ya rotado reutilizado: posible robo. Sin revoke global en
            // esta plantilla; el caso queda registrado para alerta.
            tracing::warn!("refresh token reutilizado tras rotación (posible robo)");
        }
        return Err(AppError::Unauthorized);
    };

    let user: UserRow = sqlx::query_as::<_, UserRow>(
        "SELECT id, email, name, password_hash, created_at FROM users WHERE id = $1",
    )
    .bind(row.user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| AppError::Internal)?;

    // Rotación: revoca el usado y crea el nuevo en la misma transacción.
    sqlx::query("UPDATE refresh_tokens SET revoked_at = now() WHERE id = $1")
        .bind(row.id)
        .execute(&mut *tx)
        .await
        .map_err(|_| AppError::Internal)?;
    let access = create_access_token(user.id, &state.config)?;
    let refresh = new_refresh_token();
    let expires_at = Utc::now() + Duration::days(state.config.refresh_token_ttl_days);
    sqlx::query("INSERT INTO refresh_tokens (user_id, token_hash, expires_at) VALUES ($1, $2, $3)")
        .bind(user.id)
        .bind(hash_refresh_token(&refresh))
        .bind(expires_at)
        .execute(&mut *tx)
        .await
        .map_err(|_| AppError::Internal)?;
    tx.commit().await.map_err(|_| AppError::Internal)?;

    Ok(Json(pair_response(
        &user,
        access,
        refresh,
        state.config.access_token_ttl_minutes,
    )))
}

pub async fn logout(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    Json(input): Json<LogoutInput>,
) -> Result<impl IntoResponse, AppError> {
    sqlx::query("UPDATE refresh_tokens SET revoked_at = now() WHERE token_hash = $1")
        .bind(hash_refresh_token(&input.refresh_token))
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
