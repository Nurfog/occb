//! Modelos de documentos (metadatos; el binario vive fuera de la DB).
//!
//! TODO storage: `AppState` no expone directorio de uploads; los endpoints
//! que tocan el filesystem devuelven `Unavailable` hasta configurarlo.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Document {
    pub id: Uuid,
    pub filename: String,
    pub original_name: String,
    pub mime_type: Option<String>,
    pub file_size: i64,
    pub folder: Option<String>,
    pub uploaded_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateDocument {
    #[validate(length(min = 1, max = 255, message = "filename de 1 a 255 caracteres"))]
    pub filename: String,
    #[validate(length(min = 1, max = 255, message = "original_name de 1 a 255 caracteres"))]
    pub original_name: String,
    #[validate(length(max = 127, message = "mime_type max 127 caracteres"))]
    pub mime_type: Option<String>,
    #[validate(range(min = 0, message = "file_size no negativo"))]
    pub file_size: i64,
    #[validate(length(max = 100, message = "folder max 100 caracteres"))]
    pub folder: Option<String>,
    pub uploaded_by: Option<Uuid>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct DocumentFilter {
    #[validate(length(max = 100, message = "folder max 100 caracteres"))]
    pub folder: Option<String>,
    #[validate(length(max = 200, message = "search max 200 caracteres"))]
    pub search: Option<String>,
    #[validate(length(max = 127, message = "mime_type max 127 caracteres"))]
    pub mime_type: Option<String>,
}
