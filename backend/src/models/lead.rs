//! Modelos de leads: estados, fuentes, CRUD y actividades.
//!
//! - `LeadStatus` / `LeadSource` son enums Postgres (`lead_status`, `lead_source`).
//! - Los inputs usan `validator`; el SQL de los handlers solo bindea valores.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Serialize, Deserialize, sqlx::Type, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "lead_status", rename_all = "snake_case")]
pub enum LeadStatus {
    New,
    Contacted,
    Qualified,
    Unqualified,
    Converted,
    Recycled,
}

#[derive(Debug, Serialize, Deserialize, sqlx::Type, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "lead_source", rename_all = "snake_case")]
pub enum LeadSource {
    Web,
    Referral,
    ColdCall,
    Advertisement,
    Email,
    Social,
    Partner,
    Event,
    Other,
}

impl LeadStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Contacted => "contacted",
            Self::Qualified => "qualified",
            Self::Unqualified => "unqualified",
            Self::Converted => "converted",
            Self::Recycled => "recycled",
        }
    }
}

impl LeadSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::Referral => "referral",
            Self::ColdCall => "cold_call",
            Self::Advertisement => "advertisement",
            Self::Email => "email",
            Self::Social => "social",
            Self::Partner => "partner",
            Self::Event => "event",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Lead {
    pub id: Uuid,
    pub first_name: String,
    pub last_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub company_name: Option<String>,
    pub title: Option<String>,
    pub industry: Option<String>,
    pub website: Option<String>,
    pub lead_source: LeadSource,
    pub status: LeadStatus,
    pub score: i32,
    pub assigned_to: Option<Uuid>,
    pub converted_at: Option<DateTime<Utc>>,
    pub converted_contact_id: Option<Uuid>,
    pub converted_company_id: Option<Uuid>,
    pub converted_deal_id: Option<Uuid>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct LeadActivity {
    pub id: Uuid,
    pub lead_id: Uuid,
    pub r#type: String,
    pub subject: String,
    pub description: Option<String>,
    pub due_date: Option<DateTime<Utc>>,
    pub completed: bool,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateLead {
    #[validate(length(min = 1, max = 100, message = "first_name de 1 a 100 caracteres"))]
    pub first_name: String,
    #[validate(length(min = 1, max = 100, message = "last_name de 1 a 100 caracteres"))]
    pub last_name: String,
    #[validate(email(message = "email inválido"))]
    pub email: Option<String>,
    #[validate(length(max = 50, message = "phone max 50 caracteres"))]
    pub phone: Option<String>,
    #[validate(length(max = 255, message = "company_name max 255 caracteres"))]
    pub company_name: Option<String>,
    #[validate(length(max = 100, message = "title max 100 caracteres"))]
    pub title: Option<String>,
    #[validate(length(max = 100, message = "industry max 100 caracteres"))]
    pub industry: Option<String>,
    #[validate(length(max = 255, message = "website max 255 caracteres"))]
    pub website: Option<String>,
    pub lead_source: Option<LeadSource>,
    pub assigned_to: Option<Uuid>,
    #[validate(length(max = 1000, message = "notes max 1000 caracteres"))]
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateLead {
    #[validate(length(min = 1, max = 100, message = "first_name de 1 a 100 caracteres"))]
    pub first_name: Option<String>,
    #[validate(length(min = 1, max = 100, message = "last_name de 1 a 100 caracteres"))]
    pub last_name: Option<String>,
    #[validate(email(message = "email inválido"))]
    pub email: Option<String>,
    #[validate(length(max = 50, message = "phone max 50 caracteres"))]
    pub phone: Option<String>,
    #[validate(length(max = 255, message = "company_name max 255 caracteres"))]
    pub company_name: Option<String>,
    #[validate(length(max = 100, message = "title max 100 caracteres"))]
    pub title: Option<String>,
    #[validate(length(max = 100, message = "industry max 100 caracteres"))]
    pub industry: Option<String>,
    #[validate(length(max = 255, message = "website max 255 caracteres"))]
    pub website: Option<String>,
    pub lead_source: Option<LeadSource>,
    pub status: Option<LeadStatus>,
    pub score: Option<i32>,
    pub assigned_to: Option<Uuid>,
    #[validate(length(max = 1000, message = "notes max 1000 caracteres"))]
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct ConvertLead {
    pub pipeline_id: Uuid,
    #[validate(length(max = 255, message = "deal_title max 255 caracteres"))]
    pub deal_title: Option<String>,
    // TODO: el origen usa rust_decimal::Decimal; sin nuevas deps se usa f64.
    // Revisar redondeo al persistir en NUMERIC.
    pub deal_value: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct ConvertLeadResult {
    pub contact_id: Option<Uuid>,
    pub company_id: Option<Uuid>,
    pub deal_id: Option<Uuid>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateLeadActivity {
    #[validate(length(min = 1, max = 50, message = "type de 1 a 50 caracteres"))]
    pub r#type: String,
    #[validate(length(min = 1, max = 255, message = "subject de 1 a 255 caracteres"))]
    pub subject: String,
    #[validate(length(max = 2000, message = "description max 2000 caracteres"))]
    pub description: Option<String>,
    pub due_date: Option<DateTime<Utc>>,
}
