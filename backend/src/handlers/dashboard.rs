//! Dashboard: stats, pipeline, top deals y actividades recientes.
//!
//! Adaptación a la plantilla segura:
//! - Auth: JWT válido (`Extension<Claims>`) + RBAC fino
//!   (`dashboard.view`) vía `UserPermissions`.
//! - DTOs de respuesta definidos aquí (el grupo A no porta `models/dashboard.rs`).
//! - Agregados NUMERIC casteados a `float8` en SQL (sin `rust_decimal`);
//!   `total_revenue`/`total_value`/`value` son `f64`.

use axum::{Extension, Json, extract::State, response::IntoResponse};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::middleware::auth_ext::UserPermissions;
use crate::{auth::Claims, error::AppError, state::AppState};

#[derive(Debug, Serialize)]
pub struct DashboardStats {
    pub total_contacts: i64,
    pub total_companies: i64,
    pub total_deals: i64,
    pub total_revenue: f64,
    pub active_deals: i64,
    pub won_deals: i64,
    pub lost_deals: i64,
}

#[derive(Debug, Serialize)]
pub struct PipelineStage {
    pub stage: String,
    pub count: i64,
    pub total_value: f64,
}

#[derive(Debug, Serialize)]
pub struct PipelineResponse {
    pub stages: Vec<PipelineStage>,
}

#[derive(Debug, Serialize)]
pub struct TopDeal {
    pub id: Uuid,
    pub title: String,
    pub value: f64,
    pub stage: String,
    pub company_name: Option<String>,
    pub expected_close_date: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct TopDealsResponse {
    pub deals: Vec<TopDeal>,
}

#[derive(Debug, Serialize)]
pub struct RecentActivity {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub activity_type: String,
    pub subject: String,
    pub contact_name: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct RecentActivitiesResponse {
    pub activities: Vec<RecentActivity>,
}

pub async fn get_dashboard_stats(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    perms: UserPermissions,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("dashboard.view")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    #[derive(sqlx::FromRow)]
    struct StatsRow {
        total_contacts: i64,
        total_companies: i64,
        total_deals: i64,
        total_revenue: Option<f64>,
        active_deals: i64,
        won_deals: i64,
        lost_deals: i64,
    }

    let row = sqlx::query_as::<_, StatsRow>(
        r#"
        SELECT
            (SELECT COUNT(*)::bigint FROM contacts) AS total_contacts,
            (SELECT COUNT(*)::bigint FROM companies) AS total_companies,
            (SELECT COUNT(*)::bigint FROM deals) AS total_deals,
            (SELECT COALESCE(SUM(value)::float8, 0.0) FROM deals) AS total_revenue,
            (SELECT COUNT(*)::bigint FROM deals WHERE stage NOT IN ('closed_won', 'closed_lost')) AS active_deals,
            (SELECT COUNT(*)::bigint FROM deals WHERE stage = 'closed_won') AS won_deals,
            (SELECT COUNT(*)::bigint FROM deals WHERE stage = 'closed_lost') AS lost_deals
        "#,
    )
    .fetch_one(&state.db)
    .await?;

    Ok(Json(DashboardStats {
        total_contacts: row.total_contacts,
        total_companies: row.total_companies,
        total_deals: row.total_deals,
        total_revenue: row.total_revenue.unwrap_or(0.0),
        active_deals: row.active_deals,
        won_deals: row.won_deals,
        lost_deals: row.lost_deals,
    }))
}

pub async fn get_pipeline(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    perms: UserPermissions,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("dashboard.view")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    #[derive(sqlx::FromRow)]
    struct StageRow {
        stage: String,
        count: i64,
        total_value: Option<f64>,
    }

    let rows = sqlx::query_as::<_, StageRow>(
        r#"
        SELECT stage::text AS stage, COUNT(*)::bigint AS count, COALESCE(SUM(value)::float8, 0.0) AS total_value
        FROM deals
        GROUP BY stage
        ORDER BY CASE stage
            WHEN 'lead' THEN 1
            WHEN 'qualified' THEN 2
            WHEN 'proposal' THEN 3
            WHEN 'negotiation' THEN 4
            WHEN 'closed_won' THEN 5
            WHEN 'closed_lost' THEN 6
            ELSE 7
        END
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    let stages: Vec<PipelineStage> = rows
        .into_iter()
        .map(|r| PipelineStage {
            stage: r.stage,
            count: r.count,
            total_value: r.total_value.unwrap_or(0.0),
        })
        .collect();

    Ok(Json(PipelineResponse { stages }))
}

pub async fn get_top_deals(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    perms: UserPermissions,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("dashboard.view")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    #[derive(sqlx::FromRow)]
    struct TopDealRow {
        id: Uuid,
        title: String,
        value: f64,
        stage: String,
        company_name: Option<String>,
        expected_close_date: Option<DateTime<Utc>>,
    }

    let rows = sqlx::query_as::<_, TopDealRow>(
        r#"
        SELECT d.id, d.title, d.value::float8 AS value, d.stage::text AS stage,
               c.name AS company_name, d.expected_close_date
        FROM deals d
        LEFT JOIN companies c ON d.company_id = c.id
        WHERE d.stage != 'closed_lost'
        ORDER BY d.value DESC
        LIMIT 5
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    let deals: Vec<TopDeal> = rows
        .into_iter()
        .map(|r| TopDeal {
            id: r.id,
            title: r.title,
            value: r.value,
            stage: r.stage,
            company_name: r.company_name,
            expected_close_date: r.expected_close_date,
        })
        .collect();

    Ok(Json(TopDealsResponse { deals }))
}

pub async fn get_recent_activities(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    perms: UserPermissions,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("dashboard.view")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    #[derive(sqlx::FromRow)]
    struct ActivityRow {
        id: Uuid,
        activity_type: String,
        subject: String,
        contact_name: Option<String>,
        created_at: DateTime<Utc>,
    }

    let rows = sqlx::query_as::<_, ActivityRow>(
        r#"
        SELECT a.id, a.activity_type::text AS activity_type, a.subject,
               CONCAT(c.first_name, ' ', c.last_name) AS contact_name,
               a.created_at
        FROM activities a
        LEFT JOIN contacts c ON a.contact_id = c.id
        ORDER BY a.created_at DESC
        LIMIT 10
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    let activities: Vec<RecentActivity> = rows
        .into_iter()
        .map(|r| RecentActivity {
            id: r.id,
            activity_type: r.activity_type,
            subject: r.subject,
            contact_name: r.contact_name,
            created_at: r.created_at,
        })
        .collect();

    Ok(Json(RecentActivitiesResponse { activities }))
}
