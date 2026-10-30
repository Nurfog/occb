//! Auth extendido: forgot/reset-password, perfil propio y cambio de password.
//!
//! Adaptación del `handlers/auth.rs` de origen al esquema destino:
//! - `users` tiene `name` (no `first_name`/`last_name`) y no tiene `profile_id`.
//! - `refresh_tokens` usa `revoked_at` (no `revoked` bool).
//! - Hashes nuevos SIEMPRE Argon2id (`crate::password`, política 12..128).
//! - TODO(bcrypt-compat): origen usaba bcrypt 8..72 y no se pueden añadir
//!   dependencias, así que NO hay fallback bcrypt: hashes bcrypt antiguos
//!   simplemente no verifican (ver `verify_password`). Si se necesita
//!   convivencia, añadir la dep `bcrypt` y reintentarlo aquí.
//! - Anti-enumeración: `forgot_password` y login inexistente devuelven igual
//!   con y sin usuario (mismo `200 OK` / `401` genérico).
//! - Todas las queries parametrizadas (`$1...` + `.bind`); `23505` -> 409
//!   (vía `From<sqlx::Error>`).
//!
//! NOTA de infra: la tabla `password_reset_tokens` NO existe en
//! `backend/migrations/` (no se pueden tocar). Los handlers la intentan usar
//! y degradan con gracia: `forgot` siempre `200 OK`; `reset` devuelve
//! `503 Unavailable` si la tabla falta (`42P01`), `422` si el token no vale.

use axum::{Extension, Json, extract::State, http::StatusCode, response::IntoResponse};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use validator::Validate;

use crate::{
    auth::Claims,
    error::AppError,
    middleware::auth_ext::{invalidate_user_permissions, load_permissions},
    models::UserPublic,
    password,
    state::AppState,
};

fn sha256_hex(s: &str) -> String {
    format!("{:x}", Sha256::digest(s.as_bytes()))
}

fn is_undefined_table(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(db) if db.code().as_deref() == Some("42P01"))
}

#[derive(Debug, Deserialize, Validate)]
pub struct ForgotPasswordInput {
    #[validate(email(message = "email inválido"), length(max = 320))]
    pub email: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct ResetPasswordInput {
    #[validate(length(min = 32, max = 256, message = "token inválido"))]
    pub token: String,
    /// La política fina (12..128) la aplica `password::hash_password`.
    #[validate(length(min = 1, max = 128))]
    pub password: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateProfileInput {
    #[validate(email(message = "email inválido"), length(max = 320))]
    pub email: Option<String>,
    #[validate(length(min = 1, max = 100, message = "nombre de 1 a 100 caracteres"))]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct ChangePasswordInput {
    #[validate(length(min = 1, max = 128))]
    pub current_password: String,
    #[validate(length(min = 1, max = 128))]
    pub new_password: String,
}

/// Solicita un email de reseteo. SIEMPRE `200 OK` (anti-enumeración).
pub async fn forgot_password(
    State(state): State<AppState>,
    Json(input): Json<ForgotPasswordInput>,
) -> Result<impl IntoResponse, AppError> {
    input.validate()?;
    let email = input.email.trim().to_lowercase();

    let user: Option<(uuid::Uuid,)> = sqlx::query_as("SELECT id FROM users WHERE email = $1")
        .bind(&email)
        .fetch_optional(&state.db)
        .await?;
    let Some((user_id,)) = user else {
        return Ok(StatusCode::OK);
    };

    // Token opaco de 256 bits (dos UUID v4); en DB solo su SHA-256.
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let expires_at = Utc::now() + Duration::hours(1);
    if let Err(e) = sqlx::query(
        "INSERT INTO password_reset_tokens (user_id, token_hash, expires_at)
         VALUES ($1, $2, $3)",
    )
    .bind(user_id)
    .bind(sha256_hex(&token))
    .bind(expires_at)
    .execute(&state.db)
    .await
    {
        if is_undefined_table(&e) {
            tracing::warn!(
                "password_reset_tokens ausente: no se puede persistir el reset (migración pendiente)"
            );
        } else {
            tracing::error!("error guardando token de reset: {e}");
        }
    }
    // TODO: enviar el email con el enlace cuando exista servicio SMTP
    // (origen: `services::email`; ver `state.config.frontend_url`).
    Ok(StatusCode::OK)
}

/// Consume un token de reset y fija la nueva contraseña (Argon2id).
pub async fn reset_password(
    State(state): State<AppState>,
    Json(input): Json<ResetPasswordInput>,
) -> Result<impl IntoResponse, AppError> {
    input.validate()?;
    // Costoso fuera de la transacción; valida política 12..128 aquí.
    let new_hash = password::hash_password(&input.password)?;
    let token_hash = sha256_hex(input.token.trim());

    let mut tx = state.db.begin().await.map_err(|_| AppError::Internal)?;
    // Reclama el token atómicamente (una sola request concurrente gana).
    let claimed: Result<Option<(uuid::Uuid,)>, sqlx::Error> = sqlx::query_as(
        "UPDATE password_reset_tokens SET used = TRUE
         WHERE token_hash = $1 AND used = FALSE AND expires_at > now()
         RETURNING user_id",
    )
    .bind(&token_hash)
    .fetch_optional(&mut *tx)
    .await;
    let claimed = match claimed {
        Ok(c) => c,
        Err(e) if is_undefined_table(&e) => return Err(AppError::Unavailable),
        Err(e) => {
            tracing::error!("error reclamando token de reset: {e}");
            return Err(AppError::Internal);
        }
    };
    let Some((user_id,)) = claimed else {
        return Err(AppError::Validation(
            "token inválido o expirado".to_string(),
        ));
    };

    sqlx::query("UPDATE users SET password_hash = $2 WHERE id = $1")
        .bind(user_id)
        .bind(&new_hash)
        .execute(&mut *tx)
        .await
        .map_err(|_| AppError::Internal)?;
    sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL",
    )
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| AppError::Internal)?;
    tx.commit().await.map_err(|_| AppError::Internal)?;

