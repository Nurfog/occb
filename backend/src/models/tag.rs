//! Modelos de etiquetas (tags) y su asignación a entidades.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Tag {
    pub id: Uuid,
    pub name: String,
    pub color: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateTag {
    #[validate(length(min = 1, max = 50, message = "name de 1 a 50 caracteres"))]
    pub name: String,
    #[validate(length(max = 7, message = "color max 7 caracteres"))]
    pub color: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateTag {
    #[validate(length(min = 1, max = 50, message = "name de 1 a 50 caracteres"))]
    pub name: Option<String>,
    #[validate(length(max = 7, message = "color max 7 caracteres"))]
    pub color: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct AssignTagRequest {
    pub tag_id: Uuid,
    #[validate(length(min = 1, max = 50, message = "entity_type de 1 a 50 caracteres"))]
    pub entity_type: String,
    pub entity_id: Uuid,
}

#[derive(Debug, Serialize)]
// Fila de `entity_tags` con join (contrato; el endpoint devuelve `Tag` + fecha por separado).
#[allow(dead_code)]
pub struct EntityTag {
    pub tag: Tag,
    pub assigned_at: DateTime<Utc>,
}
