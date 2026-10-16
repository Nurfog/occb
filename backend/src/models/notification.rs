//! Modelos de notificaciones por usuario.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Notification {
    pub id: Uuid,
    pub user_id: Uuid,
    pub title: String,
    pub message: String,
    pub entity_type: Option<String>,
    pub entity_id: Option<Uuid>,
    pub read: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateNotification {
    pub user_id: Uuid,
    #[validate(length(min = 1, max = 200, message = "title de 1 a 200 caracteres"))]
    pub title: String,
    #[validate(length(min = 1, max = 2000, message = "message de 1 a 2000 caracteres"))]
    pub message: String,
    #[validate(length(max = 50, message = "entity_type max 50 caracteres"))]
    pub entity_type: Option<String>,
    pub entity_id: Option<Uuid>,
}
