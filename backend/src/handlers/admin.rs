//! Administración del CRM.
//!
//! - Usuarios: CRUD sobre el esquema base (`users` con `name`, sin
//!   `profile_id`). Crear usa hash Argon2id (`crate::password`); `23505` -> 409.
//!   Listar con paginación acotada + ORDER BY de lista blanca + LIKE con escape.
//!   Borrar impide auto-borrado; 0 filas -> 404; invalida caché de permisos.
//! - Pipelines / perfiles / branding: port de opencrmccb
//!   (`handlers/admin.rs`, tablas en migración `014_010_admin_pipelines.sql`).
//! - Todas las queries con binds; `validator` en inputs; sin nuevas deps.

use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use crate::{
    auth::Claims,
    error::AppError,
    middleware::auth_ext::{
        UserPermissions, invalidate_user_permissions, invalidate_users_by_profile,
    },
    models::{ListQuery, UserPublic, escape_like, normalize_list},
    password,
    state::AppState,
};

#[derive(Debug, Deserialize, Validate)]
pub struct AdminCreateUserInput {
    #[validate(email(message = "email inválido"), length(max = 320))]
    pub email: String,
    #[validate(length(min = 1, max = 100, message = "nombre de 1 a 100 caracteres"))]
    pub name: String,
    /// Política fina (12..128) en `password::hash_password`.
    #[validate(length(min = 1, max = 128))]
    pub password: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct AdminUpdateUserInput {
    #[validate(email(message = "email inválido"), length(max = 320))]
    pub email: Option<String>,
    #[validate(length(min = 1, max = 100, message = "nombre de 1 a 100 caracteres"))]
    pub name: Option<String>,
}

/// Lista usuarios (admin). Paginada como `users::list_users`.
pub async fn admin_list_users(
    State(state): State<AppState>,
    perms: UserPermissions,
    Extension(claims): Extension<Claims>,
    Query(q): Query<ListQuery>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("admin.access")
        .map_err(|_| AppError::Forbidden)?;
    let scope = crate::auth::tenant_scope(&state, &claims.sub).await?;
    let (page, per_page, sort_col, sort_dir) = normalize_list(&q);
    let offset = (page - 1) * per_page;
    let pattern = q.q.as_deref().map(|s| format!("%{}%", escape_like(s)));

    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM users
         WHERE ($1::text IS NULL
            OR email::text ILIKE $1 ESCAPE '\\'
            OR name ILIKE $1 ESCAPE '\\')
           AND ($2::uuid IS NULL OR tenant_id = $2)",
    )
    .bind(pattern.as_deref())
    .bind(scope)
    .fetch_one(&state.db)
    .await?;

    // sort_col/sort_dir son constantes de lista blanca (ver normalize_list).
    let sql = format!(
        "SELECT id, email, name, created_at FROM users
         WHERE ($1::text IS NULL
            OR email::text ILIKE $1 ESCAPE '\\'
            OR name ILIKE $1 ESCAPE '\\')
           AND ($4::uuid IS NULL OR tenant_id = $4)
         ORDER BY {sort_col} {sort_dir} LIMIT $2 OFFSET $3"
    );
    let users = sqlx::query_as::<_, UserPublic>(&sql)
        .bind(pattern.as_deref())
        .bind(per_page)
        .bind(offset)
        .bind(scope)
        .fetch_all(&state.db)
        .await?;

    let body: Value = json!({ "data": users, "page": page, "per_page": per_page, "total": total });
    Ok(Json(body))
}

/// Crea un usuario (admin). Hash Argon2id; email duplicado -> 409.
pub async fn admin_create_user(
    State(state): State<AppState>,
    perms: UserPermissions,
    Json(input): Json<AdminCreateUserInput>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("admin.access")
        .map_err(|_| AppError::Forbidden)?;
    input.validate()?;
    let email = input.email.trim().to_lowercase();
    let hash = password::hash_password(&input.password)?;
    let user = sqlx::query_as::<_, UserPublic>(
        "INSERT INTO users (email, name, password_hash) VALUES ($1, $2, $3)
         RETURNING id, email, name, created_at",
    )
    .bind(&email)
    .bind(input.name.trim())
    .bind(&hash)
    .fetch_one(&state.db)
    .await?;
    Ok((StatusCode::CREATED, Json(user)))
}

