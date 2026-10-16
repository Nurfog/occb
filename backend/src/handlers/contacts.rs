//! Handlers de contactos: CRUD + export/import CSV + borrado masivo.
//!
//! Adaptación a la plantilla segura:
//! - Auth: JWT válido (`Extension<Claims>`) + RBAC fino
//!   (`contacts.view/create/edit/delete`) vía `UserPermissions`.
//! - Errores origen `BadRequest` -> `AppError::Validation`.
//! - Side-effects de auditoría/webhooks omitidos (otros grupos).
//!   // TODO port: requiere audit (insert_audit_log) y webhooks.
//! - SQL 100% parametrizada en `repositories::contact_repo`; ORDER BY de
//!   lista blanca; LIKE con `escape_like`.

use axum::{
    Extension, Json,
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::middleware::auth_ext::UserPermissions;
use crate::models::{Contact, CreateContact, PaginatedResponse, PaginationParams, UpdateContact};
use crate::services::contact_service::ContactService;
use crate::{auth::Claims, error::AppError, models::ContactFilters, state::AppState};

#[derive(Debug, Deserialize, Validate)]
pub struct BulkDeleteRequest {
    #[validate(length(max = 100, message = "Cannot delete more than 100 contacts at once"))]
    pub ids: Vec<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct BulkDeleteResponse {
    pub deleted: usize,
}

pub async fn list_contacts(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    perms: UserPermissions,
    Query(params): Query<PaginationParams>,
    Query(filters): Query<ContactFilters>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("contacts.view")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    filters.validate()?;
    let svc = ContactService::new(state.db.clone());
    let page: PaginatedResponse<Contact> = svc.list(&params, &filters).await?;
    Ok(Json(page))
}

pub async fn create_contact(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    perms: UserPermissions,
    Json(input): Json<CreateContact>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("contacts.create")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    input.validate()?;
    let svc = ContactService::new(state.db.clone());
    let contact = svc.create(&input).await?;
    let _ = crate::services::webhook_worker::enqueue_event(
        &state.db,
        "contact_created",
        serde_json::json!({ "id": contact.id }),
    )
    .await;
    Ok((StatusCode::CREATED, Json(contact)))
}

pub async fn get_contact(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("contacts.view")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    let svc = ContactService::new(state.db.clone());
    Ok(Json(svc.get(id).await?))
}

pub async fn update_contact(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateContact>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("contacts.edit")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    input.validate()?;
    let svc = ContactService::new(state.db.clone());
    let contact = svc.update(id, &input).await?;
    let _ = crate::services::webhook_worker::enqueue_event(
        &state.db,
        "contact_updated",
        serde_json::json!({ "id": id }),
    )
    .await;
    Ok(Json(contact))
}

pub async fn delete_contact(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("contacts.delete")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    let svc = ContactService::new(state.db.clone());
    svc.delete(id).await?;
    let _ = crate::services::webhook_worker::enqueue_event(
        &state.db,
        "contact_deleted",
        serde_json::json!({ "id": id }),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn export_contacts(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    perms: UserPermissions,
    Query(params): Query<PaginationParams>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("contacts.view")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    let svc = ContactService::new(state.db.clone());
    let csv = svc.export(params.search.as_deref()).await?;

    let mut headers = HeaderMap::new();
    headers.insert(
        "Content-Type",
        HeaderValue::from_static("text/csv; charset=utf-8"),
    );
    headers.insert(
        "Content-Disposition",
        HeaderValue::from_static("attachment; filename=\"contacts.csv\""),
    );

    Ok((headers, csv))
}

pub async fn bulk_delete_contacts(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    perms: UserPermissions,
    Json(input): Json<BulkDeleteRequest>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("contacts.delete")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    input.validate()?;
    let svc = ContactService::new(state.db.clone());
    let deleted = svc.bulk_delete(&input.ids).await?;
    Ok(Json(BulkDeleteResponse { deleted }))
}

pub async fn import_contacts(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    perms: UserPermissions,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("contacts.create")
        .map_err(|_| crate::error::AppError::Forbidden)?;
    // Bytes (no String): acepta cualquier content-type (el cliente JS manda
    // text/csv); rechaza no-UTF8 explícitamente en vez del 415 de axum.
    let body = String::from_utf8(body.to_vec())
        .map_err(|_| AppError::Validation("Import body must be UTF-8 text".into()))?;
    // Tope 2MB aquí también (defensa en profundidad; la capa global de
    // límite de cuerpo lo impone igualmente).
    if body.len() > 2 * 1024 * 1024 {
        return Err(AppError::Validation(
            "Import body too large (max 2MB)".into(),
        ));
    }

    let svc = ContactService::new(state.db.clone());
    let result = svc.import(&body).await?;
    Ok(Json(result))
}
