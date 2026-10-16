//! Modelo `Deal` + DTOs (grupo A, port origen adaptado).
//!
//! Adaptación: el origen usa `rust_decimal::Decimal` para `value`; la
//! plantilla destino no tiene esa dependencia, así que `value` es `f64`.
//! - Validación de no-negativo con `range(min = 0.0)` de `validator`.
//! - Todo SQL que lea/escriba `value` debe castear (`value::float8 AS value`,
//!   `SUM(value)::float8`) porque la columna es NUMERIC y sqlx sin
//!   `rust_decimal` no la decodifica a `f64` directamente.
//! - `expected_close_date` acepta RFC 3339 o `YYYY-MM-DD` (ver `date_flex`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::models::date_flex;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "deal_stage", rename_all = "snake_case")]
pub enum DealStage {
    Lead,
    Qualified,
    Proposal,
    Negotiation,
    ClosedWon,
    ClosedLost,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Deal {
    pub id: Uuid,
    pub title: String,
    pub value: f64,
    pub currency: String,
    pub stage: DealStage,
    pub position: Option<i32>,
    pub contact_id: Option<Uuid>,
    pub company_id: Option<Uuid>,
    #[sqlx(default)]
    pub pipeline_id: Option<Uuid>,
    #[sqlx(default)]
    pub pipeline_stage_id: Option<Uuid>,
    pub expected_close_date: Option<DateTime<Utc>>,
    pub notes: Option<String>,
    #[sqlx(default)]
    pub contact_name: Option<String>,
    #[sqlx(default)]
    pub company_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateDealStage {
    pub stage: DealStage,
    pub position: Option<i32>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateDeal {
    #[validate(length(min = 1, max = 200, message = "Title must be 1-200 characters"))]
    pub title: String,

    #[validate(range(min = 0.0, message = "Value must be non-negative"))]
    pub value: f64,

    #[validate(length(max = 3, message = "Currency must be at most 3 characters"))]
    pub currency: Option<String>,

    pub stage: Option<DealStage>,

    pub contact_id: Option<Uuid>,

    pub company_id: Option<Uuid>,

    #[serde(default, with = "date_flex")]
    pub expected_close_date: Option<DateTime<Utc>>,

    #[validate(length(max = 1000, message = "Notes must be at most 1000 characters"))]
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateDeal {
    #[validate(length(min = 1, max = 200, message = "Title must be 1-200 characters"))]
    pub title: Option<String>,

    #[validate(range(min = 0.0, message = "Value must be non-negative"))]
    pub value: Option<f64>,

    #[validate(length(max = 3, message = "Currency must be at most 3 characters"))]
    pub currency: Option<String>,

    pub stage: Option<DealStage>,

    pub contact_id: Option<Uuid>,

    pub company_id: Option<Uuid>,

    #[serde(default, with = "date_flex")]
    pub expected_close_date: Option<DateTime<Utc>>,

    #[validate(length(max = 1000, message = "Notes must be at most 1000 characters"))]
    pub notes: Option<String>,
}