/// Actualiza email y/o nombre de un usuario (admin).
pub async fn admin_update_user(
    State(state): State<AppState>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
    Json(input): Json<AdminUpdateUserInput>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("admin.access")
        .map_err(|_| AppError::Forbidden)?;
    input.validate()?;
    if input.email.is_none() && input.name.is_none() {
        return Err(AppError::Validation("nada que actualizar".to_string()));
    }
    let email = input.email.as_deref().map(|e| e.trim().to_lowercase());
    let name = input.name.as_deref().map(str::trim);
    let user: Option<UserPublic> = sqlx::query_as::<_, UserPublic>(
        "UPDATE users SET email = COALESCE($2, email), name = COALESCE($3, name)
         WHERE id = $1
         RETURNING id, email, name, created_at",
    )
    .bind(id)
    .bind(email.as_deref())
    .bind(name)
    .fetch_optional(&state.db)
    .await?;
    let user = user.ok_or(AppError::NotFound)?;
    invalidate_user_permissions(&id).await;
    Ok(Json(user))
}

/// Borra un usuario (admin). No permite auto-borrado.
pub async fn admin_delete_user(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    perms
        .require("admin.access")
        .map_err(|_| AppError::Forbidden)?;
    if id == claims.sub {
        return Err(AppError::Validation(
            "no puedes borrar tu propio usuario".to_string(),
        ));
    }
    let result = sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    invalidate_user_permissions(&id).await;
    Ok(StatusCode::NO_CONTENT)
}

// ─── Pipelines (port de opencrmccb `handlers/admin.rs`) ───
// Tablas `pipelines` / `pipeline_stages` (migración 014_010_admin_pipelines).
// `list_pipelines` es lectura para cualquier usuario autenticado (igual que
// origen: sin check admin). El resto cuelga del router admin (ya exige
// `admin.access` en `admin_only_middleware`) y lo re-exige por defensa en
// profundidad.

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Pipeline {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub entity_type: String,
    pub is_default: bool,
    pub is_active: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct PipelineStage {
    pub id: Uuid,
    pub pipeline_id: Uuid,
    pub name: String,
    pub position: i32,
    pub color: Option<String>,
    pub probability: Option<i32>,
    pub is_default: bool,
}

#[derive(Debug, Deserialize, Validate)]
pub struct PipelineInput {
    #[validate(length(min = 1, max = 200))]
    pub name: String,
    #[validate(length(min = 1, max = 100), custom(function = "validate_slug"))]
    pub slug: String,
    #[validate(length(max = 2000))]
    pub description: Option<String>,
    #[validate(length(max = 50))]
    pub entity_type: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct StageInput {
    #[validate(length(min = 1, max = 200))]
    pub name: String,
    #[validate(range(min = 0))]
    pub position: i32,
    #[validate(custom(function = "validate_hex_color_opt"))]
    pub color: Option<String>,
    #[validate(range(min = 0, max = 100))]
    pub probability: Option<i32>,
}

fn validate_slug(value: &str) -> Result<(), validator::ValidationError> {
    if value
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        Ok(())
    } else {
        Err(validator::ValidationError::new(
            "Slug must be lowercase alphanumeric with dashes/underscores",
        ))
    }
}

fn validate_hex_color_opt(value: &&String) -> Result<(), validator::ValidationError> {
    let v = value.trim_start_matches('#');
    if (v.len() == 3 || v.len() == 6) && v.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(validator::ValidationError::new(
            "Color must be hex like #RGB or #RRGGBB",
        ))
    }
}

fn validate_domain_opt(value: &&String) -> Result<(), validator::ValidationError> {
    let v = value.as_str();
    let ok = !v.is_empty()
        && v.len() <= 253
        && !v.contains("://")
        && !v
            .chars()
            .any(|c| c.is_whitespace() || c == '/' || c == '\\' || c == '@')
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(validator::ValidationError::new(
            "Custom domain must be a plain hostname without scheme",
        ))
    }
}

#[derive(Debug, Serialize)]
pub struct PipelineWithStages {
    pub pipeline: Pipeline,
    pub stages: Vec<PipelineStage>,
}

