//! Endpoints de leads: CRUD, conversión a contacto/empresa/negocio,
//! actividades y estadísticas.
//!
//! Seguridad: solo-autenticado (`Extension<Claims>`); valores siempre con
//! binds, ORDER BY de lista blanca y LIKE con escape.

use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{
    auth::Claims,
    error::AppError,
    handlers::audit::insert_audit_log,
    middleware::auth_ext::UserPermissions,
    models::{
        escape_like,
        lead::{
            ConvertLead, ConvertLeadResult, CreateLead, CreateLeadActivity, Lead, LeadActivity,
            LeadSource, LeadStatus, UpdateLead,
        },
    },
    state::AppState,
};

const LEAD_COLUMNS: &str = "id, first_name, last_name, email, phone, company_name, title, \
    industry, website, lead_source, status, score, assigned_to, converted_at, \
    converted_contact_id, converted_company_id, converted_deal_id, notes, created_at, updated_at";

#[derive(Debug, Deserialize, Validate)]
pub struct LeadQuery {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
    #[validate(length(max = 200, message = "search max 200 caracteres"))]
    pub search: Option<String>,
    pub status: Option<LeadStatus>,
    pub lead_source: Option<LeadSource>,
    pub assigned_to: Option<Uuid>,
    #[validate(length(max = 20))]
    pub sort: Option<String>,
    #[validate(length(max = 4))]
    pub sort_dir: Option<String>,
}

impl LeadQuery {
    fn page_clamped(&self) -> i64 {
        self.page.unwrap_or(1).clamp(1, 10_000)
    }
    fn per_page_clamped(&self) -> i64 {
        self.per_page.unwrap_or(20).clamp(1, 100)
    }
    fn offset(&self) -> i64 {
        (self.page_clamped() - 1) * self.per_page_clamped()
    }
    /// Solo columnas de la lista blanca (el `format!` nunca recibe input).
    fn sort_column(&self) -> &'static str {
        match self.sort.as_deref() {
            Some("name") => "first_name",
            Some("company") => "company_name",
            Some("score") => "score",
            Some("source") => "lead_source",
            Some("created") => "created_at",
            _ => "created_at",
        }
    }
    fn sort_direction(&self) -> &'static str {
        match self.sort_dir.as_deref() {
            Some("asc") => "ASC",
            _ => "DESC",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PaginatedLeads {
    pub data: Vec<Lead>,
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
    pub total_pages: i64,
}

pub async fn list_leads(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(_claims): Extension<Claims>,
    Query(params): Query<LeadQuery>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("leads.view")
        .map_err(|_| AppError::Forbidden)?;
    params.validate()?;
    let page = params.page_clamped();
    let per_page = params.per_page_clamped();
    let offset = params.offset();
    let sort_col = params.sort_column();
    let sort_dir = params.sort_direction();

    let pattern = params
        .search
        .as_deref()
        .map(|s| format!("%{}%", escape_like(s)));
    let status = params.status.as_ref().map(|s| s.as_str().to_string());
    let source = params.lead_source.as_ref().map(|s| s.as_str().to_string());
    let assigned = params.assigned_to;

    let count_sql = "SELECT COUNT(*) FROM leads \
        WHERE ($1::text IS NULL \
            OR first_name ILIKE $1 ESCAPE '\\' OR last_name ILIKE $1 ESCAPE '\\' \
            OR email ILIKE $1 ESCAPE '\\' OR company_name ILIKE $1 ESCAPE '\\') \
        AND ($2::lead_status IS NULL OR status = $2::lead_status) \
        AND ($3::lead_source IS NULL OR lead_source = $3::lead_source) \
        AND ($4::uuid IS NULL OR assigned_to = $4)";
    // sort_col/sort_dir son constantes de la lista blanca.
    let data_sql = format!(
        "SELECT {LEAD_COLUMNS} FROM leads \
        WHERE ($1::text IS NULL \
            OR first_name ILIKE $1 ESCAPE '\\' OR last_name ILIKE $1 ESCAPE '\\' \
            OR email ILIKE $1 ESCAPE '\\' OR company_name ILIKE $1 ESCAPE '\\') \
        AND ($2::lead_status IS NULL OR status = $2::lead_status) \
        AND ($3::lead_source IS NULL OR lead_source = $3::lead_source) \
        AND ($4::uuid IS NULL OR assigned_to = $4) \
        ORDER BY {sort_col} {sort_dir} LIMIT $5 OFFSET $6"
    );

    let total: i64 = sqlx::query_scalar(count_sql)
        .bind(pattern.as_deref())
        .bind(status.as_deref())
        .bind(source.as_deref())
        .bind(assigned)
        .fetch_one(&state.db)
        .await?;

    let leads = sqlx::query_as::<_, Lead>(&data_sql)
        .bind(pattern.as_deref())
        .bind(status.as_deref())
        .bind(source.as_deref())
        .bind(assigned)
        .bind(per_page)
        .bind(offset)
        .fetch_all(&state.db)
        .await?;

    let total_pages = if per_page > 0 {
        (total + per_page - 1) / per_page
    } else {
        1
    };
    Ok(Json(PaginatedLeads {
        data: leads,
        total,
        page,
        per_page,
        total_pages,
    }))
}

pub async fn get_lead(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(_claims): Extension<Claims>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("leads.view")
        .map_err(|_| AppError::Forbidden)?;
    let sql = format!("SELECT {LEAD_COLUMNS} FROM leads WHERE id = $1");
    let lead = sqlx::query_as::<_, Lead>(&sql)
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(lead))
}

