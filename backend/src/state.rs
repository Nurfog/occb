//! Estado compartido de la app.

use crate::{config::Config, middleware::rate_limit::RateLimiter};

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::PgPool,
    pub config: Config,
    pub limiter: RateLimiter,
}
