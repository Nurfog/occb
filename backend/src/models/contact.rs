//! Modelo `Contact` + DTOs y filtros de listado (grupo A, port origen).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Contact {
    pub id: Uuid,
    pub first_name: String,
    pub last_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub company_id: Option<Uuid>,
    pub position: Option<String>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateContact {
    #[validate(length(min = 1, max = 100, message = "First name must be 1-100 characters"))]
    pub first_name: String,

    #[validate(length(min = 1, max = 100, message = "Last name must be 1-100 characters"))]
    pub last_name: String,

    #[validate(email(message = "Email must be valid"))]
    pub email: Option<String>,

    #[validate(length(max = 20, message = "Phone must be at most 20 characters"))]
    pub phone: Option<String>,

    pub company_id: Option<Uuid>,

    #[validate(length(max = 100, message = "Position must be at most 100 characters"))]
    pub position: Option<String>,

    #[validate(length(max = 1000, message = "Notes must be at most 1000 characters"))]
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateContact {
    #[validate(length(min = 1, max = 100, message = "First name must be 1-100 characters"))]
    pub first_name: Option<String>,

    #[validate(length(min = 1, max = 100, message = "Last name must be 1-100 characters"))]
    pub last_name: Option<String>,

    #[validate(email(message = "Email must be valid"))]
    pub email: Option<String>,

    #[validate(length(max = 20, message = "Phone must be at most 20 characters"))]
    pub phone: Option<String>,

    pub company_id: Option<Uuid>,

    #[validate(length(max = 100, message = "Position must be at most 100 characters"))]
    pub position: Option<String>,

    #[validate(length(max = 1000, message = "Notes must be at most 1000 characters"))]
    pub notes: Option<String>,
}

/// Filtros opcionales de listado (los usa la UI de filtros avanzados).
/// Todos opcionales; las fechas son strings `YYYY-MM-DD` parseadas en el repo.
#[derive(Debug, Clone, Default, Deserialize, Validate)]
pub struct ContactFilters {
    #[validate(length(max = 200, message = "Company filter must be at most 200 characters"))]
    pub company: Option<String>,

    #[validate(length(max = 200, message = "Position filter must be at most 200 characters"))]
    pub position: Option<String>,

    pub created_after: Option<String>,

    pub created_before: Option<String>,
}