/// Lista pipelines con sus etapas. Lectura autenticada (origen sin check admin).
pub async fn list_pipelines(
    State(state): State<AppState>,
) -> Result<Json<Vec<PipelineWithStages>>, AppError> {
    let pipelines = sqlx::query_as::<_, Pipeline>(
        "SELECT id, name, slug, description, entity_type, is_default, is_active, created_at FROM pipelines WHERE is_active = true ORDER BY created_at",
    )
    .fetch_all(&state.db)
    .await?;

    let pipeline_ids: Vec<Uuid> = pipelines.iter().map(|p| p.id).collect();
    let all_stages = if pipeline_ids.is_empty() {
        Vec::new()
    } else {
        sqlx::query_as::<_, PipelineStage>(
            "SELECT id, pipeline_id, name, position, color, probability, is_default FROM pipeline_stages WHERE pipeline_id = ANY($1) ORDER BY position",
        )
        .bind(&pipeline_ids)
        .fetch_all(&state.db)
        .await?
    };

    let mut stages_by_pipeline: std::collections::HashMap<Uuid, Vec<PipelineStage>> =
        std::collections::HashMap::new();
    for stage in all_stages {
        stages_by_pipeline
            .entry(stage.pipeline_id)
            .or_default()
            .push(stage);
    }

    let mut result = Vec::new();
    for p in pipelines {
        let stages = stages_by_pipeline.remove(&p.id).unwrap_or_default();
        result.push(PipelineWithStages {
            pipeline: p,
            stages,
        });
    }

    Ok(Json(result))
}

pub async fn create_pipeline(
    State(state): State<AppState>,
    perms: UserPermissions,
    Json(input): Json<PipelineInput>,
) -> Result<(StatusCode, Json<Pipeline>), AppError> {
    perms
        .require("admin.access")
        .map_err(|_| AppError::Forbidden)?;
    input.validate()?;
    // Duplicado de slug (`23505`) -> 409 vía `From<sqlx::Error>`; resto -> 500.
    let pipeline = sqlx::query_as::<_, Pipeline>(
        r#"
        INSERT INTO pipelines (name, slug, description, entity_type)
        VALUES ($1, $2, $3, $4)
        RETURNING id, name, slug, description, entity_type, is_default, is_active, created_at
        "#,
    )
    .bind(&input.name)
    .bind(&input.slug)
    .bind(&input.description)
    .bind(input.entity_type.as_deref().unwrap_or("person"))
    .fetch_one(&state.db)
    .await?;

    Ok((StatusCode::CREATED, Json(pipeline)))
}

pub async fn update_pipeline(
    State(state): State<AppState>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
    Json(input): Json<PipelineInput>,
) -> Result<Json<Pipeline>, AppError> {
    perms
        .require("admin.access")
        .map_err(|_| AppError::Forbidden)?;
    input.validate()?;

    let pipeline = sqlx::query_as::<_, Pipeline>(
        r#"
        UPDATE pipelines SET name = $2, slug = $3, description = $4, entity_type = $5, updated_at = NOW()
        WHERE id = $1
        RETURNING id, name, slug, description, entity_type, is_default, is_active, created_at
        "#,
    )
    .bind(id)
    .bind(&input.name)
    .bind(&input.slug)
    .bind(&input.description)
    .bind(input.entity_type.as_deref().unwrap_or("person"))
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    Ok(Json(pipeline))
}

