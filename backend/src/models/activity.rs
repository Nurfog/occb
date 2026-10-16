//! Modelo `Activity` + DTOs de creación/actualización (grupo A, port origen).
//!
//! - `activity_type` mapea al ENUM Postgres `activity_type`.
//! - `recurrence_type` se guarda como VARCHAR(20); el enum mapea a etiquetas
//!   en minúsculas. Los SELECT usan `COALESCE(recurrence_type,'none')` para
//!   filas legacy con NULL.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "activity_type", rename_all = "lowercase")]
pub enum ActivityType {
    Call,
    Email,
    Meeting,
    Task,
    Note,
}

#[derive(Debug, Serialize, Deserialize, sqlx::Type)]
// No existe ENUM Postgres para esto (la migración guarda VARCHAR(20)):
// el enum mapea a/desde etiquetas de texto.
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "varchar", rename_all = "lowercase")]
pub enum RecurrenceType {
    None,
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Activity {
    pub id: Uuid,
    pub activity_type: ActivityType,
    pub subject: String,
    pub description: Option<String>,
    pub contact_id: Option<Uuid>,
    pub deal_id: Option<Uuid>,
    pub company_id: Option<Uuid>,
    pub due_date: Option<DateTime<Utc>>,
    pub completed: bool,
    pub recurrence_type: RecurrenceType,
    pub recurrence_interval: Option<i32>,
    pub recurrence_end_date: Option<NaiveDate>,
    pub parent_activity_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateActivity {
    pub activity_type: ActivityType,

    #[validate(length(min = 1, max = 255, message = "Subject must be 1-255 characters"))]
    pub subject: String,

    #[validate(length(max = 2000, message = "Description must be at most 2000 characters"))]
    pub description: Option<String>,

    pub contact_id: Option<Uuid>,

    pub deal_id: Option<Uuid>,

    pub company_id: Option<Uuid>,

    pub due_date: Option<DateTime<Utc>>,

    pub recurrence_type: Option<RecurrenceType>,
    pub recurrence_interval: Option<i32>,
    pub recurrence_end_date: Option<NaiveDate>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateActivity {
    pub activity_type: Option<ActivityType>,
    pub subject: Option<String>,
    pub description: Option<String>,
    pub contact_id: Option<Uuid>,
    pub deal_id: Option<Uuid>,
    pub company_id: Option<Uuid>,
    pub due_date: Option<DateTime<Utc>>,
    pub completed: Option<bool>,
    pub recurrence_type: Option<RecurrenceType>,
    pub recurrence_interval: Option<i32>,
    pub recurrence_end_date: Option<NaiveDate>,
}
