//! Endpoints de etiquetas: CRUD y asignación a entidades.
//!
//! Solo-autenticado (`Extension<Claims>`); `entity_type` siempre como bind.

use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use uuid::Uuid;
use validator::Validate;

use crate::{
    auth::Claims,
    error::AppError,
    middleware::auth_ext::UserPermissions,
    models::tag::{AssignTagRequest, CreateTag, Tag, UpdateTag},
    state::AppState,
};

pub async fn list_tags(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(_claims): Extension<Claims>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("tags.view")
        .map_err(|_| AppError::Forbidden)?;
    let tags =
        sqlx::query_as::<_, Tag>("SELECT id, name, color, created_at FROM tags ORDER BY name")
            .fetch_all(&state.db)
            .await?;
    Ok(Json(tags))
}

pub async fn create_tag(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(_claims): Extension<Claims>,
    Json(input): Json<CreateTag>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("tags.create")
        .map_err(|_| AppError::Forbidden)?;
    input.validate()?;
    let color = input.color.unwrap_or_else(|| "#6366f1".into());
    // 23505 (nombre duplicado) -> 409 vía `From<sqlx::Error>`.
    let tag = sqlx::query_as::<_, Tag>(
        "INSERT INTO tags (name, color) VALUES ($1, $2) \
         RETURNING id, name, color, created_at",
    )
    .bind(&input.name)
    .bind(&color)
    .fetch_one(&state.db)
    .await?;
    Ok((StatusCode::CREATED, Json(tag)))
}

pub async fn update_tag(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(_claims): Extension<Claims>,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateTag>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("tags.edit")
        .map_err(|_| AppError::Forbidden)?;
    input.validate()?;
    let tag = sqlx::query_as::<_, Tag>(
        "UPDATE tags SET name = COALESCE($2, name), color = COALESCE($3, color) \
         WHERE id = $1 RETURNING id, name, color, created_at",
    )
    .bind(id)
    .bind(input.name.as_deref())
    .bind(input.color.as_deref())
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(Json(tag))
}

pub async fn delete_tag(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(_claims): Extension<Claims>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("tags.delete")
        .map_err(|_| AppError::Forbidden)?;
    let result = sqlx::query("DELETE FROM tags WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn assign_tag(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(_claims): Extension<Claims>,
    Json(input): Json<AssignTagRequest>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("tags.create")
        .map_err(|_| AppError::Forbidden)?;
    input.validate()?;
    sqlx::query(
        "INSERT INTO entity_tags (tag_id, entity_type, entity_id) \
         VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
    )
    .bind(input.tag_id)
    .bind(&input.entity_type)
    .bind(input.entity_id)
    .execute(&state.db)
    .await?;
    Ok(StatusCode::CREATED)
}

pub async fn remove_tag(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(_claims): Extension<Claims>,
    Path((tag_id, entity_type, entity_id)): Path<(Uuid, String, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("tags.delete")
        .map_err(|_| AppError::Forbidden)?;
    if entity_type.is_empty() || entity_type.len() > 50 {
        return Err(AppError::Validation(
            "entity_type de 1 a 50 caracteres".into(),
        ));
    }
    let result = sqlx::query(
        "DELETE FROM entity_tags WHERE tag_id = $1 AND entity_type = $2 AND entity_id = $3",
    )
    .bind(tag_id)
    .bind(&entity_type)
    .bind(entity_id)
    .execute(&state.db)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_entity_tags(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(_claims): Extension<Claims>,
    Path((entity_type, entity_id)): Path<(String, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("tags.view")
        .map_err(|_| AppError::Forbidden)?;
    if entity_type.is_empty() || entity_type.len() > 50 {
        return Err(AppError::Validation(
            "entity_type de 1 a 50 caracteres".into(),
        ));
    }
    let tags = sqlx::query_as::<_, Tag>(
        "SELECT t.id, t.name, t.color, t.created_at FROM tags t \
         JOIN entity_tags et ON t.id = et.tag_id \
         WHERE et.entity_type = $1 AND et.entity_id = $2 ORDER BY t.name",
    )
    .bind(&entity_type)
    .bind(entity_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(tags))
}
