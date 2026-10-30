//! Reportes: pipeline y win/loss.
//!
//! Adaptación a la plantilla segura:
//! - Auth: JWT válido (`Extension<Claims>`) + RBAC fino
//!   (`reports.view`) vía `UserPermissions`.
//! - DTOs definidos aquí (el grupo A no porta `models/report.rs`).
//! - `SUM(value)::double precision` ya castea en SQL (origen), `f64` en Rust.

use axum::{Extension, Json, extract::State, response::IntoResponse};

use crate::middleware::auth_ext::UserPermissions;
use crate::{
    auth::Claims,
    error::AppError,
    models::report::{PipelineReport, PipelineReportItem, WinLossReport},
    state::AppState,
};

#[derive(sqlx::FromRow)]
struct StageRow {
    stage: String,
    count: i64,
    total_value: f64,
}

pub async fn get_pipeline_report(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    perms: UserPermissions,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("reports.view")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    let scope = crate::auth::tenant_scope(&state, &claims.sub).await?;
    let total_row: (i64, Option<f64>) =
        sqlx::query_as("SELECT COUNT(*), COALESCE(SUM(value)::double precision, 0.0) FROM deals WHERE ($1::uuid IS NULL OR tenant_id = $1)")
            .bind(scope)
            .fetch_one(&state.db)
            .await?;

    let total_deals = total_row.0;
    let total_value = total_row.1.unwrap_or(0.0);

    let rows = sqlx::query_as::<_, StageRow>(
        r#"
        SELECT stage::text AS stage, COUNT(*) AS count, COALESCE(SUM(value)::double precision, 0.0) AS total_value
        FROM deals
        WHERE ($1::uuid IS NULL OR tenant_id = $1)
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
    .bind(scope)
    .fetch_all(&state.db)
    .await?;

    let stages: Vec<PipelineReportItem> = rows
        .into_iter()
        .map(|r| {
            let avg_value = if r.count > 0 {
                r.total_value / r.count as f64
            } else {
                0.0
            };
            let percentage = if total_deals > 0 {
                (r.count as f64 / total_deals as f64) * 100.0
            } else {
                0.0
            };
            PipelineReportItem {
                stage: r.stage,
                count: r.count,
                total_value: r.total_value,
                avg_value,
                percentage,
            }
        })
        .collect();

    Ok(Json(PipelineReport {
        stages,
        total_deals,
        total_value,
    }))
}

pub async fn get_win_loss_report(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    perms: UserPermissions,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("reports.view")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    let scope = crate::auth::tenant_scope(&state, &claims.sub).await?;
    let won_row: (i64, Option<f64>) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(value)::double precision, 0.0) FROM deals WHERE stage = 'closed_won' AND ($1::uuid IS NULL OR tenant_id = $1)",
    )
    .bind(scope)
    .fetch_one(&state.db)
    .await?;

    let lost_row: (i64, Option<f64>) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(value)::double precision, 0.0) FROM deals WHERE stage = 'closed_lost' AND ($1::uuid IS NULL OR tenant_id = $1)",
    )
    .bind(scope)
    .fetch_one(&state.db)
    .await?;

    let won_count = won_row.0;
    let won_value = won_row.1.unwrap_or(0.0);
    let lost_count = lost_row.0;
    let lost_value = lost_row.1.unwrap_or(0.0);
    let total_closed = won_count + lost_count;

    let win_rate = if total_closed > 0 {
        (won_count as f64 / total_closed as f64) * 100.0
    } else {
        0.0
    };

    let loss_rate = if total_closed > 0 {
        (lost_count as f64 / total_closed as f64) * 100.0
    } else {
        0.0
    };

    Ok(Json(WinLossReport {
        won_count,
        lost_count,
        win_rate,
        loss_rate,
        won_value,
        lost_value,
        total_closed,
    }))
}
