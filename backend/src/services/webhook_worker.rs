//! Worker de entrega de webhooks (despacho HTTP real).
//!
//! - Polling en Postgres cada 5 s (sin Redis): reclama pendientes vencidos
//!   con `FOR UPDATE SKIP LOCKED`, los marca `processing` y los despacha
//!   fuera de la transacción.
//! - Firma `X-Hub-Signature-256: sha256=<hex>` (HMAC-SHA256 del body con el
//!   `secret` del webhook, si tiene). Timeout HTTP 10 s.
//! - Backoff: 60 s × 2^intentos (tope 1 h); tras 5 intentos → `failed`.
//!   Guarda `response_status` + `response_body` (truncado) para diagnóstico.
//! - URLs revalidadas con `is_webhook_url_allowed` (defensa SSRF en
//!   profundidad; ya se exige al crear el webhook).

use hmac::{Hmac, Mac};
use sha2::Sha256;
use sqlx::PgPool;

use crate::handlers::webhooks::is_webhook_url_allowed;

const MAX_ATTEMPTS: i32 = 5;
const POLL_INTERVAL_SECS: u64 = 5;
const HTTP_TIMEOUT_SECS: u64 = 10;

fn is_undefined_table(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(db) if db.code().as_deref() == Some("42P01"))
}

fn backoff_secs(attempts: i32) -> i64 {
    (60_i64.saturating_mul(1 << attempts.min(6))).min(3600)
}

fn sign(secret: &str, body: &[u8]) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC acepta cualquier clave");
    mac.update(body);
    format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
}

#[derive(Debug, sqlx::FromRow)]
struct DueDelivery {
    id: uuid::Uuid,
    event_type: String,
    payload: serde_json::Value,
    attempts: i32,
    url: String,
    secret: Option<String>,
}

/// Bucle del worker: reclama y despacha entregas pendientes vencidas.
pub async fn start_worker(pool: PgPool) {
    tracing::info!("webhook worker iniciado (polling 5 s, timeout HTTP 10 s)");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
        .unwrap_or_else(|e| panic!("no se pudo crear cliente HTTP del worker: {e}"));
    loop {
        match dispatch_due(&pool, &client).await {
            Ok(n) => {
                if n > 0 {
                    tracing::info!("webhook worker: {n} entregas procesadas");
                }
            }
            Err(e) => {
                if is_undefined_table(&e) {
                    tracing::debug!(
                        "tablas de webhooks ausentes; reintentando en el próximo ciclo"
                    );
                } else {
                    tracing::error!("error despachando webhooks: {e}");
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(POLL_INTERVAL_SECS)).await;
    }
}

async fn dispatch_due(pool: &PgPool, client: &reqwest::Client) -> Result<i64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let due: Vec<DueDelivery> = sqlx::query_as::<_, DueDelivery>(
        "SELECT d.id, d.event_type, d.payload, d.attempts, w.url, w.secret
         FROM webhook_deliveries d JOIN webhooks w ON w.id = d.webhook_id
         WHERE d.status = 'pending' AND d.next_attempt_at <= now()
         ORDER BY d.next_attempt_at LIMIT 10 FOR UPDATE OF d SKIP LOCKED",
    )
    .fetch_all(&mut *tx)
    .await?;
    for d in &due {
        sqlx::query("UPDATE webhook_deliveries SET status = 'processing' WHERE id = $1")
            .bind(d.id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;

    for d in &due {
        dispatch_one(pool, client, d).await;
    }
    Ok(due.len() as i64)
}

async fn dispatch_one(pool: &PgPool, client: &reqwest::Client, d: &DueDelivery) {
    let body = d.payload.to_string();
    let attempts = d.attempts + 1;

    if !is_webhook_url_allowed(&d.url) {
        tracing::warn!("webhook {}: URL ya no permitida, se marca failed", d.id);
        let _ = sqlx::query(
            "UPDATE webhook_deliveries SET status = 'failed', attempts = $2,
             response_body = 'URL no permitida (SSRF guard)' WHERE id = $1",
        )
        .bind(d.id)
        .bind(attempts)
        .execute(pool)
        .await;
        return;
    }

    let mut req = client
        .post(&d.url)
        .header("Content-Type", "application/json")
        .header("X-Webhook-Event", &d.event_type)
        .body(body.clone());
    if let Some(secret) = d.secret.as_deref().filter(|s| !s.is_empty()) {
        req = req.header("X-Hub-Signature-256", sign(secret, body.as_bytes()));
    }

    let outcome = req.send().await;
    let (ok, status_code, snippet) = match outcome {
        Ok(resp) => {
            let code = resp.status().as_u16() as i32;
            let text = resp.text().await.unwrap_or_default();
            let mut s: String = text.chars().take(500).collect();
            if text.len() > s.len() {
                s.push('…');
            }
            ((200..300).contains(&code), Some(code), s)
        }
        Err(e) => {
            tracing::warn!("webhook {}: fallo HTTP (intento {attempts}): {e}", d.id);
            (false, None, format!("error HTTP: {e}"))
        }
    };

    if ok {
        let _ = sqlx::query(
            "UPDATE webhook_deliveries SET status = 'success', attempts = $2,
             response_status = $3, response_body = $4 WHERE id = $1",
        )
        .bind(d.id)
        .bind(attempts)
        .bind(status_code)
        .bind(&snippet)
        .execute(pool)
        .await;
    } else if attempts >= MAX_ATTEMPTS {
        let _ = sqlx::query(
            "UPDATE webhook_deliveries SET status = 'failed', attempts = $2,
             response_status = $3, response_body = $4 WHERE id = $1",
        )
        .bind(d.id)
        .bind(attempts)
        .bind(status_code)
        .bind(&snippet)
        .execute(pool)
        .await;
    } else {
        let wait = backoff_secs(attempts);
        let _ = sqlx::query(
            "UPDATE webhook_deliveries SET status = 'pending', attempts = $2,
             response_status = $3, response_body = $4,
             next_attempt_at = now() + ($5::int * interval '1 second') WHERE id = $1",
        )
        .bind(d.id)
        .bind(attempts)
        .bind(status_code)
        .bind(&snippet)
        .bind(wait as i32)
        .execute(pool)
        .await;
    }
}

/// Encola un evento: crea una entrega `pending` por cada webhook activo
/// suscrito. `event` debe ser uno de la lista blanca de `handlers::webhooks`.
/// Best-effort: los handlers lo llaman con `let _ = ...` (nunca falla el request).
pub async fn enqueue_event(
    pool: &PgPool,
    event: &str,
    payload: serde_json::Value,
) -> Result<(), sqlx::Error> {
    let ids: Vec<(uuid::Uuid,)> = match sqlx::query_as(
        "SELECT id FROM webhooks WHERE active = TRUE AND event = $1::webhook_event",
    )
    .bind(event)
    .fetch_all(pool)
    .await
    {
        Ok(ids) => ids,
        Err(e) => {
            if is_undefined_table(&e) {
                tracing::warn!(
                    "webhooks ausentes: evento {event} descartado (migración pendiente)"
                );
                return Ok(());
            }
            // Los callers lo usan best-effort (`let _ =`): dejar rastro.
            tracing::error!("no se pudo encolar evento {event}: {e}");
            return Err(e);
        }
    };
    if ids.is_empty() {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    for (id,) in &ids {
        sqlx::query(
            "INSERT INTO webhook_deliveries (webhook_id, event_type, payload, status)
             VALUES ($1, $2, $3, 'pending')",
        )
        .bind(id)
        .bind(event)
        .bind(payload.clone())
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}
