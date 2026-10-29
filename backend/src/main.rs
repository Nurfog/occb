//! OCCB backend — punto de entrada.
//!
//! Capas (de fuera hacia dentro): trace → request-id → cabeceras de
//! seguridad → timeout → límite de cuerpo → CORS → rate-limit → auth → handler.

mod auth;
mod config;
mod db;
mod error;
mod handlers;
mod middleware;
mod models;
mod password;
mod routes;
mod state;

use std::{net::SocketAddr, time::Duration};

use axum::http::{HeaderName, Method, StatusCode, header};
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    limit::RequestBodyLimitLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::{config::AppEnv, middleware::rate_limit::RateLimiter};

const REQUEST_ID_HEADER: &str = "x-request-id";

fn init_tracing(env: AppEnv) {
    let default_filter = match env {
        // tower_http en info: su nivel debug registra cabeceras (Authorization).
        AppEnv::Development => "occb_backend=debug,tower_http=info",
        AppEnv::Production => "info",
    };
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));
    let fmt = tracing_subscriber::fmt::layer().with_target(true);
    match env {
        AppEnv::Production => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt.json())
                .init();
        }
        AppEnv::Development => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt.pretty())
                .init();
        }
    }
}

fn build_cors(cfg: &config::Config) -> CorsLayer {
    let origins = cfg
        .cors_origins
        .iter()
        .map(|o| {
            o.parse()
                .unwrap_or_else(|_| panic!("origen CORS inválido: {o}"))
        })
        .collect::<Vec<_>>();
    CorsLayer::new()
        // Lista cerrada + credenciales. Jamás `*` con allow_credentials(true).
        .allow_origin(AllowOrigin::list(origins))
        .allow_credentials(true)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::ACCEPT,
            HeaderName::from_static(REQUEST_ID_HEADER),
        ])
        .max_age(Duration::from_secs(600))
}

#[tokio::main]
async fn main() {
    let cfg = config::Config::from_env();
    init_tracing(cfg.env);

    tracing::info!(
        env = cfg.env.name(),
        version = env!("CARGO_PKG_VERSION"),
        db = %cfg.database_url_redacted(),
        frontend = %cfg.frontend_url,
        redis = %cfg.redis_url.as_deref().map(|_| "configurado").unwrap_or("deshabilitado"),
        "arrancando occb-backend"
    );

    let pool = db::create_pool(&cfg).await;
    let state = state::AppState {
        db: pool,
        limiter: RateLimiter::new(cfg.rate_limit_per_minute),
        config: cfg.clone(),
    };

    let request_id = HeaderName::from_static(REQUEST_ID_HEADER);
    let app = middleware::security_headers::apply(routes::router(state), cfg.env.is_prod())
        .layer(build_cors(&cfg))
        .layer(RequestBodyLimitLayer::new(cfg.body_limit_bytes))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(10),
        ))
        .layer(SetRequestIdLayer::new(request_id.clone(), MakeRequestUuid))
        .layer(PropagateRequestIdLayer::new(request_id))
        .layer(TraceLayer::new_for_http());

    let addr = format!("{}:{}", cfg.host, cfg.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("no se pudo escuchar en {addr}: {e}"));
    tracing::info!("escuchando en {addr}");

    let shutdown = async {
        let ctrl_c = tokio::signal::ctrl_c();
        #[cfg(unix)]
        let terminate = async {
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("signal handler")
                .recv()
                .await;
        };
        #[cfg(not(unix))]
        let terminate = std::future::pending::<()>();
        tokio::select! {
            _ = ctrl_c => {},
            _ = terminate => {},
        }
        tracing::info!("apagado ordenado");
    };

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
    .await
    .expect("error del servidor");
}
