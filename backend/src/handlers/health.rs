//! Liveness (sin dependencias) y versión.

use axum::{Json, extract::State};
use serde_json::{Value, json};

use crate::state::AppState;

pub async fn health(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "status": "ok",
        "env": state.config.env.name(),
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

pub async fn version() -> Json<Value> {
    Json(json!({
        "name": env!("CARGO_PKG_NAME"),
        "version": env!("CARGO_PKG_VERSION"),
    }))
}
