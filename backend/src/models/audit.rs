//! Modelos de auditoría (log append-only de cambios).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct AuditLog {
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    pub action: String,
    pub entity_type: String,
    pub entity_id: Uuid,
    pub old_values: Option<serde_json::Value>,
    pub new_values: Option<serde_json::Value>,
    pub ip_address: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct AuditFilter {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
    #[validate(length(max = 50, message = "entity_type max 50 caracteres"))]
    pub entity_type: Option<String>,
    pub entity_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
}

impl AuditFilter {
    pub fn page_clamped(&self) -> i64 {
        self.page.unwrap_or(1).clamp(1, 10_000)
    }
    pub fn per_page_clamped(&self) -> i64 {
        self.per_page.unwrap_or(20).clamp(1, 100)
    }
    pub fn offset(&self) -> i64 {
        (self.page_clamped() - 1) * self.per_page_clamped()
    }
}
