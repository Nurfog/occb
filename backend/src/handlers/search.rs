//! Búsqueda global (contactos + compañías + deals), tope 5 por entidad.
//!
//! Adaptación a la plantilla segura:
//! - Auth: JWT válido (`Extension<Claims>`) + RBAC fino
//!   (`search.view`) vía `UserPermissions`.
//! - `q` validado con `validator` (máx 200) + `escape_like` + `ESCAPE '\'`.
//! - `deals.value` casteado a `float8` (columna NUMERIC, sin `rust_decimal`).
//! - Las 3 queries corren en concurrente (`tokio::join!`).

use axum::{
    Extension, Json,
    extract::{Query, State},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::middleware::auth_ext::UserPermissions;
use crate::models::escape_like;
use crate::{auth::Claims, error::AppError, state::AppState};

#[derive(Debug, Deserialize, Validate)]
pub struct SearchParams {
    #[validate(length(max = 200, message = "Search query must be at most 200 characters"))]
    pub q: String,
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub id: String,
    #[serde(rename = "entity_type")]
    pub entity_type: String,
    pub label: String,
    pub subtitle: String,
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub contacts: Vec<SearchResult>,
    pub companies: Vec<SearchResult>,
    pub deals: Vec<SearchResult>,
}

pub async fn global_search(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    perms: UserPermissions,
    Query(params): Query<SearchParams>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("search.view")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    params.validate()?;
    let scope = crate::auth::tenant_scope(&state, &claims.sub).await?;
    let query = params.q.trim().to_string();
    if query.is_empty() {
        return Ok(Json(SearchResponse {
            contacts: vec![],
            companies: vec![],
            deals: vec![],
        }));
    }

    let search_pattern = format!("%{}%", escape_like(&query));
    let limit: i64 = 5;

    let contacts_fut = sqlx::query_as::<_, (uuid::Uuid, String, String, Option<String>)>(
        r#"
        SELECT id, first_name, last_name, email
        FROM contacts
        WHERE (first_name ILIKE $1 ESCAPE '\'
           OR last_name ILIKE $1 ESCAPE '\'
           OR email ILIKE $1 ESCAPE '\')
          -- Ley 21.719 (bloqueo): no tratar datos bloqueados ni en búsqueda.
          AND (blocked IS NULL OR blocked = FALSE)
          AND ($2::uuid IS NULL OR tenant_id = $2)
        ORDER BY first_name ASC
        LIMIT $3
        "#,
    )
    .bind(&search_pattern)
    .bind(scope)
    .bind(limit)
    .fetch_all(&state.db);

    let companies_fut = sqlx::query_as::<_, (uuid::Uuid, String, Option<String>)>(
        r#"
        SELECT id, name, industry
        FROM companies
        WHERE (name ILIKE $1 ESCAPE '\'
           OR industry ILIKE $1 ESCAPE '\')
          AND ($2::uuid IS NULL OR tenant_id = $2)
        ORDER BY name ASC
        LIMIT $3
        "#,
    )
    .bind(&search_pattern)
    .bind(scope)
    .bind(limit)
    .fetch_all(&state.db);

    let deals_fut = sqlx::query_as::<_, (uuid::Uuid, String, f64, String)>(
        r#"
        SELECT id, title, value::float8, stage::text
        FROM deals
        WHERE title ILIKE $1 ESCAPE '\'
          AND ($2::uuid IS NULL OR tenant_id = $2)
        ORDER BY created_at DESC
        LIMIT $3
        "#,
    )
    .bind(&search_pattern)
    .bind(scope)
    .bind(limit)
    .fetch_all(&state.db);

    let (contacts_rows, companies_rows, deals_rows) =
        tokio::join!(contacts_fut, companies_fut, deals_fut);

    let contacts = contacts_rows?
        .into_iter()
        .map(|(id, first_name, last_name, email)| {
            let label = format!("{} {}", first_name, last_name);
            let subtitle = email.unwrap_or_default();
            SearchResult {
                id: id.to_string(),
                entity_type: "contact".to_string(),
                label,
                subtitle,
            }
        })
        .collect();

    let companies = companies_rows?
        .into_iter()
        .map(|(id, name, industry)| {
            let subtitle = industry.unwrap_or_default();
            SearchResult {
                id: id.to_string(),
                entity_type: "company".to_string(),
                label: name,
                subtitle,
            }
        })
        .collect();

    let deals = deals_rows?
        .into_iter()
        .map(|(id, title, value, stage)| {
            let subtitle = format!("${value:.0} - {stage}");
            SearchResult {
                id: id.to_string(),
                entity_type: "deal".to_string(),
                label: title,
                subtitle,
            }
        })
        .collect();

    Ok(Json(SearchResponse {
        contacts,
        companies,
        deals,
    }))
}
