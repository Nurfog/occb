//! Métricas HTTP (stub sin Prometheus).
//!
//! - Origen usaba `prometheus` (`Metrics`, `metrics_handler`, histogramas).
//!   Esa dependencia NO existe en este workspace y no se pueden añadir deps,
//!   así que `track_metrics` es un pass-through compatible en firma.
//! - Se conserva `normalize_path` (UUIDs y segmentos numéricos -> `:id`) para
//!   acotar cardinalidad cuando se cablee un backend real de métricas.
//! - `/metrics` NO se expone (ver `routes.rs`): si se necesita, servirlo en
//!   otro puerto interno.

use axum::{extract::Request, middleware::Next, response::Response};

/// Middleware de métricas (stub): mide duración para log futuro y delega.
// Intencionalmente sin cablear: sin backend Prometheus solo añadiría log por
// request. Cablear con `from_fn(track_metrics)` cuando haya dónde exportar.
#[allow(dead_code)]
pub async fn track_metrics(req: Request, next: Next) -> Response {
    let method = req.method().to_string();
    let path = normalize_path(req.uri().path());
    let start = std::time::Instant::now();
    let resp = next.run(req).await;
    tracing::debug!(
        method = %method,
        path = %path,
        status = resp.status().as_u16(),
        elapsed_ms = start.elapsed().as_millis() as u64,
        "request"
    );
    resp
}

/// Normaliza una ruta a su plantilla: UUIDs y segmentos numéricos -> `:id`.
/// p. ej. `/api/v1/users/550e8400-...` -> `/api/v1/users/:id`.
// Usada solo por `track_metrics` (ver arriba).
#[allow(dead_code)]
pub fn normalize_path(path: &str) -> String {
    path.split('/')
        .map(|seg| {
            if seg.is_empty() {
                return String::new();
            }
            if !seg.is_empty() && seg.bytes().all(|b| b.is_ascii_digit()) {
                return ":id".to_string();
            }
            if is_uuid_like(seg) {
                return ":id".to_string();
            }
            seg.to_string()
        })
        .collect::<Vec<_>>()
        .join("/")
}

#[allow(dead_code)]
fn is_uuid_like(s: &str) -> bool {
    if s.len() != 36 {
        return false;
    }
    let b = s.as_bytes();
    if b[8] != b'-' || b[13] != b'-' || b[18] != b'-' || b[23] != b'-' {
        return false;
    }
    for (i, &c) in b.iter().enumerate() {
        if i == 8 || i == 13 || i == 18 || i == 23 {
            continue;
        }
        if !c.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normaliza_uuid_y_enteros() {
        assert_eq!(
            normalize_path("/api/v1/users/550e8400-e29b-41d4-a716-446655440000"),
            "/api/v1/users/:id"
        );
        assert_eq!(normalize_path("/api/v1/users/123"), "/api/v1/users/:id");
        assert_eq!(normalize_path("/api/v1/health"), "/api/v1/health");
    }
}
