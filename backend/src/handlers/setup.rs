//! Instalador inicial estilo WordPress (`/setup` del frontend).
//!
//! - `GET /api/v1/setup/status` (público): ¿instalado? + checklist de
//!   requisitos (booleanos, sin fugas: nunca secretos ni conteos exactos).
//! - `POST /api/v1/setup/install` (público, rate-limit estricto): valida
//!   todo, crea el PRIMER usuario (root/admin) y fija el nombre del sitio.
//!   Una vez instalado responde 403 para siempre (el instalador se
//!   "autodestruye" lógicamente; no hay flag que revertir).
//! - La creación reutiliza `auth::create_first_admin` (mismo advisory lock
//!   anti-TOCTOU que `register`).

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::{db, error::AppError, handlers::auth::create_first_admin, state::AppState};

#[derive(Debug, Serialize)]
pub struct SetupCheck {
    pub ok: bool,
    pub detail: &'static str,
}

#[derive(Debug, Serialize)]
pub struct SetupStatus {
    pub installed: bool,
    pub version: &'static str,
    pub checks: Vec<(&'static str, SetupCheck)>,
}

async fn users_exist(db: &sqlx::PgPool) -> Result<bool, AppError> {
    sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM users)")
        .fetch_one(db)
        .await
        .map_err(|_| AppError::Unavailable)
}

/// Estado del instalador + checklist (sin secretos).
pub async fn status(State(state): State<AppState>) -> Result<impl IntoResponse, AppError> {
    let db_ok = db::ping(&state.db).await;
    let installed = if db_ok {
        users_exist(&state.db).await.unwrap_or(true)
    } else {
        true
    };
    let jwt_ok = state.config.jwt_secret.len() >= 32;
    let redis = match state.config.redis_url.as_deref() {
        None => (true, "deshabilitado: rate-limit en memoria"),
        Some(_) => (true, "configurado"),
    };
    let encryption = match state.config.token_encryption_key {
        Some(_) => (true, "clave de cifrado configurada"),
        None => (
            !state.config.env.is_prod(),
            "sin clave: tokens externos en claro (solo dev)",
        ),
    };
    Ok(Json(SetupStatus {
        installed,
        version: env!("CARGO_PKG_VERSION"),
        checks: vec![
            (
                "base_datos",
                SetupCheck {
                    ok: db_ok,
                    detail: "Postgres + migraciones aplicadas",
                },
            ),
            (
                "secretos",
                SetupCheck {
                    ok: jwt_ok,
                    detail: "JWT_SECRET con longitud mínima",
                },
            ),
            (
                "redis",
                SetupCheck {
                    ok: redis.0,
                    detail: redis.1,
                },
            ),
            (
                "cifrado",
                SetupCheck {
                    ok: encryption.0,
                    detail: encryption.1,
                },
            ),
        ],
    }))
}

#[derive(Debug, Deserialize, Validate)]
pub struct InstallInput {
    /// Nombre del sitio/colegio (se guarda en branding).
    #[validate(length(min = 1, max = 100, message = "nombre del sitio de 1 a 100 caracteres"))]
    pub site_name: String,
    #[validate(length(min = 1, max = 100, message = "nombre de 1 a 100 caracteres"))]
    pub admin_name: String,
    #[validate(email(message = "email inválido"), length(max = 320))]
    pub admin_email: String,
    /// Política 12..128 aplicada en `password::hash_password`.
    #[validate(length(min = 1, max = 128))]
    pub admin_password: String,
}

/// Ejecuta la instalación: checks + primer admin + nombre del sitio.
/// 403 si ya instalado o si algún check crítico falla.
pub async fn install(
    State(state): State<AppState>,
    Json(input): Json<InstallInput>,
) -> Result<impl IntoResponse, AppError> {
    input.validate()?;
    if !db::ping(&state.db).await {
        return Err(AppError::Unavailable);
    }
    if state.config.jwt_secret.len() < 32 {
        return Err(AppError::Validation(
            "JWT_SECRET inválido en el servidor".into(),
        ));
    }
    let (user, access, refresh) = create_first_admin(
        &state,
        &input.admin_email,
        &input.admin_name,
        &input.admin_password,
    )
    .await?;
    // Nombre del sitio en branding (best-effort, no falla la instalación).
    let site = input.site_name.trim().to_string();
    let _ = sqlx::query(
        "UPDATE branding SET company_name = $1 \
         WHERE id = (SELECT id FROM branding LIMIT 1)",
    )
    .bind(&site)
    .execute(&state.db)
    .await;
    let _ = crate::handlers::audit::insert_audit_log(
        &state.db,
        Some(user.id),
        "install",
        "system",
        user.id,
        None,
        Some(serde_json::json!({ "site_name": site })),
    )
    .await;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "installed": true,
            "site_name": site,
            "user": {
                "id": user.id,
                "email": user.email,
                "name": user.name,
            },
            "access_token": access,
            "refresh_token": refresh,
            "token_type": "Bearer",
            "expires_in": state.config.access_token_ttl_minutes * 60,
        })),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_rechaza_inputs_invalidos() {
        let bad = InstallInput {
            site_name: "".into(),
            admin_name: "".into(),
            admin_email: "no-es-email".into(),
            admin_password: "x".into(),
        };
        assert!(bad.validate().is_err());
        let ok = InstallInput {
            site_name: "Colegio Demo".into(),
            admin_name: "Root Admin".into(),
            admin_email: "root@colegio.cl".into(),
            admin_password: "clave-inicial-segura-123".into(),
        };
        assert!(ok.validate().is_ok());
    }
}
