//! Handlers de compañías: CRUD + export/import CSV.
//!
//! Adaptación a la plantilla segura:
//! - Auth: JWT válido (`Extension<Claims>`) + RBAC fino
//!   (`companies.view/create/edit/delete`) vía `UserPermissions`.
//! - `BadRequest` origen -> `AppError::Validation`; `Conflict`/`Internal`
//!   destino son unitarios (sin mensaje al cliente).
//! - Auditoría omitida (otro grupo).
//!   // TODO port: requiere audit (insert_audit_log created/updated/deleted).
//! - Orden con lista blanca propia (`normalize_company_sort`); el origen
//!   reutilizaba la de contactos (`ORDER BY first_name` inexistente aquí).

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
use crate::models::{Company, CreateCompany, PaginatedResponse, PaginationParams, UpdateCompany};
use crate::models::{ImportResult, escape_csv, escape_like, parse_csv_rows};
use crate::repositories::company_repo::normalize_company_sort;
use crate::{auth::Claims, error::AppError, state::AppState};

pub async fn list_companies(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    perms: UserPermissions,
    Query(params): Query<PaginationParams>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("companies.view")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    let page = params.page();
    let per_page = params.per_page();
    let offset = params.offset();
    let (sort_col, sort_dir) =
        normalize_company_sort(params.sort_by.as_deref(), params.sort_order.as_deref());

    let search_filter = params
        .search
        .as_ref()
        .map(|s| format!("%{}%", escape_like(s)));

    let count_query = if search_filter.is_some() {
        "SELECT COUNT(*) FROM companies WHERE name ILIKE $1 ESCAPE '\\' OR industry ILIKE $1 ESCAPE '\\' OR city ILIKE $1 ESCAPE '\\' OR country ILIKE $1 ESCAPE '\\'".to_string()
    } else {
        "SELECT COUNT(*) FROM companies".to_string()
    };

    // sort_col/sort_dir son constantes de lista blanca: interpolar es seguro.
    let data_query = if search_filter.is_some() {
        format!(
            "SELECT id, name, industry, website, phone, email, address, city, country, notes, created_at, updated_at
             FROM companies
             WHERE name ILIKE $1 ESCAPE '\\' OR industry ILIKE $1 ESCAPE '\\' OR city ILIKE $1 ESCAPE '\\' OR country ILIKE $1 ESCAPE '\\'
             ORDER BY {} {}
             LIMIT $2 OFFSET $3",
            sort_col, sort_dir
        )
    } else {
        format!(
            "SELECT id, name, industry, website, phone, email, address, city, country, notes, created_at, updated_at
             FROM companies
             ORDER BY {} {}
             LIMIT $1 OFFSET $2",
            sort_col, sort_dir
        )
    };

    let total: (i64,) = if let Some(ref search) = search_filter {
        sqlx::query_as(&count_query)
            .bind(search)
            .fetch_one(&state.db)
            .await?
    } else {
        sqlx::query_as(&count_query).fetch_one(&state.db).await?
    };

    let companies = if let Some(ref search) = search_filter {
        sqlx::query_as::<_, Company>(&data_query)
            .bind(search)
            .bind(per_page)
            .bind(offset)
            .fetch_all(&state.db)
            .await?
    } else {
        sqlx::query_as::<_, Company>(&data_query)
            .bind(per_page)
            .bind(offset)
            .fetch_all(&state.db)
            .await?
    };

    Ok(Json(PaginatedResponse::new(
        companies, total.0, page, per_page,
    )))
}

pub async fn create_company(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    perms: UserPermissions,
    Json(input): Json<CreateCompany>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("companies.create")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    input.validate()?;

    let company = sqlx::query_as::<_, Company>(
        r#"
        INSERT INTO companies (name, industry, website, phone, email, address, city, country, notes)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        RETURNING id, name, industry, website, phone, email, address, city, country, notes, created_at, updated_at
        "#,
    )
    .bind(&input.name)
    .bind(&input.industry)
    .bind(&input.website)
    .bind(&input.phone)
    .bind(&input.email)
    .bind(&input.address)
    .bind(&input.city)
    .bind(&input.country)
    .bind(&input.notes)
    .fetch_one(&state.db)
    .await?;

    // TODO port: requiere audit (insert_audit_log "created"/"company").
    Ok((StatusCode::CREATED, Json(company)))
}