    invalidate_user_permissions(&user_id).await;
    Ok(StatusCode::OK)
}

/// Perfil propio del portador del JWT (contrato origen: incluye permisos).
#[derive(Debug, Serialize, sqlx::FromRow)]
struct ProfileRow {
    id: uuid::Uuid,
    email: String,
    name: String,
    first_name: Option<String>,
    last_name: Option<String>,
    profile_id: Option<uuid::Uuid>,
    is_superadmin: bool,
    tenant_id: Option<uuid::Uuid>,
}

#[derive(Debug, Serialize)]
struct ProfileResponse {
    id: uuid::Uuid,
    email: String,
    name: String,
    first_name: Option<String>,
    last_name: Option<String>,
    profile_id: Option<uuid::Uuid>,
    /// Root instalado (platform). El frontend lo usa para el dashboard.
    is_superadmin: bool,
    /// Empresa del usuario (NULL = platform).
    tenant_id: Option<uuid::Uuid>,
    permissions: Vec<String>,
}

pub async fn get_profile(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<impl IntoResponse, AppError> {
    let user: Option<ProfileRow> = sqlx::query_as::<_, ProfileRow>(
        "SELECT id, email, name, first_name, last_name, profile_id, is_superadmin, tenant_id FROM users WHERE id = $1",
    )
    .bind(claims.sub)
    .fetch_optional(&state.db)
    .await?;
    let user = user.ok_or(AppError::Unauthorized)?;
    let permissions = load_permissions(&state, claims.sub)
        .await
        .map_err(|_| AppError::Internal)?;
    Ok(Json(ProfileResponse {
        id: user.id,
        email: user.email,
        name: user.name,
        first_name: user.first_name,
        last_name: user.last_name,
        profile_id: user.profile_id,
        is_superadmin: user.is_superadmin,
        tenant_id: user.tenant_id,
        permissions,
    }))
}

/// Actualiza email y/o nombre propios. `23505` (email duplicado) -> 409.
pub async fn update_profile(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(input): Json<UpdateProfileInput>,
) -> Result<impl IntoResponse, AppError> {
    input.validate()?;
    if input.email.is_none() && input.name.is_none() {
        return Err(AppError::Validation("nada que actualizar".to_string()));
    }
    let email = input.email.as_deref().map(|e| e.trim().to_lowercase());
    let name = input.name.as_deref().map(str::trim);
    let user: Option<UserPublic> = sqlx::query_as::<_, UserPublic>(
        "UPDATE users SET email = COALESCE($2, email), name = COALESCE($3, name)
         WHERE id = $1
         RETURNING id, email, name, created_at",
    )
    .bind(claims.sub)
    .bind(email.as_deref())
    .bind(name)
    .fetch_optional(&state.db)
    .await?;
    user.map(Json).ok_or(AppError::Unauthorized)
}

/// Cambia la contraseña verificando la actual; revoca todos los refresh.
pub async fn change_password(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(input): Json<ChangePasswordInput>,
) -> Result<impl IntoResponse, AppError> {
    input.validate()?;
    let row: Option<(String,)> = sqlx::query_as("SELECT password_hash FROM users WHERE id = $1")
        .bind(claims.sub)
        .fetch_optional(&state.db)
        .await?;
    let Some((hash,)) = row else {
        return Err(AppError::Unauthorized);
    };
    // Solo Argon2 (ver TODO bcrypt-compat en cabecera).
    if !password::verify_password(&hash, &input.current_password) {
        return Err(AppError::Unauthorized);
    }
    let new_hash = password::hash_password(&input.new_password)?;
    sqlx::query("UPDATE users SET password_hash = $2 WHERE id = $1")
        .bind(claims.sub)
        .bind(&new_hash)
        .execute(&state.db)
        .await?;
    sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL",
    )
    .bind(claims.sub)
    .execute(&state.db)
    .await?;

    invalidate_user_permissions(&claims.sub).await;
    Ok(StatusCode::OK)
}
