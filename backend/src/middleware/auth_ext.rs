//! RBAC extendido: permisos con caché de 60 s + extractor + middlewares.
//!
//! - `UserPermissions`: extractor que reutiliza los permisos ya cargados por
//!   `auth_middleware`/`admin_only_middleware` o los carga con `load_permissions`.
//! - La caché es en memoria (`PERM_TTL = 60 s`, acotada a 10 000 entradas).
//! - Las tablas RBAC (`profiles`, `profile_permissions`) NO existen en el
//!   esquema base (ver `backend/migrations/`): si faltan (`42P01`) se devuelve
//!   lista vacía y se registra un `warn`, sin romper la request.
//! - `admin_only_middleware` exige el permiso `admin.access` (mismo que origen).
//! - `csrf_middleware` es un stub compatible (double-submit `X-CSRF-Token` vs
//!   cookie `csrf_token`); NO está cableado en `routes.rs` (fichero congelado).
//! - TODO(bcrypt-compat): origen usaba bcrypt; aquí solo Argon2 (ver password.rs).

use std::{
    collections::HashMap,
    sync::OnceLock,
    time::{Duration, Instant},
};

use axum::{
    extract::{FromRequestParts, Request, State},
    http::{StatusCode, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{auth::Claims, error::AppError, state::AppState};

/// TTL de la caché de permisos (igual que origen: 60 s).
const PERM_TTL: Duration = Duration::from_secs(60);
/// Cota de entradas para no crecer sin límite en memoria.
const PERM_MAX_ENTRIES: usize = 10_000;

struct PermCacheEntry {
    perms: Vec<String>,
    fetched_at: Instant,
}

fn perm_cache() -> &'static Mutex<HashMap<Uuid, PermCacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<Uuid, PermCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn json_error(status: StatusCode, msg: &str) -> Response {
    (status, axum::Json(serde_json::json!({ "error": msg }))).into_response()
}

fn is_undefined_table(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(db) if db.code().as_deref() == Some("42P01"))
}