pub async fn create_lead(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(claims): Extension<Claims>,
    Json(input): Json<CreateLead>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("leads.create")
        .map_err(|_| AppError::Forbidden)?;
    input.validate()?;
    let lead = sqlx::query_as::<_, Lead>(
        "INSERT INTO leads (first_name, last_name, email, phone, company_name, title, \
            industry, website, lead_source, assigned_to, notes) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::lead_source, $10, $11) \
         RETURNING id, first_name, last_name, email, phone, company_name, title, industry, \
            website, lead_source, status, score, assigned_to, converted_at, \
            converted_contact_id, converted_company_id, converted_deal_id, notes, \
            created_at, updated_at",
    )
    .bind(&input.first_name)
    .bind(&input.last_name)
    .bind(input.email.as_deref())
    .bind(input.phone.as_deref())
    .bind(input.company_name.as_deref())
    .bind(input.title.as_deref())
    .bind(input.industry.as_deref())
    .bind(input.website.as_deref())
    .bind(
        input
            .lead_source
            .as_ref()
            .map(|s| s.as_str())
            .unwrap_or("other"),
    )
    .bind(input.assigned_to)
    .bind(input.notes.as_deref())
    .fetch_one(&state.db)
    .await?;

    let _ = insert_audit_log(
        &state.db,
        Some(claims.sub),
        "create",
        "lead",
        lead.id,
        None,
        Some(serde_json::json!(lead)),
    )
    .await;

    Ok((StatusCode::CREATED, Json(lead)))
}

