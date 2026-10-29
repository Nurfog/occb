//! Rate limiting global (ventana fija de 60 s por IP, en memoria).
//!
//! - Responde 429 + `Retry-After` + cabeceras `X-RateLimit-*`.
//! - IP cliente: si `TRUSTED_PROXY=true` (Caddy delante) usa la ÚLTIMA IP de
//!   `X-Forwarded-For` (la que añadió el proxy de confianza; la primera sería
//!   falsificable). Si no, usa la IP de la conexión.
//! - Para un límite distribuido entre réplicas, mueve este estado a Redis
//!   (el contenedor vive en el proyecto de base de datos / aparte).

use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    extract::{ConnectInfo, Request, State},
    http::{HeaderName, HeaderValue},
    middleware::Next,
    response::Response,
};
use tokio::sync::Mutex;

use crate::{error::AppError, state::AppState};

const WINDOW: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<Mutex<HashMap<IpAddr, Vec<Instant>>>>,
    max_per_minute: u32,
}

impl RateLimiter {
    pub fn new(max_per_minute: u32) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            max_per_minute: max_per_minute.max(1),
        }
    }

    /// Ok((límite, restantes)) o Err(retry_after_secs).
    pub async fn check(&self, ip: IpAddr) -> Result<(u32, u32), u64> {
        let mut map = self.inner.lock().await;
        if map.len() > 50_000 {
            // Anti-DoS de memoria: purga entradas vacías si hay demasiadas IPs.
            map.retain(|_, v| !v.is_empty());
        }
        let now = Instant::now();
        let hits = map.entry(ip).or_default();
        hits.retain(|t| now.duration_since(*t) < WINDOW);
        if hits.len() as u32 >= self.max_per_minute {
            let retry = hits
                .iter()
                .filter_map(|t| WINDOW.checked_sub(now.duration_since(*t)))
                .min()
                .map(|d| d.as_secs().max(1))
                .unwrap_or(1);
            return Err(retry);
        }
        hits.push(now);
        Ok((self.max_per_minute, self.max_per_minute - hits.len() as u32))
    }
}

fn client_ip(req: &Request, trusted_proxy: bool) -> IpAddr {
    if trusted_proxy
        && let Some(xff) = req
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
    {
        // rsplit: empieza por el final (el salto añadido por Caddy) y
        // se detiene en la primera IP válida (no falsificable).
        for part in xff.rsplit(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if let Ok(ip) = part.parse::<IpAddr>() {
                return ip;
            }
        }
    }
    // `into_make_service_with_connect_info::<SocketAddr>()` lo inserta.
    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip())
        .unwrap_or(IpAddr::from([127, 0, 0, 1]))
}

pub async fn rate_limit_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let ip = client_ip(&req, state.config.trusted_proxy);
    match state.limiter.check(ip).await {
        Ok((limit, remaining)) => {
            let mut resp = next.run(req).await;
            let h = resp.headers_mut();
            h.insert(
                HeaderName::from_static("x-ratelimit-limit"),
                HeaderValue::from_str(&limit.to_string()).unwrap(),
            );
            h.insert(
                HeaderName::from_static("x-ratelimit-remaining"),
                HeaderValue::from_str(&remaining.to_string()).unwrap(),
            );
            Ok(resp)
        }
        Err(retry_after_secs) => Err(AppError::RateLimited { retry_after_secs }),
    }
}