pub async fn get_company(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("companies.view")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    let company = sqlx::query_as::<_, Company>(
        "SELECT id, name, industry, website, phone, email, address, city, country, notes, created_at, updated_at FROM companies WHERE id = $1"
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    Ok(Json(company))
}

pub async fn update_company(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateCompany>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("companies.edit")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    input.validate()?;

    let company = sqlx::query_as::<_, Company>(
        r#"
        UPDATE companies
        SET name = COALESCE($2, name),
            industry = COALESCE($3, industry),
            website = COALESCE($4, website),
            phone = COALESCE($5, phone),
            email = COALESCE($6, email),
            address = COALESCE($7, address),
            city = COALESCE($8, city),
            country = COALESCE($9, country),
            notes = COALESCE($10, notes),
            updated_at = NOW()
        WHERE id = $1
        RETURNING id, name, industry, website, phone, email, address, city, country, notes, created_at, updated_at
        "#,
    )
    .bind(id)
    .bind(&input.name)
    .bind(&input.industry)
    .bind(&input.website)
    .bind(&input.phone)
    .bind(&input.email)
    .bind(&input.address)
    .bind(&input.city)
    .bind(&input.country)
    .bind(&input.notes)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    // TODO port: requiere audit ("updated" con old/new).
    Ok(Json(company))
}

pub async fn delete_company(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("companies.delete")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    let result = sqlx::query("DELETE FROM companies WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    // TODO port: requiere audit ("deleted" con snapshot previo).
    Ok(StatusCode::NO_CONTENT)
}

pub async fn export_companies(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    perms: UserPermissions,
    Query(params): Query<PaginationParams>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("companies.view")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    let search_filter = params
        .search
        .as_ref()
        .map(|s| format!("%{}%", escape_like(s)));

    // TODO: stremear el CSV en vez de cargar todo en memoria.
    // Tope 10k filas para acotar memoria/tiempo.
    let companies = if let Some(ref search) = search_filter {
        sqlx::query_as::<_, Company>(
            "SELECT id, name, industry, website, phone, email, address, city, country, notes, created_at, updated_at FROM companies WHERE name ILIKE $1 ESCAPE '\\' OR industry ILIKE $1 ESCAPE '\\' OR city ILIKE $1 ESCAPE '\\' OR country ILIKE $1 ESCAPE '\\' ORDER BY created_at DESC LIMIT 10000"
        )
        .bind(search)
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as::<_, Company>(
            "SELECT id, name, industry, website, phone, email, address, city, country, notes, created_at, updated_at FROM companies ORDER BY created_at DESC LIMIT 10000"
        )
        .fetch_all(&state.db)
        .await?
    };

    let mut csv = String::from("name,industry,website,phone,email,address,city,country,notes\n");
    for c in &companies {
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{},{}\n",
            escape_csv(&c.name),
            escape_csv(c.industry.as_deref().unwrap_or("")),
            escape_csv(c.website.as_deref().unwrap_or("")),
            escape_csv(c.phone.as_deref().unwrap_or("")),
            escape_csv(c.email.as_deref().unwrap_or("")),
            escape_csv(c.address.as_deref().unwrap_or("")),
            escape_csv(c.city.as_deref().unwrap_or("")),
            escape_csv(c.country.as_deref().unwrap_or("")),
            escape_csv(c.notes.as_deref().unwrap_or(""))
        ));
    }

    let mut headers = HeaderMap::new();
    headers.insert(
        "Content-Type",
        HeaderValue::from_static("text/csv; charset=utf-8"),
    );
    headers.insert(
        "Content-Disposition",
        HeaderValue::from_static("attachment; filename=\"companies.csv\""),
    );

    Ok((headers, csv))
}

pub async fn import_companies(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    perms: UserPermissions,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("companies.create")
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

    for (row_num, fields) in rows.iter().enumerate() {
        if fields.is_empty() {
            errors.push(format!("Línea {}: formato inválido", row_num + 2));
            continue;
        }

        // Validar con las mismas reglas del create individual.
        let candidate = CreateCompany {
            name: fields[0].trim().to_string(),
            industry: opt_field(fields, 1),
            website: opt_field(fields, 2),
            phone: opt_field(fields, 3),
            email: opt_field(fields, 4),
            address: opt_field(fields, 5),
            city: opt_field(fields, 6),
            country: opt_field(fields, 7),
            notes: opt_field(fields, 8),
        };
        if let Err(e) = candidate.validate() {
            errors.push(format!(
                "Línea {}: {}",
                row_num + 2,
                first_validation_message(&e)
            ));
            continue;
        }

        let result = sqlx::query_as::<_, Company>(
            r#"
            INSERT INTO companies (name, industry, website, phone, email, address, city, country, notes)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING id, name, industry, website, phone, email, address, city, country, notes, created_at, updated_at
            "#,
        )
        .bind(&candidate.name)
        .bind(&candidate.industry)
        .bind(&candidate.website)
        .bind(&candidate.phone)
        .bind(&candidate.email)
        .bind(&candidate.address)
        .bind(&candidate.city)
        .bind(&candidate.country)
        .bind(&candidate.notes)
        .fetch_one(&state.db)
        .await;

        match result {
            Ok(_) => imported += 1,
            Err(e) => {
                // sqlx::Error -> AppError para no exponer detalle interno
                // (409 genérico en unicidad, 500 genérico en resto).
                let app_err: AppError = e.into();
                errors.push(format!("Línea {}: {}", row_num + 2, app_err));
            }
        }
    }

    Ok(Json(ImportResult { imported, errors }))
}

fn opt_field(fields: &[String], index: usize) -> Option<String> {
    fields
        .get(index)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn first_validation_message(e: &validator::ValidationErrors) -> String {
    e.field_errors()
        .values()
        .flat_map(|errs| errs.iter())
        .filter_map(|e| e.message.as_ref().map(|m| m.to_string()))
        .next()
        .unwrap_or_else(|| "validación inválida".to_string())
}
