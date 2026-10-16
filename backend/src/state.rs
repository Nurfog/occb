//! Estado compartido de la app.

use std::time::Duration;

use crate::{config::Config, middleware::rate_limit::RateLimiter};

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::PgPool,
    pub config: Config,
    pub limiter: RateLimiter,
    /// Límite estricto (10/min) para endpoints públicos sensibles
    /// (solicitudes de derechos Ley 21.719): anti-spam y anti-enumeración.
    pub strict_limiter: RateLimiter,
    pub smtp: crate::config::SmtpConfig,
    pub upload: crate::config::UploadConfig,
    pub http_client: reqwest::Client,
}

impl AppState {
    /// Cliente HTTP compartido con timeout de 30 s (evita cuelgues en
    /// llamadas salientes: SMTP no lo usa, pero sí futuros providers OAuth).
    pub fn http_client() -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("no se pudo crear el cliente HTTP")
    }
}
