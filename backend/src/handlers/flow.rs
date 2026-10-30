//! Stub del DLC `flow` (Automatizaciones estilo n8n).
//!
//! - Este archivo es SOLO el punto de anclaje del gating: la ruta
//!   `GET /api/v1/flow/ping` exige licencia del módulo `flow`
//!   (`require_module("flow")` + JWT) y responde sin tocar la DB de flujos.
//! - La implementación real vive en `docs/modules/flow-integration-plan.md`
//!   (schema `flowcraft`, worker, canvas). Al integrarla, este stub se
//!   reemplaza por los handlers reales manteniendo el mismo gating.

use axum::{Extension, Json, response::IntoResponse};
use serde_json::{Value, json};

use crate::{auth::Claims, error::AppError};

/// Ping del DLC flow (JWT + licencia `flow`). 402 si no hay licencia.
pub async fn ping(Extension(_claims): Extension<Claims>) -> Result<impl IntoResponse, AppError> {
    let body: Value = json!({
        "module": "flow",
        "licensed": true,
        "status": "stub: ver docs/modules/flow-integration-plan.md",
    });
    Ok(Json(body))
}
