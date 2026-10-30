//! Handlers de actividades: listar (con filtros), crear, actualizar,
//! borrar y completar.
//!
//! Adaptación a la plantilla segura:
//! - Auth: JWT válido (`Extension<Claims>`) + RBAC fino
//!   (`activities.view/create/edit/delete`) vía `UserPermissions`.
//! - Auditoría best-effort incluida.
//! - `list_activities`: filtros con binds (`$n`); solo se interpolan los
//!   índices de parámetro. `activity_type` se valida contra lista blanca.
//! - `update_activity`: el origen bindeaba `recurrence_*` sin hueco en el
//!   SET (binds extra); aquí el SET incluye esos campos explícitamente.

use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;
use uuid::Uuid;
use validator::Validate;

use crate::middleware::auth_ext::UserPermissions;
use crate::models::{Activity, CreateActivity, PaginationParams, UpdateActivity};
use crate::{auth::Claims, error::AppError, state::AppState};

#[derive(Debug, Deserialize)]
pub struct ActivityFilter {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
    pub search: Option<String>,
    pub sort_by: Option<String>,
    pub sort_order: Option<String>,
    pub activity_type: Option<String>,
    pub contact_id: Option<Uuid>,
    pub deal_id: Option<Uuid>,
    pub company_id: Option<Uuid>,
}

/// `activity_type` del query debe ser un valor del ENUM (lista blanca).
fn normalize_activity_type(s: &str) -> Result<&'static str, AppError> {
    match s.to_ascii_lowercase().as_str() {
        "call" => Ok("call"),
        "email" => Ok("email"),
        "meeting" => Ok("meeting"),
        "task" => Ok("task"),
        "note" => Ok("note"),
        _ => Err(AppError::Validation("Invalid activity_type".into())),
    }
}

pub async fn list_activities(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    perms: UserPermissions,
    Query(params): Query<ActivityFilter>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("activities.view")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    let scope = crate::auth::tenant_scope(&state, &claims.sub).await?;
    let pp = PaginationParams {
        page: params.page,
        per_page: params.per_page,
        search: params.search.clone(),
        sort_by: params.sort_by.clone(),
        sort_order: params.sort_order.clone(),
    };
    let per_page = pp.per_page();
    let offset = pp.offset();

    let activity_type = params
        .activity_type
        .as_deref()
        .map(normalize_activity_type)
        .transpose()?;

    let mut query = String::from(
        "SELECT id, activity_type, subject, description, contact_id, deal_id, company_id, due_date, completed, COALESCE(recurrence_type, 'none') AS recurrence_type, recurrence_interval, recurrence_end_date, parent_activity_id, created_at, updated_at FROM activities WHERE 1=1",
    );
    // Solo se interpolan índices `$n` (números generados aquí, no input).
    let mut param_idx = 1i32;

    if activity_type.is_some() {
        query.push_str(&format!(" AND activity_type::text = ${}", param_idx));
        param_idx += 1;
    }
    if params.contact_id.is_some() {
        query.push_str(&format!(" AND contact_id = ${}", param_idx));
        param_idx += 1;
    }
    if params.deal_id.is_some() {
        query.push_str(&format!(" AND deal_id = ${}", param_idx));
        param_idx += 1;
    }
    if params.company_id.is_some() {
        query.push_str(&format!(" AND company_id = ${}", param_idx));
        param_idx += 1;
    }
    query.push_str(&format!(
        " AND (${}::uuid IS NULL OR tenant_id = ${})",
        param_idx, param_idx
    ));
    param_idx += 1;

    query.push_str(&format!(
        " ORDER BY created_at DESC LIMIT ${} OFFSET ${}",
        param_idx,
        param_idx + 1
    ));

    let mut q = sqlx::query_as::<_, Activity>(&query);
    if let Some(at) = activity_type {
        q = q.bind(at);
    }
    if let Some(cid) = params.contact_id {
        q = q.bind(cid);
    }
    if let Some(did) = params.deal_id {
        q = q.bind(did);
    }
    if let Some(cmid) = params.company_id {
        q = q.bind(cmid);
    }
    q = q.bind(scope).bind(per_page).bind(offset);

    let activities = q.fetch_all(&state.db).await?;

    Ok(Json(activities))
}

