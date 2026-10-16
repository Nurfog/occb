//! Grupo C (port) — modelos de calendar.
//!
//! Los callbacks/sync OAuth devuelven 503 en este destino (sin reqwest/oauth/
//! http_client ni cifrado); el CRUD local de `calendar_events` es SQL puro.
//! TODO port: requiere OAuth (google/microsoft) + AES-GCM + HTTP.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct CalendarToken {
    pub id: Uuid,
    pub user_id: Uuid,
    pub provider: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub calendar_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct CalendarEvent {
    pub id: Uuid,
    pub user_id: Uuid,
    pub provider: String,
    pub external_id: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub all_day: bool,
    pub attendees: Option<serde_json::Value>,
    pub entity_type: Option<String>,
    pub entity_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateCalendarEvent {
    pub title: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub all_day: Option<bool>,
    pub attendees: Option<Vec<String>>,
    pub entity_type: Option<String>,
    pub entity_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateCalendarEvent {
    pub title: Option<String>,
    pub description: Option<String>,
    pub location: Option<String>,
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
    pub all_day: Option<bool>,
    pub attendees: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct CalendarQuery {
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub provider: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CalendarConnectionStatus {
    pub google: bool,
    pub microsoft: bool,
}
