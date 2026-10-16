//! Handlers de relaciones entre entidades (grupo A, port origen adaptado).
//!
//! - Auth: JWT válido (`Extension<Claims>`) + RBAC fino
//!   (`contacts/deals/companies.view`) vía `UserPermissions`.
//! - `Deal.value` es `f64`: los SELECT castean `value::float8 AS value`.
//! - `get_company_revenue`: `SUM(value)::float8` (NUMERIC no decodifica a
//!   `f64` sin `rust_decimal`).

use axum::{
    Extension, Json,
    extract::{Path, State},
    response::IntoResponse,
};
use uuid::Uuid;

use crate::middleware::auth_ext::UserPermissions;
use crate::models::{Activity, Contact, Deal};
use crate::{auth::Claims, error::AppError, state::AppState};

const DEAL_COLS: &str = "id, title, value::float8 AS value, currency, stage, position, contact_id, company_id, expected_close_date, notes, created_at, updated_at";
const ACTIVITY_COLS: &str = "id, subject, description, activity_type, due_date, completed, contact_id, deal_id, company_id, COALESCE(recurrence_type, 'none') AS recurrence_type, recurrence_interval, recurrence_end_date, parent_activity_id, created_at, updated_at";

pub async fn get_contact_deals(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("contacts.view")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    let deals = sqlx::query_as::<_, Deal>(&format!(
        "SELECT {DEAL_COLS} FROM deals WHERE contact_id = $1 ORDER BY created_at DESC"
    ))
    .bind(id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(deals))
}

pub async fn get_contact_activities(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("contacts.view")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    let activities = sqlx::query_as::<_, Activity>(&format!(
        "SELECT {ACTIVITY_COLS} FROM activities WHERE contact_id = $1 ORDER BY due_date DESC"
    ))
    .bind(id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(activities))
}

pub async fn get_company_contacts(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("companies.view")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    let contacts = sqlx::query_as::<_, Contact>(
        "SELECT id, first_name, last_name, email, phone, company_id, position, notes, created_at, updated_at FROM contacts WHERE company_id = $1 ORDER BY created_at DESC"
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(contacts))
}

pub async fn get_company_deals(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("companies.view")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    let deals = sqlx::query_as::<_, Deal>(&format!(
        "SELECT {DEAL_COLS} FROM deals WHERE company_id = $1 ORDER BY created_at DESC"
    ))
    .bind(id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(deals))
}

pub async fn get_deal_activities(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("deals.view")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    let activities = sqlx::query_as::<_, Activity>(&format!(
        "SELECT {ACTIVITY_COLS} FROM activities WHERE deal_id = $1 ORDER BY due_date DESC"
    ))
    .bind(id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(activities))
}

pub async fn get_company_revenue(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("companies.view")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    let result: (Option<f64>, i64) = sqlx::query_as(
        "SELECT SUM(value)::float8, COUNT(*) FROM deals WHERE company_id = $1 AND stage IN ('closed_won', 'proposal', 'negotiation')"
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(serde_json::json!({
        "total_value": result.0.unwrap_or(0.0),
        "deal_count": result.1,
    })))
}
