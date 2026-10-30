//! Handlers de deals: CRUD + stage + export/import CSV.
//!
//! Adaptación a la plantilla segura:
//! - Auth: JWT válido (`Extension<Claims>`) + RBAC fino
//!   (`deals.view/create/edit/delete`) vía `UserPermissions`.
//! - Repo construido desde `state.db` (la plantilla no guarda repos en
//!   `AppState`; NO añadir campos).
//! - Auditoría best-effort incluida; webhooks omitidos (otro grupo).
//! - `value` es `f64` con cast `::float8` en el repo/queries.
//! - Orden con lista blanca local `normalize_deal_sort`.

use axum::{
    Extension, Json,
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
};
use uuid::Uuid;
use validator::Validate;

use crate::middleware::auth_ext::UserPermissions;
use crate::models::{
    CreateDeal, Deal, DealStage, PaginatedResponse, PaginationParams, UpdateDeal, UpdateDealStage,
};
use crate::models::{ImportResult, escape_csv, parse_csv_rows, parse_deal_import_row};
use crate::repositories::deal_repo::PgDealRepo;
use crate::{auth::Claims, error::AppError, state::AppState};

/// Lista blanca de columnas ordenables de deals (con alias `d.`).
fn normalize_deal_sort(
    sort_by: Option<&str>,
    sort_order: Option<&str>,
) -> (&'static str, &'static str) {
    let col = match sort_by {
        Some("title") => "d.title",
        Some("value") => "d.value",
        Some("stage") => "d.stage",
        Some("created_at") => "d.created_at",
        _ => "d.created_at",
    };
    let dir = match sort_order {
        Some("asc") => "ASC",
        _ => "DESC",
    };
    (col, dir)
}

pub async fn list_deals(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    perms: UserPermissions,
    Query(params): Query<PaginationParams>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("deals.view")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    let page = params.page();
    let per_page = params.per_page();
    let offset = params.offset();
    let (sort_col, sort_dir) =
        normalize_deal_sort(params.sort_by.as_deref(), params.sort_order.as_deref());

    let search_filter = params
        .search
        .as_ref()
        .map(|s| format!("%{}%", crate::models::escape_like(s)));

    let scope = crate::auth::tenant_scope(&state, &claims.sub).await?;
    let count_query = if search_filter.is_some() {
        format!(
            "{} AND ($2::uuid IS NULL OR tenant_id = $2)",
            crate::queries::deal_queries::DEAL_COUNT_SEARCH
        )
    } else {
        format!(
            "{} WHERE ($1::uuid IS NULL OR tenant_id = $1)",
            crate::queries::deal_queries::DEAL_COUNT
        )
    };

    let base_select = crate::queries::deal_queries::DEAL_SELECT;

    // sort_col/sort_dir son constantes de lista blanca.
    let data_query = if search_filter.is_some() {
        format!(
            "{base_select} WHERE d.title ILIKE $1 ESCAPE '\\' AND ($4::uuid IS NULL OR d.tenant_id = $4) ORDER BY {sort_col} {sort_dir} LIMIT $2 OFFSET $3"
        )
    } else {
        format!(
            "{base_select} WHERE ($3::uuid IS NULL OR d.tenant_id = $3) ORDER BY {sort_col} {sort_dir} LIMIT $1 OFFSET $2"
        )
    };

    let total: (i64,) = if let Some(ref search) = search_filter {
        sqlx::query_as(&count_query)
            .bind(search)
            .bind(scope)
            .fetch_one(&state.db)
            .await?
    } else {
        sqlx::query_as(&count_query)
            .bind(scope)
            .fetch_one(&state.db)
            .await?
    };

    let deals = if let Some(ref search) = search_filter {
        sqlx::query_as::<_, Deal>(&data_query)
            .bind(search)
            .bind(per_page)
            .bind(offset)
            .bind(scope)
            .fetch_all(&state.db)
            .await?
    } else {
        sqlx::query_as::<_, Deal>(&data_query)
            .bind(per_page)
            .bind(offset)
            .bind(scope)
            .fetch_all(&state.db)
            .await?
    };

    Ok(Json(PaginatedResponse::new(deals, total.0, page, per_page)))
}

pub async fn create_deal(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    perms: UserPermissions,
    Json(input): Json<CreateDeal>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("deals.create")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    input.validate()?;

    let stage = input.stage.unwrap_or(DealStage::Lead);
    let currency = input.currency.unwrap_or_else(|| "USD".to_string());

    let scope = crate::auth::tenant_scope(&state, &claims.sub).await?;
    let deal = PgDealRepo::new(state.db.clone())
        .create(
            &input.title,
            input.value,
            &currency,
            stage,
            input.contact_id,
            input.company_id,
            input.expected_close_date,
            input.notes.as_deref(),
            scope,
        )
        .await?;

    let _ = crate::services::webhook_worker::enqueue_event(
        &state.db,
        "deal_created",
        serde_json::json!({ "id": deal.id }),
    )
    .await;
    let _ = crate::handlers::audit::insert_audit_log(
        &state.db,
        Some(claims.sub),
        "create",
        "deal",
        deal.id,
        None,
        Some(serde_json::json!(deal)),
    )
    .await;
    Ok((StatusCode::CREATED, Json(deal)))
}

