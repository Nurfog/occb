//! Pool de Postgres con timeots defensivos.
//!
//! - `statement_timeout = 5s`: ninguna consulta puede colgar el pool (mitiga DoS).
//! - `idle_in_transaction_session_timeout = 10s`: sin transacciones olvidadas abiertas.

use sqlx::postgres::{PgConnectOptions, PgConnection, PgPoolOptions};
use std::str::FromStr;
use std::time::Duration;

use crate::config::Config;

pub async fn create_pool(cfg: &Config) -> sqlx::PgPool {
    let options =
        PgConnectOptions::from_str(&cfg.database_url).expect("DATABASE_URL con formato inválido");
    let max_conn = if cfg.env.is_prod() { 20 } else { 5 };

    PgPoolOptions::new()
        .max_connections(max_conn)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Duration::from_secs(600))
        .max_lifetime(Duration::from_secs(1800))
        .after_connect(|conn: &mut PgConnection, _| {
            Box::pin(async move {
                sqlx::query("SET statement_timeout = '5s'")
                    .execute(&mut *conn)
                    .await?;
                sqlx::query("SET idle_in_transaction_session_timeout = '10s'")
                    .execute(&mut *conn)
                    .await?;
                Ok::<_, sqlx::Error>(())
            })
        })
        .connect_with(options)
        .await
        .expect("no se pudo conectar a Postgres (revisa DATABASE_URL y que db esté healthy)")
}

/// Ping con timeout para el endpoint /ready.
pub async fn ping(pool: &sqlx::PgPool) -> bool {
    tokio::time::timeout(
        Duration::from_secs(2),
        sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(pool),
    )
    .await
    .map(|r| r.is_ok())
    .unwrap_or(false)
}
