//! Endpoints de notificaciones del usuario autenticado.
//!
//! Todas las operaciones se acotan a `claims.sub` (Uuid directo, sin parse).

use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{
    auth::Claims, error::AppError, middleware::auth_ext::UserPermissions,
    models::notification::Notification, state::AppState,
};

const NOTIF_COLUMNS: &str = "id, user_id, title, message, entity_type, entity_id, read, created_at";

pub async fn list_notifications(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<impl IntoResponse, AppError> {
    let sql = format!(
        "SELECT {NOTIF_COLUMNS} FROM notifications WHERE user_id = $1 ORDER BY created_at DESC LIMIT 50"
    );
    let notifications = sqlx::query_as::<_, Notification>(&sql)
        .bind(claims.sub)
        .fetch_all(&state.db)
        .await?;
    Ok(Json(notifications))
}

pub async fn get_unread_count(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<impl IntoResponse, AppError> {
    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM notifications WHERE user_id = $1 AND read = FALSE")
            .bind(claims.sub)
            .fetch_one(&state.db)
            .await?;
    Ok(Json(serde_json::json!({ "count": count.0 })))
}

pub async fn mark_as_read(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let result = sqlx::query("UPDATE notifications SET read = TRUE WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(claims.sub)
        .execute(&state.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::OK)
}

pub async fn mark_all_as_read(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<impl IntoResponse, AppError> {
    sqlx::query("UPDATE notifications SET read = TRUE WHERE user_id = $1 AND read = FALSE")
        .bind(claims.sub)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::OK)
}

pub async fn delete_notification(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let result = sqlx::query("DELETE FROM notifications WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(claims.sub)
        .execute(&state.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, Validate)]
pub struct TestEmailInput {
    #[validate(email(message = "email destino inválido"))]
    pub to: String,
    #[validate(length(max = 500, message = "subject max 500 caracteres"))]
    pub subject: Option<String>,
    #[validate(length(max = 10000, message = "body max 10000 caracteres"))]
    pub body: Option<String>,
}

/// Envía un correo de prueba vía SMTP (`services::email`) con la dirección
/// del usuario actual como remitente (SMTP `from` como respaldo).
pub async fn send_test_email(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(claims): Extension<Claims>,
    Json(input): Json<TestEmailInput>,
) -> Result<StatusCode, AppError> {
    perms
        .require("notifications.manage")
        .map_err(|_| AppError::Forbidden)?;
    input.validate()?;
    let user_email: Option<String> = sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
        .bind(claims.sub)
        .fetch_optional(&state.db)
        .await?;
    let msg = crate::services::email::EmailMessage {
        from: user_email,
        to: input.to,
        cc: None,
        bcc: None,
        subject: input
            .subject
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "OCCB — correo de prueba".to_string()),
        body_text: input
            .body
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| {
                "Si lees esto, el envío SMTP está configurado correctamente.".to_string()
            }),
        body_html: None,
    };
    crate::services::email::send_via_smtp(&state.smtp, &msg).await?;
    Ok(StatusCode::OK)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NotificationPreferences {
    pub user_id: Uuid,
    pub email_enabled: bool,
    pub push_enabled: bool,
    pub weekly_digest: bool,
    pub marketing_emails: bool,
}

pub async fn get_notification_preferences(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<impl IntoResponse, AppError> {
    let prefs = sqlx::query_as::<_, (bool, bool, bool, bool)>(
        "SELECT email_enabled, push_enabled, weekly_digest, marketing_emails \
         FROM notification_preferences WHERE user_id = $1",
    )
    .bind(claims.sub)
    .fetch_optional(&state.db)
    .await?
    .unwrap_or((true, true, false, false));
    Ok(Json(NotificationPreferences {
        user_id: claims.sub,
        email_enabled: prefs.0,
        push_enabled: prefs.1,
        weekly_digest: prefs.2,
        marketing_emails: prefs.3,
    }))
}

#[derive(Debug, Deserialize)]
pub struct UpdateNotificationPreferencesInput {
    pub email_enabled: Option<bool>,
    pub push_enabled: Option<bool>,
    pub weekly_digest: Option<bool>,
    pub marketing_emails: Option<bool>,
}

pub async fn update_notification_preferences(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(input): Json<UpdateNotificationPreferencesInput>,
) -> Result<impl IntoResponse, AppError> {
    let current = sqlx::query_as::<_, (bool, bool, bool, bool)>(
        "SELECT email_enabled, push_enabled, weekly_digest, marketing_emails \
         FROM notification_preferences WHERE user_id = $1",
    )
    .bind(claims.sub)
    .fetch_optional(&state.db)
    .await?
    .unwrap_or((true, true, false, false));

    sqlx::query(
        "INSERT INTO notification_preferences \
            (user_id, email_enabled, push_enabled, weekly_digest, marketing_emails) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (user_id) DO UPDATE SET \
            email_enabled = EXCLUDED.email_enabled, \
            push_enabled = EXCLUDED.push_enabled, \
            weekly_digest = EXCLUDED.weekly_digest, \
            marketing_emails = EXCLUDED.marketing_emails, \
            updated_at = NOW()",
    )
    .bind(claims.sub)
    .bind(input.email_enabled.unwrap_or(current.0))
    .bind(input.push_enabled.unwrap_or(current.1))
    .bind(input.weekly_digest.unwrap_or(current.2))
    .bind(input.marketing_emails.unwrap_or(current.3))
    .execute(&state.db)
    .await?;
    Ok(StatusCode::OK)
}