pub async fn get_deal(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("deals.view")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    let scope = crate::auth::tenant_scope(&state, &claims.sub).await?;
    let deal = PgDealRepo::new(state.db.clone())
        .find_by_id(id, scope)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(deal))
}

pub async fn update_deal(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateDeal>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("deals.edit")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    input.validate()?;

    let scope = crate::auth::tenant_scope(&state, &claims.sub).await?;
    let repo = PgDealRepo::new(state.db.clone());
    repo.find_by_id(id, scope)
        .await?
        .ok_or(AppError::NotFound)?;
    let deal = repo
        .update(id, &input, scope)
        .await?
        .ok_or(AppError::NotFound)?;

    let _ = crate::services::webhook_worker::enqueue_event(
        &state.db,
        "deal_updated",
        serde_json::json!({ "id": id }),
    )
    .await;
    let _ = crate::handlers::audit::insert_audit_log(
        &state.db,
        Some(claims.sub),
        "update",
        "deal",
        id,
        None,
        Some(serde_json::json!(deal)),
    )
    .await;
    Ok(Json(deal))
}

pub async fn delete_deal(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("deals.delete")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    let scope = crate::auth::tenant_scope(&state, &claims.sub).await?;
    let old = PgDealRepo::new(state.db.clone())
        .delete(id, scope)
        .await?
        .ok_or(AppError::NotFound)?;
    let _ = old;

    let _ = crate::services::webhook_worker::enqueue_event(
        &state.db,
        "deal_deleted",
        serde_json::json!({ "id": id }),
    )
    .await;
    let _ = crate::handlers::audit::insert_audit_log(
        &state.db,
        Some(claims.sub),
        "delete",
        "deal",
        id,
        None,
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn export_deals(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    perms: UserPermissions,
    Query(params): Query<PaginationParams>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("deals.view")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    let scope = crate::auth::tenant_scope(&state, &claims.sub).await?;
    let deals = PgDealRepo::new(state.db.clone())
        .find_all_for_export(params.search.as_deref(), scope)
        .await?;

    let mut csv = String::from(
        "title,value,currency,stage,contact_id,company_id,expected_close_date,notes\n",
    );
    for d in &deals {
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{}\n",
            escape_csv(&d.title),
            d.value,
            escape_csv(&d.currency),
            escape_csv(
                &serde_json::to_value(&d.stage)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_else(|| "lead".into())
            ),
            d.contact_id.map(|id| id.to_string()).unwrap_or_default(),
            d.company_id.map(|id| id.to_string()).unwrap_or_default(),
            d.expected_close_date
                .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                .unwrap_or_default(),
            escape_csv(d.notes.as_deref().unwrap_or(""))
        ));
    }

    let mut headers = HeaderMap::new();
    headers.insert(
        "Content-Type",
        HeaderValue::from_static("text/csv; charset=utf-8"),
    );
    headers.insert(
        "Content-Disposition",
        HeaderValue::from_static("attachment; filename=\"deals.csv\""),
    );

    Ok((headers, csv))
}

pub async fn import_deals(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    perms: UserPermissions,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("deals.create")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    let body = String::from_utf8(body.to_vec())
        .map_err(|_| AppError::Validation("Import body must be UTF-8 text".into()))?;
    if body.len() > 2 * 1024 * 1024 {
        return Err(AppError::Validation(
            "Import body too large (max 2MB)".into(),
        ));
    }
    let rows = parse_csv_rows(&body);
    if rows.len() > 1000 {
        return Err(AppError::Validation(
            "Too many rows (max 1000 per import)".into(),
        ));
    }

    let mut imported = 0;
    let mut errors = Vec::new();
    let scope = crate::auth::tenant_scope(&state, &claims.sub).await?;
    let repo = PgDealRepo::new(state.db.clone());

    for (row_num, fields) in rows.iter().enumerate() {
        let row = match parse_deal_import_row(fields) {
            Ok(row) => row,
            Err(message) => {
                errors.push(format!("Línea {}: {}", row_num + 2, message));
                continue;
            }
        };

        match repo
            .create(
                &row.title,
                row.value,
                &row.currency,
                row.stage,
                row.contact_id,
                row.company_id,
                row.expected_close_date,
                row.notes.as_deref(),
                scope,
            )
            .await
        {
            Ok(_) => imported += 1,
            Err(e) => errors.push(format!("Línea {}: {}", row_num + 2, e)),
        }
    }

    Ok(Json(ImportResult { imported, errors }))
}

pub async fn update_deal_stage(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateDealStage>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("deals.edit")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    let scope = crate::auth::tenant_scope(&state, &claims.sub).await?;
    let repo = PgDealRepo::new(state.db.clone());
    repo.find_by_id(id, scope)
        .await?
        .ok_or(AppError::NotFound)?;
    let deal = repo
        .update_stage(id, input.stage, input.position, scope)
        .await?
        .ok_or(AppError::NotFound)?;

    let _ = crate::services::webhook_worker::enqueue_event(
        &state.db,
        "deal_updated",
        serde_json::json!({ "id": id }),
    )
    .await;
    let _ = crate::handlers::audit::insert_audit_log(
        &state.db,
        Some(claims.sub),
        "update",
        "deal",
        id,
        None,
        Some(serde_json::json!(deal)),
    )
    .await;
    Ok(Json(deal))
}