pub async fn delete_pipeline(
    State(state): State<AppState>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    perms
        .require("admin.access")
        .map_err(|_| AppError::Forbidden)?;
    // Origen: 400 si no se puede borrar (inexistente o por defecto). Aquí sin
    // variante BadRequest: por defecto protegido -> 422; inexistente -> 404.
    let result = sqlx::query("DELETE FROM pipelines WHERE id = $1 AND is_default = false")
        .bind(id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pipelines WHERE id = $1)")
                .bind(id)
                .fetch_one(&state.db)
                .await?;
        if exists {
            return Err(AppError::Validation(
                "no se puede eliminar el pipeline por defecto".to_string(),
            ));
        }
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

// ─── Stages ──────────────────────────────────────────────────────

pub async fn create_stage(
    State(state): State<AppState>,
    perms: UserPermissions,
    Path(pipeline_id): Path<Uuid>,
    Json(input): Json<StageInput>,
) -> Result<(StatusCode, Json<PipelineStage>), AppError> {
    perms
        .require("admin.access")
        .map_err(|_| AppError::Forbidden)?;
    input.validate()?;

    let stage = sqlx::query_as::<_, PipelineStage>(
        r#"
        INSERT INTO pipeline_stages (pipeline_id, name, position, color, probability)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, pipeline_id, name, position, color, probability, is_default
        "#,
    )
    .bind(pipeline_id)
    .bind(&input.name)
    .bind(input.position)
    .bind(&input.color)
    .bind(input.probability)
    .fetch_one(&state.db)
    .await?;

    Ok((StatusCode::CREATED, Json(stage)))
}

pub async fn delete_stage(
    State(state): State<AppState>,
    perms: UserPermissions,
    Path((_pipeline_id, stage_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    perms
        .require("admin.access")
        .map_err(|_| AppError::Forbidden)?;
    let result = sqlx::query("DELETE FROM pipeline_stages WHERE id = $1 AND is_default = false")
        .bind(stage_id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pipeline_stages WHERE id = $1)")
                .bind(stage_id)
                .fetch_one(&state.db)
                .await?;
        if exists {
            return Err(AppError::Validation(
                "no se puede eliminar la etapa por defecto".to_string(),
            ));
        }
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

// ─── Profiles ────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Profile {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub is_system: bool,
}

#[derive(Debug, Serialize)]
pub struct ProfileWithPermissions {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub is_system: bool,
    pub permissions: Vec<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct ProfileInput {
    #[validate(length(min = 1, max = 100))]
    pub name: String,
    pub description: Option<String>,
    pub permissions: Option<Vec<String>>,
}

#[derive(sqlx::FromRow)]
struct ProfileWithPermissionsRow {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub is_system: bool,
    pub permissions: Vec<String>,
}

pub async fn list_profiles(
    State(state): State<AppState>,
    perms: UserPermissions,
) -> Result<Json<Vec<ProfileWithPermissions>>, AppError> {
    perms
        .require("admin.access")
        .map_err(|_| AppError::Forbidden)?;
    let rows = sqlx::query_as::<_, ProfileWithPermissionsRow>(
        r#"
        SELECT p.id, p.name, p.description, p.is_system,
               COALESCE(array_agg(pp.permission) FILTER (WHERE pp.permission IS NOT NULL), ARRAY[]::text[]) AS permissions
        FROM profiles p
        LEFT JOIN profile_permissions pp ON pp.profile_id = p.id
        GROUP BY p.id
        ORDER BY p.created_at
        "#,
    )
    .fetch_all(&state.db)
    .await?;

    let result = rows
        .into_iter()
        .map(|r| ProfileWithPermissions {
            id: r.id,
            name: r.name,
            description: r.description,
            is_system: r.is_system,
            permissions: r.permissions,
        })
        .collect();

    Ok(Json(result))
}

pub async fn create_profile(
    State(state): State<AppState>,
    perms: UserPermissions,
    Json(input): Json<ProfileInput>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    perms
        .require("admin.access")
        .map_err(|_| AppError::Forbidden)?;
    input.validate()?;

    // Nombre duplicado (`23505`) -> 409 vía `From<sqlx::Error>`.
    let profile = sqlx::query_as::<_, Profile>(
        r#"
        INSERT INTO profiles (name, description) VALUES ($1, $2)
        RETURNING id, name, description, is_system
        "#,
    )
    .bind(&input.name)
    .bind(&input.description)
    .fetch_one(&state.db)
    .await?;

    if let Some(perms) = &input.permissions {
        // Inserción atómica en tx para no dejar el perfil a medio poblar.
        let mut tx = state.db.begin().await?;
        for perm in perms {
            sqlx::query(
                "INSERT INTO profile_permissions (profile_id, permission) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            )
            .bind(profile.id)
            .bind(perm)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        invalidate_users_by_profile(&state.db, &profile.id).await;
    }

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "id": profile.id, "name": profile.name, "description": profile.description, "is_system": profile.is_system,
            "permissions": input.permissions.unwrap_or_default(),
        })),
    ))
}

pub async fn update_profile(
    State(state): State<AppState>,
    perms: UserPermissions,
    Path(id): Path<Uuid>,
    Json(input): Json<ProfileInput>,
) -> Result<Json<Value>, AppError> {
    perms
        .require("admin.access")
        .map_err(|_| AppError::Forbidden)?;
    input.validate()?;

    let profile = sqlx::query_as::<_, Profile>(
        "UPDATE profiles SET name = $2, description = $3, updated_at = NOW() WHERE id = $1 RETURNING id, name, description, is_system",
    )
    .bind(id)
    .bind(&input.name)
    .bind(&input.description)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;

    if let Some(perms) = &input.permissions {
        // Reemplazo atómico en tx: DELETE + INSERTs con commit único.
        let mut tx = state.db.begin().await?;
        sqlx::query("DELETE FROM profile_permissions WHERE profile_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        for perm in perms {
            sqlx::query("INSERT INTO profile_permissions (profile_id, permission) VALUES ($1, $2)")
                .bind(id)
                .bind(perm)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        invalidate_users_by_profile(&state.db, &id).await;
    }

    Ok(Json(json!({
        "id": profile.id, "name": profile.name, "description": profile.description, "is_system": profile.is_system,
        "permissions": input.permissions.unwrap_or_default(),
    })))
}

// ─── Branding ────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Branding {
    pub id: Uuid,
    pub company_name: Option<String>,
    pub logo_url: Option<String>,
    pub primary_color: Option<String>,
    pub secondary_color: Option<String>,
    pub accent_color: Option<String>,
    pub favicon_url: Option<String>,
    pub custom_domain: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct BrandingInput {
    #[validate(length(max = 255))]
    pub company_name: Option<String>,
    #[validate(length(max = 500), url)]
    pub logo_url: Option<String>,
    #[validate(custom(function = "validate_hex_color_opt"))]
    pub primary_color: Option<String>,
    #[validate(custom(function = "validate_hex_color_opt"))]
    pub secondary_color: Option<String>,
    #[validate(custom(function = "validate_hex_color_opt"))]
    pub accent_color: Option<String>,
    #[validate(length(max = 500), url)]
    pub favicon_url: Option<String>,
    #[validate(custom(function = "validate_domain_opt"))]
    pub custom_domain: Option<String>,
}

/// Branding público (ruta sin JWT, igual que origen: sin check de permisos).
pub async fn get_branding(State(state): State<AppState>) -> Result<Json<Branding>, AppError> {
    let branding = sqlx::query_as::<_, Branding>(
        "SELECT id, company_name, logo_url, primary_color, secondary_color, accent_color, favicon_url, custom_domain FROM branding ORDER BY id LIMIT 1",
    )
    .fetch_optional(&state.db)
    .await?
    .unwrap_or(Branding {
        id: Uuid::nil(), company_name: Some("OpenCRM".into()), logo_url: None,
        primary_color: Some("#2563eb".into()), secondary_color: Some("#1e40af".into()),
        accent_color: Some("#10b981".into()), favicon_url: None, custom_domain: None,
    });

    Ok(Json(branding))
}

pub async fn update_branding(
    State(state): State<AppState>,
    perms: UserPermissions,
    Json(input): Json<BrandingInput>,
) -> Result<Json<Branding>, AppError> {
    perms
        .require("admin.access")
        .map_err(|_| AppError::Forbidden)?;
    input.validate()?;

    let existing = sqlx::query_scalar::<_, Uuid>("SELECT id FROM branding LIMIT 1")
        .fetch_optional(&state.db)
        .await?;

    if let Some(id) = existing {
        sqlx::query(
            r#"
            UPDATE branding SET
                company_name = COALESCE($2, company_name),
                logo_url = COALESCE($3, logo_url),
                primary_color = COALESCE($4, primary_color),
                secondary_color = COALESCE($5, secondary_color),
                accent_color = COALESCE($6, accent_color),
                favicon_url = COALESCE($7, favicon_url),
                custom_domain = COALESCE($8, custom_domain),
                updated_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(&input.company_name)
        .bind(&input.logo_url)
        .bind(&input.primary_color)
        .bind(&input.secondary_color)
        .bind(&input.accent_color)
        .bind(&input.favicon_url)
        .bind(&input.custom_domain)
        .execute(&state.db)
        .await?;
    } else {
        sqlx::query(
            r#"
            INSERT INTO branding (company_name, logo_url, primary_color, secondary_color, accent_color, favicon_url, custom_domain)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(&input.company_name)
        .bind(&input.logo_url)
        .bind(&input.primary_color)
        .bind(&input.secondary_color)
        .bind(&input.accent_color)
        .bind(&input.favicon_url)
        .bind(&input.custom_domain)
        .execute(&state.db)
        .await?;
    }

    get_branding(State(state)).await
}
