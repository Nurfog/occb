//! Tabla de rutas del API.
//!
//! - Públicas: health, ready, version, auth (con rate-limit global).
//! - Protegidas: JWT + rate-limit global.
//! - Autorizadas por RBAC: /users* (`users.read`), /admin/* (`admin.access`).
//! - `/metrics` NO se expone (si se necesita, servirlo en otro puerto interno).

use axum::{
    Router,
    middleware::from_fn_with_state,
    routing::{get, post},
};

use crate::{handlers, middleware, state::AppState};

pub fn router(state: AppState) -> Router {
    // RBAC interno: requiere JWT previo (la capa auth se aplica fuera).
    let users_routes = Router::new()
        .route("/api/v1/users", get(handlers::users::list_users))
        .route("/api/v1/users/{id}", get(handlers::users::get_user))
        .layer(from_fn_with_state(
            (state.clone(), "users.read"),
            middleware::authorize::require_permission,
        ));

    let admin_routes = Router::new()
        .route("/api/v1/admin/ping", get(handlers::admin::ping))
        .layer(from_fn_with_state(
            (state.clone(), "admin.access"),
            middleware::authorize::require_permission,
        ));

    let protected = Router::new()
        .route("/api/v1/auth/me", get(handlers::auth::me))
        .route("/api/v1/auth/logout", post(handlers::auth::logout))
        .merge(users_routes)
        .merge(admin_routes)
        .layer(from_fn_with_state(
            state.clone(),
            middleware::auth::auth_middleware,
        ));

    Router::new()
        .route("/api/v1/health", get(handlers::health::health))
        .route("/api/v1/ready", get(handlers::ready::ready))
        .route("/api/v1/version", get(handlers::health::version))
        .route("/api/v1/auth/register", post(handlers::auth::register))
        .route("/api/v1/auth/login", post(handlers::auth::login))
        .route("/api/v1/auth/refresh", post(handlers::auth::refresh))
        .merge(protected)
        .layer(from_fn_with_state(
            state.clone(),
            middleware::rate_limit::rate_limit_middleware,
        ))
        .with_state(state)
}