pub async fn update_lead(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(claims): Extension<Claims>,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateLead>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("leads.edit")
        .map_err(|_| AppError::Forbidden)?;
    input.validate()?;

    let existing_sql = format!("SELECT {LEAD_COLUMNS} FROM leads WHERE id = $1");
    let existing = sqlx::query_as::<_, Lead>(&existing_sql)
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    if let Some(ref new_status) = input.status {
        let allowed = matches!(
            (&existing.status, new_status),
            (LeadStatus::New, LeadStatus::Contacted)
                | (LeadStatus::Contacted, LeadStatus::Qualified)
                | (_, LeadStatus::Unqualified)
                | (LeadStatus::Unqualified, LeadStatus::Recycled)
        );
        if !allowed {
            return Err(AppError::Validation("transición de estado inválida".into()));
        }
    }

    let lead = sqlx::query_as::<_, Lead>(
        "UPDATE leads SET \
            first_name = COALESCE($2, first_name), \
            last_name = COALESCE($3, last_name), \
            email = COALESCE($4, email), \
            phone = COALESCE($5, phone), \
            company_name = COALESCE($6, company_name), \
            title = COALESCE($7, title), \
            industry = COALESCE($8, industry), \
            website = COALESCE($9, website), \
            lead_source = COALESCE($10::lead_source, lead_source), \
            status = COALESCE($11::lead_status, status), \
            score = COALESCE($12, score), \
            assigned_to = COALESCE($13, assigned_to), \
            notes = COALESCE($14, notes), \
            updated_at = NOW() \
         WHERE id = $1 \
         RETURNING id, first_name, last_name, email, phone, company_name, title, industry, \
            website, lead_source, status, score, assigned_to, converted_at, \
            converted_contact_id, converted_company_id, converted_deal_id, notes, \
            created_at, updated_at",
    )
    .bind(id)
    .bind(input.first_name.as_deref())
    .bind(input.last_name.as_deref())
    .bind(input.email.as_deref())
    .bind(input.phone.as_deref())
    .bind(input.company_name.as_deref())
    .bind(input.title.as_deref())
    .bind(input.industry.as_deref())
    .bind(input.website.as_deref())
    .bind(input.lead_source.as_ref().map(|s| s.as_str()))
    .bind(input.status.as_ref().map(|s| s.as_str()))
    .bind(input.score)
    .bind(input.assigned_to)
    .bind(input.notes.as_deref())
    .fetch_one(&state.db)
    .await?;

    let _ = insert_audit_log(
        &state.db,
        Some(claims.sub),
        "update",
        "lead",
        id,
        Some(serde_json::json!(existing)),
        Some(serde_json::json!(lead)),
    )
    .await;

    Ok(Json(lead))
}

