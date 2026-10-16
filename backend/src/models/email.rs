//! Grupo C (port) — modelos de email.
//!
//! El envío SMTP real no existe en este destino (sin lettre/`state.smtp`):
//! los handlers validan el input y devuelven 503.
//! TODO port: requiere SMTP (lettre) + `state.smtp`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct EmailLog {
    pub id: Uuid,
    pub from_email: String,
    pub to_email: String,
    pub cc: Option<String>,
    pub bcc: Option<String>,
    pub subject: String,
    pub body: String,
    pub body_html: Option<String>,
    pub entity_type: Option<String>,
    pub entity_id: Option<Uuid>,
    pub status: String,
    pub template_id: Option<Uuid>,
    pub sent_by: Option<Uuid>,
    pub sent_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct EmailTemplate {
    pub id: Uuid,
    pub name: String,
    pub subject: String,
    pub body: String,
    pub body_html: Option<String>,
    pub category: String,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct SendEmail {
    #[validate(email(message = "email `from` inválido"))]
    pub from: Option<String>,
    #[validate(email(message = "email `to` inválido"))]
    pub to: String,
    pub cc: Option<String>,
    pub bcc: Option<String>,
    #[validate(length(min = 1, max = 500, message = "asunto de 1 a 500 caracteres"))]
    pub subject: String,
    #[validate(length(min = 1, message = "cuerpo requerido"))]
    pub body: String,
    pub body_html: Option<String>,
    pub entity_type: Option<String>,
    pub entity_id: Option<Uuid>,
    pub template_id: Option<Uuid>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateEmailTemplate {
    #[validate(length(min = 1, max = 100, message = "nombre de 1 a 100 caracteres"))]
    pub name: String,
    #[validate(length(min = 1, max = 500, message = "asunto de 1 a 500 caracteres"))]
    pub subject: String,
    #[validate(length(min = 1, message = "cuerpo requerido"))]
    pub body: String,
    pub body_html: Option<String>,
    pub category: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateEmailTemplate {
    pub name: Option<String>,
    pub subject: Option<String>,
    pub body: Option<String>,
    pub body_html: Option<String>,
    pub category: Option<String>,
}