/// Carga los permisos del usuario desde `profile_permissions` con caché de 60 s.
///
/// Si las tablas RBAC no existen aún, devuelve lista vacía (y la cachea) para
/// no inundar los logs ni romper las rutas protegidas.
pub(crate) async fn load_permissions(
    state: &AppState,
    user_id: Uuid,
) -> Result<Vec<String>, StatusCode> {
    {
        let guard = perm_cache().lock().await;
        if let Some(entry) = guard.get(&user_id)
            && entry.fetched_at.elapsed() < PERM_TTL
        {
            return Ok(entry.perms.clone());
        }
    }

    let perms: Vec<String> = match sqlx::query_scalar(
        "SELECT pp.permission FROM profile_permissions pp \
         JOIN users u ON u.profile_id = pp.profile_id \
         WHERE u.id = $1",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    {
        Ok(p) => p,
        Err(e) => {
            if is_undefined_table(&e) {
                tracing::warn!(
                    "tablas RBAC ausentes (profiles/profile_permissions): permisos vacíos hasta migrar"
                );
            } else {
                tracing::error!("error cargando permisos: {e}");
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
            Vec::new()
        }
    };

    {
        let mut guard = perm_cache().lock().await;
        if guard.len() >= PERM_MAX_ENTRIES {
            let mut entries: Vec<(Uuid, Instant)> =
                guard.iter().map(|(k, v)| (*k, v.fetched_at)).collect();
            entries.sort_by_key(|(_, t)| *t);
            for (k, _) in entries.into_iter().take(PERM_MAX_ENTRIES / 2) {
                guard.remove(&k);
            }
        }
        guard.insert(
            user_id,
            PermCacheEntry {
                perms: perms.clone(),
                fetched_at: Instant::now(),
            },
        );
    }

    Ok(perms)
}

/// Exige superadmin (root instalado). Solo platform-level, nunca admins de
/// empresa: hay un único nivel por encima de todos los tenants.
pub async fn require_superadmin(state: &AppState, user_id: &Uuid) -> Result<(), AppError> {
    let ok: bool = sqlx::query_scalar("SELECT is_superadmin FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| AppError::Internal)?
        .unwrap_or(false);
    if ok { Ok(()) } else { Err(AppError::Forbidden) }
}

/// Invalida la caché de permisos de un usuario (llamar tras cambios de rol).
pub async fn invalidate_user_permissions(user_id: &Uuid) {
    perm_cache().lock().await.remove(user_id);
}

/// Invalida la caché de todos los usuarios con un perfil dado.
///
/// Si la columna `users.profile_id` no existe aún, no hace nada (solo log).
pub async fn invalidate_users_by_profile(pool: &sqlx::PgPool, profile_id: &Uuid) {
    let user_ids: Vec<Uuid> = match sqlx::query_scalar("SELECT id FROM users WHERE profile_id = $1")
        .bind(profile_id)
        .fetch_all(pool)
        .await
    {
        Ok(ids) => ids,
        Err(e) => {
            if !is_undefined_table(&e) {
                tracing::error!("error buscando usuarios por perfil: {e}");
            }
            return;
        }
    };
    if user_ids.is_empty() {
        return;
    }
    let mut guard = perm_cache().lock().await;
    for uid in &user_ids {
        guard.remove(uid);
    }
    tracing::info!(
        profile_id = %profile_id,
        count = user_ids.len(),
        "caché de permisos invalidada para usuarios del perfil"
    );
}

/// Genera un token CSRF opaco (UUID v4). El frontend debe devolverlo en
/// `X-CSRF-Token` cuando use cookies HttpOnly.
// Intencionalmente sin cablear: con Bearer en memoria no hay CSRF posible y
// cablearlo sin cookies rompería todos los mutantes con 403. Cablear solo
// junto a cookies HttpOnly + `csrf_middleware` en routes.
#[allow(dead_code)]
pub fn generate_csrf_token() -> String {
    Uuid::new_v4().to_string()
}

/// Solo `Authorization: Bearer <access JWT>` ya verificado + permiso admin.
///
/// Debe ejecutarse DESPUÉS de `auth_middleware` (lee `Claims` de extensiones).
#[allow(clippy::result_large_err)]
pub async fn admin_only_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, Response> {
    let claims = req
        .extensions()
        .get::<Claims>()
        .cloned()
        .ok_or_else(|| json_error(StatusCode::UNAUTHORIZED, "Unauthorized"))?;

    let permissions = load_permissions(&state, claims.sub)
        .await
        .map_err(|_| json_error(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error"))?;

    if !permissions.iter().any(|p| p == "admin.access") {
        return Err(json_error(StatusCode::FORBIDDEN, "Forbidden"));
    }

    req.extensions_mut().insert(UserPermissions(permissions));
    Ok(next.run(req).await)
}

/// Solo superadmin (root instalado). Va DESPUÉS de `auth_middleware`.
/// No usa perfiles: hay un único nivel por encima de todos los tenants.
#[allow(clippy::result_large_err)]
pub async fn superadmin_only_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, Response> {
    let claims = req
        .extensions()
        .get::<Claims>()
        .cloned()
        .ok_or_else(|| json_error(StatusCode::UNAUTHORIZED, "Unauthorized"))?;
    match require_superadmin(&state, &claims.sub).await {
        Ok(()) => Ok(next.run(req).await),
        Err(_) => Err(json_error(StatusCode::FORBIDDEN, "Forbidden")),
    }
}

/// Stub compatible de CSRF double-submit (origen: `middleware/auth.rs`).
///
/// - Omite GET/HEAD/OPTIONS y los endpoints de auth (`login`, `register`,
///   `refresh`, `forgot-password`, `reset-password`).
/// - En el resto exige `X-CSRF-Token: <valor de la cookie csrf_token>`.
/// - NO cableado en `routes.rs` a propósito (seguridad): sin cookies HttpOnly
///   no hay superficie CSRF y exigir el header rompería el flujo Bearer.
///   Cablear solo junto a cookies HttpOnly + `generate_csrf_token` en login.
#[allow(dead_code)]
#[allow(clippy::result_large_err)]
pub async fn csrf_middleware(req: Request, next: Next) -> Result<Response, Response> {
    let method = req.method().clone();
    if method == axum::http::Method::GET
        || method == axum::http::Method::HEAD
        || method == axum::http::Method::OPTIONS
    {
        return Ok(next.run(req).await);
    }

    let path = req.uri().path().to_string();
    if path.starts_with("/api/v1/auth/login")
        || path.starts_with("/api/v1/auth/register")
        || path.starts_with("/api/v1/auth/refresh")
        || path.starts_with("/api/v1/auth/forgot-password")
        || path.starts_with("/api/v1/auth/reset-password")
    {
        return Ok(next.run(req).await);
    }

    let header_csrf = req
        .headers()
        .get("X-CSRF-Token")
        .and_then(|h| h.to_str().ok());
    let cookie_csrf = req
        .headers()
        .get("cookie")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| {
            s.split(';')
                .find_map(|c| c.trim().strip_prefix("csrf_token=").map(str::to_string))
        });

    match (header_csrf, cookie_csrf) {
        (Some(h), Some(c)) if !h.is_empty() && h == c => Ok(next.run(req).await),
        _ => Err(json_error(StatusCode::FORBIDDEN, "CSRF validation failed")),
    }
}

/// Extractor de permisos del usuario actual.
///
/// Uso: `perms: UserPermissions` en la firma del handler y
/// `perms.require("recurso.accion").map_err(|_| AppError::Forbidden)?;`.
#[derive(Clone, Debug)]
pub struct UserPermissions(pub Vec<String>);

impl UserPermissions {
    pub fn has(&self, permission: &str) -> bool {
        self.0.iter().any(|p| p == permission)
    }

    pub fn require(&self, permission: &str) -> Result<(), StatusCode> {
        if self.has(permission) {
            Ok(())
        } else {
            Err(StatusCode::FORBIDDEN)
        }
    }
}

impl FromRequestParts<AppState> for UserPermissions {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if let Some(perms) = parts.extensions.get::<UserPermissions>() {
            return Ok(perms.clone());
        }
        let claims = parts
            .extensions
            .get::<Claims>()
            .cloned()
            .ok_or_else(|| json_error(StatusCode::UNAUTHORIZED, "Unauthorized"))?;
        let permissions = load_permissions(state, claims.sub)
            .await
            .map_err(|_| json_error(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error"))?;
        Ok(UserPermissions(permissions))
    }
}

/// Convierte errores de `StatusCode` del extractor a `AppError` cuando el
/// handler prefiere ese tipo. (Reservado para compatibilidad con origen.)
impl From<StatusCode> for AppError {
    fn from(s: StatusCode) -> Self {
        match s {
            StatusCode::FORBIDDEN => Self::Forbidden,
            StatusCode::UNAUTHORIZED => Self::Unauthorized,
            _ => Self::Internal,
        }
    }
}