pub async fn create_activity(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    perms: UserPermissions,
    Json(input): Json<CreateActivity>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("activities.create")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    input.validate()?;
    let scope = crate::auth::tenant_scope(&state, &claims.sub).await?;
    if let Some(cid) = input.contact_id {
        let visible: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM contacts WHERE id = $1 AND ($2::uuid IS NULL OR tenant_id = $2))",
        )
        .bind(cid)
        .bind(scope)
        .fetch_one(&state.db)
        .await?;
        if !visible {
            return Err(AppError::NotFound);
        }
    }
    if let Some(did) = input.deal_id {
        let visible: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM deals WHERE id = $1 AND ($2::uuid IS NULL OR tenant_id = $2))",
        )
        .bind(did)
        .bind(scope)
        .fetch_one(&state.db)
        .await?;
        if !visible {
            return Err(AppError::NotFound);
        }
    }
    if let Some(cmid) = input.company_id {
        let visible: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM companies WHERE id = $1 AND ($2::uuid IS NULL OR tenant_id = $2))",
        )
        .bind(cmid)
        .bind(scope)
        .fetch_one(&state.db)
        .await?;
        if !visible {
            return Err(AppError::NotFound);
        }
    }
    let activity = sqlx::query_as::<_, Activity>(
        r#"
        INSERT INTO activities (activity_type, subject, description, contact_id, deal_id, company_id, due_date, recurrence_type, recurrence_interval, recurrence_end_date, tenant_id)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        RETURNING id, activity_type, subject, description, contact_id, deal_id, company_id, due_date, completed, recurrence_type, recurrence_interval, recurrence_end_date, parent_activity_id, created_at, updated_at
        "#,
    )
    .bind(input.activity_type)
    .bind(&input.subject)
    .bind(&input.description)
    .bind(input.contact_id)
    .bind(input.deal_id)
    .bind(input.company_id)
    .bind(input.due_date)
    .bind(input.recurrence_type.unwrap_or(crate::models::RecurrenceType::None))
    .bind(input.recurrence_interval.unwrap_or(1))
    .bind(input.recurrence_end_date)
    .bind(scope)
    .fetch_one(&state.db)
    .await?;

    let _ = crate::handlers::audit::insert_audit_log(
        &state.db,
        Some(claims.sub),
        "create",
        "activity",
        activity.id,
        None,
        Some(serde_json::json!(activity)),
    )
    .await;
    Ok((StatusCode::CREATED, Json(activity)))
}

pub async fn update_activity(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateActivity>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("activities.edit")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    input.validate()?;
    let activity = sqlx::query_as::<_, Activity>(
        r#"
        UPDATE activities
        SET activity_type = COALESCE($2, activity_type),
            subject = COALESCE($3, subject),
            description = COALESCE($4, description),
            contact_id = COALESCE($5, contact_id),
            deal_id = COALESCE($6, deal_id),
            company_id = COALESCE($7, company_id),
            due_date = COALESCE($8, due_date),
            completed = COALESCE($9, completed),
            recurrence_type = COALESCE($10, recurrence_type),
            recurrence_interval = COALESCE($11, recurrence_interval),
            recurrence_end_date = COALESCE($12, recurrence_end_date),
            updated_at = NOW()
        WHERE id = $1 AND ($13::uuid IS NULL OR tenant_id = $13)
        RETURNING id, activity_type, subject, description, contact_id, deal_id, company_id, due_date, completed, recurrence_type, recurrence_interval, recurrence_end_date, parent_activity_id, created_at, updated_at
        "#,
    )
    .bind(id)
    .bind(input.activity_type)
    .bind(&input.subject)
    .bind(&input.description)
    .bind(input.contact_id)
    .bind(input.deal_id)
    .bind(input.company_id)
    .bind(input.due_date)
    .bind(input.completed)
    .bind(input.recurrence_type)
    .bind(input.recurrence_interval)
    .bind(input.recurrence_end_date)
    .bind(crate::auth::tenant_scope(&state, &claims.sub).await?)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let _ = crate::handlers::audit::insert_audit_log(
        &state.db,
        Some(claims.sub),
        "update",
        "activity",
        id,
        None,
        Some(serde_json::json!(activity)),
    )
    .await;
    Ok(Json(activity))
}

pub async fn delete_activity(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("activities.delete")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    let scope = crate::auth::tenant_scope(&state, &claims.sub).await?;
    let result = sqlx::query(
        "DELETE FROM activities WHERE id = $1 AND ($2::uuid IS NULL OR tenant_id = $2)",
    )
    .bind(id)
    .bind(scope)
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    let _ = crate::handlers::audit::insert_audit_log(
        &state.db,
        Some(claims.sub),
        "delete",
        "activity",
        id,
        None,
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn complete_activity(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("activities.edit")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    let activity = sqlx::query_as::<_, Activity>(
        r#"
        UPDATE activities
        SET completed = true, updated_at = NOW()
        WHERE id = $1 AND ($2::uuid IS NULL OR tenant_id = $2)
        RETURNING id, activity_type, subject, description, contact_id, deal_id, company_id, due_date, completed, recurrence_type, recurrence_interval, recurrence_end_date, parent_activity_id, created_at, updated_at
        "#,
    )
    .bind(id)
    .bind(crate::auth::tenant_scope(&state, &claims.sub).await?)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    let _ = crate::handlers::audit::insert_audit_log(
        &state.db,
        Some(claims.sub),
        "update",
        "activity",
        id,
        None,
        Some(serde_json::json!(activity)),
    )
    .await;
    Ok(Json(activity))
}
