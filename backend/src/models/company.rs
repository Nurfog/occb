//! Modelo `Company` + DTOs de creación/actualización (grupo A, port origen).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Company {
    pub id: Uuid,
    pub name: String,
    pub industry: Option<String>,
    pub website: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub address: Option<String>,
    pub city: Option<String>,
    pub country: Option<String>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateCompany {
    #[validate(length(min = 1, max = 200, message = "Name must be 1-200 characters"))]
    pub name: String,

    #[validate(length(max = 100, message = "Industry must be at most 100 characters"))]
    pub industry: Option<String>,

    #[validate(url(message = "Website must be a valid URL"))]
    pub website: Option<String>,

    #[validate(length(max = 20, message = "Phone must be at most 20 characters"))]
    pub phone: Option<String>,

    #[validate(email(message = "Email must be valid"))]
    pub email: Option<String>,

    #[validate(length(max = 200, message = "Address must be at most 200 characters"))]
    pub address: Option<String>,

    #[validate(length(max = 100, message = "City must be at most 100 characters"))]
    pub city: Option<String>,

    #[validate(length(max = 100, message = "Country must be at most 100 characters"))]
    pub country: Option<String>,

    #[validate(length(max = 1000, message = "Notes must be at most 1000 characters"))]
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateCompany {
    #[validate(length(min = 1, max = 200, message = "Name must be 1-200 characters"))]
    pub name: Option<String>,

    #[validate(length(max = 100, message = "Industry must be at most 100 characters"))]
    pub industry: Option<String>,

    #[validate(url(message = "Website must be a valid URL"))]
    pub website: Option<String>,

    #[validate(length(max = 20, message = "Phone must be at most 20 characters"))]
    pub phone: Option<String>,

    #[validate(email(message = "Email must be valid"))]
    pub email: Option<String>,

    #[validate(length(max = 200, message = "Address must be at most 200 characters"))]
    pub address: Option<String>,

    #[validate(length(max = 100, message = "City must be at most 100 characters"))]
    pub city: Option<String>,

    #[validate(length(max = 100, message = "Country must be at most 100 characters"))]
    pub country: Option<String>,

    #[validate(length(max = 1000, message = "Notes must be at most 1000 characters"))]
    pub notes: Option<String>,
}