pub async fn delete_lead(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(claims): Extension<Claims>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("leads.delete")
        .map_err(|_| AppError::Forbidden)?;
    let result = sqlx::query("DELETE FROM leads WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    let _ = insert_audit_log(
        &state.db,
        Some(claims.sub),
        "delete",
        "lead",
        id,
        None,
        None,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn convert_lead(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(claims): Extension<Claims>,
    Path(id): Path<Uuid>,
    Json(input): Json<ConvertLead>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("leads.convert")
        .map_err(|_| AppError::Forbidden)?;
    input.validate()?;
    let mut tx = state.db.begin().await?;

    // Bloquea la fila para evitar doble conversión concurrente.
    let lock_sql = format!("SELECT {LEAD_COLUMNS} FROM leads WHERE id = $1 FOR UPDATE");
    let lead = sqlx::query_as::<_, Lead>(&lock_sql)
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AppError::NotFound)?;

    if lead.status != LeadStatus::Qualified {
        return Err(AppError::Validation(
            "el lead debe estar calificado para convertir".into(),
        ));
    }
    if lead.converted_at.is_some() {
        return Err(AppError::Conflict);
    }

    let pipeline: Option<(String,)> =
        sqlx::query_as("SELECT entity_type FROM pipelines WHERE id = $1")
            .bind(input.pipeline_id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some((entity_type,)) = pipeline else {
        return Err(AppError::Validation("pipeline inválido".into()));
    };
    let is_company = entity_type == "company";

    let default_stage: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM pipeline_stages WHERE pipeline_id = $1 AND is_default = true LIMIT 1",
    )
    .bind(input.pipeline_id)
    .fetch_optional(&mut *tx)
    .await?;

    // Empresa solo si persona jurídica; se usa id escalar para no depender de
    // modelos de otros grupos.
    let mut company_id: Option<Uuid> = None;
    if is_company {
        let cid: Uuid = sqlx::query_scalar(
            "INSERT INTO companies (name, industry, website, phone, email) \
             VALUES ($1, $2, $3, $4, $5) RETURNING id",
        )
        .bind(lead.company_name.as_deref().unwrap_or("Unknown"))
        .bind(lead.industry.as_deref())
        .bind(lead.website.as_deref())
        .bind(lead.phone.as_deref())
        .bind(lead.email.as_deref())
        .fetch_one(&mut *tx)
        .await?;
        company_id = Some(cid);
    }

    let last_name = lead
        .last_name
        .split_whitespace()
        .next()
        .unwrap_or(lead.last_name.as_str());
    let contact_id: Uuid = sqlx::query_scalar(
        "INSERT INTO contacts (first_name, last_name, email, phone, company_id, position) \
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
    )
    .bind(&lead.first_name)
    .bind(last_name)
    .bind(lead.email.as_deref())
    .bind(lead.phone.as_deref())
    .bind(company_id)
    .bind(lead.title.as_deref())
    .fetch_one(&mut *tx)
    .await?;

    let deal_title = input.deal_title.clone().unwrap_or_else(|| {
        format!(
            "{} {} - {}",
            lead.first_name,
            lead.last_name,
            lead.company_name.as_deref().unwrap_or("Lead")
        )
    });
    // TODO: columna NUMERIC; se bindea f64 (el origen usaba Decimal).
    let deal_id: Uuid = sqlx::query_scalar(
        "INSERT INTO deals (title, value, stage, contact_id, company_id, pipeline_id, \
            pipeline_stage_id, notes) \
         VALUES ($1, $2, 'qualified', $3, $4, $5, $6, $7) RETURNING id",
    )
    .bind(&deal_title)
    .bind(input.deal_value.unwrap_or(0.0))
    .bind(contact_id)
    .bind(company_id)
    .bind(input.pipeline_id)
    .bind(default_stage.map(|s| s.0))
    .bind(format!("Converted from lead: {id}"))
    .fetch_one(&mut *tx)
    .await?;

    // Condicional anti-carrera: solo la primera transacción concurrente gana.
    let updated: Option<(Uuid,)> = sqlx::query_as(
        "UPDATE leads SET status = 'converted', converted_at = NOW(), \
            converted_contact_id = $2, converted_company_id = $3, converted_deal_id = $4, \
            updated_at = NOW() \
         WHERE id = $1 AND status = 'qualified' AND converted_at IS NULL RETURNING id",
    )
    .bind(id)
    .bind(contact_id)
    .bind(company_id)
    .bind(deal_id)
    .fetch_optional(&mut *tx)
    .await?;
    if updated.is_none() {
        return Err(AppError::Conflict);
    }

    tx.commit().await?;

    let _ = insert_audit_log(
        &state.db,
        Some(claims.sub),
        "convert",
        "lead",
        id,
        Some(serde_json::json!(lead)),
        Some(serde_json::json!({
            "contact_id": contact_id,
            "company_id": company_id,
            "deal_id": deal_id,
        })),
    )
    .await;

    Ok(Json(ConvertLeadResult {
        contact_id: Some(contact_id),
        company_id,
        deal_id: Some(deal_id),
    }))
}

pub async fn list_lead_activities(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(_claims): Extension<Claims>,
    Path(lead_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("leads.view")
        .map_err(|_| AppError::Forbidden)?;
    let activities = sqlx::query_as::<_, LeadActivity>(
        "SELECT id, lead_id, type, subject, description, due_date, completed, created_by, \
            created_at FROM lead_activities WHERE lead_id = $1 ORDER BY created_at DESC",
    )
    .bind(lead_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(activities))
}

pub async fn create_lead_activity(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(claims): Extension<Claims>,
    Path(lead_id): Path<Uuid>,
    Json(input): Json<CreateLeadActivity>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("leads.create")
        .map_err(|_| AppError::Forbidden)?;
    input.validate()?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM leads WHERE id = $1)")
        .bind(lead_id)
        .fetch_one(&state.db)
        .await?;
    if !exists {
        return Err(AppError::NotFound);
    }
    let activity = sqlx::query_as::<_, LeadActivity>(
        "INSERT INTO lead_activities (lead_id, type, subject, description, due_date, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         RETURNING id, lead_id, type, subject, description, due_date, completed, created_by, \
            created_at",
    )
    .bind(lead_id)
    .bind(&input.r#type)
    .bind(&input.subject)
    .bind(input.description.as_deref())
    .bind(input.due_date)
    .bind(claims.sub)
    .fetch_one(&state.db)
    .await?;
    Ok((StatusCode::CREATED, Json(activity)))
}

pub async fn complete_lead_activity(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(_claims): Extension<Claims>,
    Path((lead_id, activity_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("leads.edit")
        .map_err(|_| AppError::Forbidden)?;
    let result =
        sqlx::query("UPDATE lead_activities SET completed = TRUE WHERE id = $1 AND lead_id = $2")
            .bind(activity_id)
            .bind(lead_id)
            .execute(&state.db)
            .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::OK)
}

pub async fn delete_lead_activity(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(_claims): Extension<Claims>,
    Path((lead_id, activity_id)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("leads.delete")
        .map_err(|_| AppError::Forbidden)?;
    let result = sqlx::query("DELETE FROM lead_activities WHERE id = $1 AND lead_id = $2")
        .bind(activity_id)
        .bind(lead_id)
        .execute(&state.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Serialize)]
pub struct LeadStats {
    pub total: i64,
    pub new: i64,
    pub contacted: i64,
    pub qualified: i64,
    pub converted: i64,
    pub conversion_rate: f64,
    pub by_source: Vec<SourceCount>,
}

#[derive(Debug, Serialize)]
pub struct SourceCount {
    pub source: String,
    pub count: i64,
}

pub async fn lead_stats(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(_claims): Extension<Claims>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("leads.view")
        .map_err(|_| AppError::Forbidden)?;
    #[derive(sqlx::FromRow)]
    struct StatsRow {
        total: i64,
        new: i64,
        contacted: i64,
        qualified: i64,
        converted: i64,
    }
    // Un solo COUNT con FILTER en vez de N consultas secuenciales.
    let counts_fut = sqlx::query_as::<_, StatsRow>(
        "SELECT COUNT(*) AS total, \
            COUNT(*) FILTER (WHERE status = 'new') AS new, \
            COUNT(*) FILTER (WHERE status = 'contacted') AS contacted, \
            COUNT(*) FILTER (WHERE status = 'qualified') AS qualified, \
            COUNT(*) FILTER (WHERE status = 'converted') AS converted \
         FROM leads",
    )
    .fetch_one(&state.db);
    let by_source_fut = sqlx::query_as::<_, (String, i64)>(
        "SELECT lead_source::text, COUNT(*) FROM leads \
         GROUP BY lead_source ORDER BY COUNT(*) DESC",
    )
    .fetch_all(&state.db);

    let (counts, by_source_rows) = tokio::join!(counts_fut, by_source_fut);
    let counts = counts?;
    let by_source_rows = by_source_rows?;

    let conversion_rate = if counts.total > 0 {
        (counts.converted as f64 / counts.total as f64) * 100.0
    } else {
        0.0
    };
    let by_source = by_source_rows
        .into_iter()
        .map(|(source, count)| SourceCount { source, count })
        .collect();

    Ok(Json(LeadStats {
        total: counts.total,
        new: counts.new,
        contacted: counts.contacted,
        qualified: counts.qualified,
        converted: counts.converted,
        conversion_rate,
        by_source,
    }))
}
