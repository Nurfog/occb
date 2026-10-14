//! Readiness: 200 solo si las dependencias responden, 503 si no.
//!
//! - Postgres: `SELECT 1` con timeout (ver `db::ping`).
//! - Redis (opcional): PING por TCP con AUTH si la URL trae password.
//!   Sin `REDIS_URL` se informa `redis: "disabled"` y no falla.

use std::time::Duration;

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{db, state::AppState};

fn parse_redis_url(url: &str) -> Option<(Option<String>, String, u16)> {
    let rest = url.strip_prefix("redis://")?;
    let (auth, hostport) = match rest.rsplit_once('@') {
        Some((a, h)) => (Some(a.to_string()), h),
        None => (None, rest),
    };
    let pass = auth.map(|a| a.strip_prefix(':').map(str::to_string).unwrap_or(a));
    let (host, port) = match hostport.split_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().unwrap_or(6379)),
        None => (hostport.to_string(), 6379),
    };
    if host.is_empty() {
        return None;
    }
    Some((pass, host, port))
}

async fn ping_redis(url: &str) -> bool {
    let Some((pass, host, port)) = parse_redis_url(url) else {
        return false;
    };
    let conn = tokio::time::timeout(
        Duration::from_secs(2),
        tokio::net::TcpStream::connect((host.as_str(), port)),
    )
    .await;
    let Ok(Ok(mut stream)) = conn else {
        return false;
    };
    if let Some(pw) = pass {
        let auth_cmd = format!("AUTH {pw}\r\n");
        if tokio::time::timeout(
            Duration::from_secs(2),
            stream.write_all(auth_cmd.as_bytes()),
        )
        .await
        .is_err()
        {
            return false;
        }
        let mut buf = [0u8; 5];
        if tokio::time::timeout(Duration::from_secs(2), stream.read_exact(&mut buf))
            .await
            .is_err()
        {
            return false;
        }
        if &buf != b"+OK\r\n" {
            return false;
        }
    }
    if tokio::time::timeout(Duration::from_secs(2), stream.write_all(b"PING\r\n"))
        .await
        .is_err()
    {
        return false;
    }
    let mut buf = [0u8; 7];
    if tokio::time::timeout(Duration::from_secs(2), stream.read_exact(&mut buf))
        .await
        .is_err()
    {
        return false;
    }
    &buf == b"+PONG\r\n"
}

pub async fn ready(State(state): State<AppState>) -> impl IntoResponse {
    let db_ok = db::ping(&state.db).await;
    let (redis_status, redis_ok) = match &state.config.redis_url {
        None => ("disabled", true),
        Some(url) => {
            if ping_redis(url).await {
                ("up", true)
            } else {
                ("down", false)
            }
        }
    };
    let body: Value = json!({
        "status": if db_ok && redis_ok { "ready" } else { "not_ready" },
        "checks": { "db": if db_ok { "up" } else { "down" }, "redis": redis_status },
    });
    if db_ok && redis_ok {
        (StatusCode::OK, Json(body)).into_response()
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_redis_url_variantes() {
        assert_eq!(
            parse_redis_url("redis://:secreto@redis:6379"),
            Some((Some("secreto".to_string()), "redis".to_string(), 6379))
        );
        let (pass, host, port) = parse_redis_url("redis://cache:6380").unwrap();
        assert_eq!((pass, host, port), (None, "cache".to_string(), 6380));
        assert!(parse_redis_url("http://x").is_none());
    }
}
