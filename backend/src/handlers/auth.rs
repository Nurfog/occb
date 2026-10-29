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
struct UserRow {
    id: Uuid,
    email: String,
    name: String,
    password_hash: String,
    profile: String,
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
            profile: user.profile.clone(),
            created_at: user.created_at,
        },
        access_token: access,
        refresh_token: refresh,
        token_type: "Bearer".to_string(),
        expires_in: ttl_minutes * 60,
    }
}

const USER_SELECT: &str = "SELECT u.id, u.email, u.name, u.password_hash, \
    p.name AS profile, u.created_at \
    FROM users u JOIN profiles p ON p.id = u.profile_id";

pub async fn register(
    State(state): State<AppState>,
    Json(input): Json<RegisterInput>,
) -> Result<impl IntoResponse, AppError> {
    input.validate()?;
    let email = input.email.trim().to_lowercase();
    let hash = password::hash_password(&input.password)?;
    // Bootstrap: si la tabla está vacía, este usuario es admin.
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (email, name, password_hash, profile_id) \
         VALUES ($1, $2, $3, \
           (SELECT id FROM profiles WHERE name = \
             CASE WHEN (SELECT COUNT(*) FROM users) = 0 THEN 'admin' ELSE 'user' END)) \
         RETURNING id",
    )
    .bind(&email)
    .bind(input.name.trim())
    .bind(&hash)
    .fetch_one(&state.db)
    .await?;
    let user = sqlx::query_as::<_, UserRow>(&format!("{USER_SELECT} WHERE u.id = $1"))
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    let (access, refresh) = issue_pair(&state, &user).await?;
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

pub async fn login(
    State(state): State<AppState>,
    Json(input): Json<LoginInput>,
) -> Result<impl IntoResponse, AppError> {
    input.validate()?;
    let email = input.email.trim().to_lowercase();
    let user: Option<UserRow> =
        sqlx::query_as::<_, UserRow>(&format!("{USER_SELECT} WHERE u.email = $1"))
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

    let user: UserRow = sqlx::query_as::<_, UserRow>(&format!("{USER_SELECT} WHERE u.id = $1"))
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

pub async fn me(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<impl IntoResponse, AppError> {
    let user: Option<UserPublic> = sqlx::query_as::<_, UserPublic>(
        "SELECT u.id, u.email, u.name, p.name AS profile, u.created_at \
         FROM users u JOIN profiles p ON p.id = u.profile_id \
         WHERE u.id = $1",
    )
    .bind(claims.sub)
    .fetch_optional(&state.db)
    .await?;
    user.map(Json).ok_or(AppError::Unauthorized)
}
