//! Modelos de derechos de titulares Ley 21.719 (Chile).
//!
//! - `PrivacyRequestType` / `PrivacyRequestStatus` mapean los enums Postgres
//!   `privacy_request_type` / `privacy_request_status` (snake_case, igual que
//!   `LeadStatus` en `lead.rs`).
//! - Los inputs usan `validator`; el SQL de los handlers solo bindea valores.
//! - `PrivacyRequest.r#type` mapea la columna `type` (igual que
//!   `LeadActivity.r#type`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::{Validate, ValidationError};

/// Tipos de solicitud (art. 12 y ss. Ley 21.719).
#[derive(Debug, Serialize, Deserialize, sqlx::Type, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "privacy_request_type", rename_all = "snake_case")]
pub enum PrivacyRequestType {
    Acceso,
    Rectificacion,
    Supresion,
    Oposicion,
    Portabilidad,
    Bloqueo,
}

impl PrivacyRequestType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Acceso => "acceso",
            Self::Rectificacion => "rectificacion",
            Self::Supresion => "supresion",
            Self::Oposicion => "oposicion",
            Self::Portabilidad => "portabilidad",
            Self::Bloqueo => "bloqueo",
        }
    }
}

/// Estados del flujo (máquina estricta en handlers).
#[derive(Debug, Serialize, Deserialize, sqlx::Type, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "privacy_request_status", rename_all = "snake_case")]
pub enum PrivacyRequestStatus {
    Recibida,
    Verificada,
    #[serde(rename = "en_curso")]
    EnCurso,
    Cumplida,
    Denegada,
    Desistida,
}

impl PrivacyRequestStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Recibida => "recibida",
            Self::Verificada => "verificada",
            Self::EnCurso => "en_curso",
            Self::Cumplida => "cumplida",
            Self::Denegada => "denegada",
            Self::Desistida => "desistida",
        }
    }
}

/// Lista blanca de tipos (inputs y filtros).
pub const TIPOS_VALIDOS: &[&str] = &[
    "acceso",
    "rectificacion",
    "supresion",
    "oposicion",
    "portabilidad",
    "bloqueo",
];

/// Lista blanca de estados (filtros).
pub const ESTADOS_VALIDOS: &[&str] = &[
    "recibida",
    "verificada",
    "en_curso",
    "cumplida",
    "denegada",
    "desistida",
];

/// Quién presenta la solicitud (los NNA actúan por apoderado/representante).
pub const RELACIONES_VALIDAS: &[&str] = &["titular", "apoderado", "representante", "otro"];

/// Lista blanca de acciones del staff.
pub const ACCIONES_VALIDAS: &[&str] = &[
    "verificar",
    "admitir",
    "prorrogar",
    "resolver-cumplida",
    "resolver-denegada",
    "desistir",
    "levantar_bloqueo",
];

/// Columnas rectificables (únicas permitidas en `corrections`).
pub const RECTIFICACION_WHITELIST: &[&str] = &[
    "first_name",
    "last_name",
    "email",
    "phone",
    "position",
    "notes",
];

fn validar_tipo(s: &str) -> Result<(), ValidationError> {
    if TIPOS_VALIDOS.contains(&s.trim().to_lowercase().as_str()) {
        Ok(())
    } else {
        Err(ValidationError::new("tipo_invalido"))
    }
}

fn validar_tipo_opt(s: &str) -> Result<(), ValidationError> {
    validar_tipo(s)
}

fn validar_estado_opt(s: &str) -> Result<(), ValidationError> {
    if ESTADOS_VALIDOS.contains(&s.trim().to_lowercase().as_str()) {
        Ok(())
    } else {
        Err(ValidationError::new("estado_invalido"))
    }
}

pub(crate) fn validar_relacion_opt(s: &str) -> Result<(), ValidationError> {
    if RELACIONES_VALIDAS.contains(&s.trim().to_lowercase().as_str()) {
        Ok(())
    } else {
        Err(ValidationError::new("relacion_invalida"))
    }
}

fn validar_accion(s: &str) -> Result<(), ValidationError> {
    if ACCIONES_VALIDAS.contains(&s.trim().to_lowercase().as_str()) {
        Ok(())
    } else {
        Err(ValidationError::new("accion_invalida"))
    }
}

/// Fila completa de `privacy_requests` (sin `SELECT *` en handlers).
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow, Clone)]
pub struct PrivacyRequest {
    pub id: Uuid,
    pub contact_id: Option<Uuid>,
    pub email: String,
    pub r#type: PrivacyRequestType,
    pub status: PrivacyRequestStatus,
    pub motivo: Option<String>,
    pub identity_verified: bool,
    pub verify_token_hash: Option<String>,
    pub verify_expires_at: Option<DateTime<Utc>>,
    pub due_at: DateTime<Utc>,
    pub extended: bool,
    pub extended_at: Option<DateTime<Utc>>,
    pub corrections: Option<serde_json::Value>,
    pub legal_hold: Option<String>,
    pub solicitante_nombre: Option<String>,
    pub relacion: String,
    pub documento_ref: Option<String>,
    pub resolution: Option<String>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Fila de listado con `overdue` calculado en SQL.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PrivacyRequestListItem {
    pub id: Uuid,
    pub contact_id: Option<Uuid>,
    pub email: String,
    pub r#type: PrivacyRequestType,
    pub status: PrivacyRequestStatus,
    pub motivo: Option<String>,
    pub identity_verified: bool,
    pub due_at: DateTime<Utc>,
    pub extended: bool,
    pub created_at: DateTime<Utc>,
    pub overdue: bool,
}

/// Entrada pública de solicitud (sin auth).
#[derive(Debug, Deserialize, Validate)]
pub struct CreatePrivacyRequest {
    #[validate(email(message = "email inválido"), length(max = 320))]
    pub email: String,
    #[validate(length(min = 1, max = 30), custom(function = "validar_tipo"))]
    pub request_type: String,
    #[validate(length(max = 2000, message = "motivo max 2000 caracteres"))]
    pub motivo: Option<String>,
    pub contact_id: Option<Uuid>,
    pub corrections: Option<serde_json::Value>,
    pub con_bloqueo: Option<bool>,
    /// Quién solicita (NNA actúan por apoderado/representante).
    #[validate(length(max = 200, message = "solicitante max 200 caracteres"))]
    pub solicitante_nombre: Option<String>,
    #[validate(custom(function = "validar_relacion_opt"))]
    pub relacion: Option<String>,
    /// Referencia documental de la representación (poder, cédula apoderado).
    #[validate(length(max = 200, message = "documento max 200 caracteres"))]
    pub documento_ref: Option<String>,
}

/// Verificación pública de identidad (token opaco de 64 hex).
#[derive(Debug, Deserialize, Validate)]
pub struct VerifyPrivacyRequest {
    pub request_id: Uuid,
    #[validate(length(min = 32, max = 128))]
    pub token: String,
}

/// Filtros + paginación del listado staff.
#[derive(Debug, Deserialize, Validate, Default)]
pub struct PrivacyQuery {
    #[validate(custom(function = "validar_estado_opt"))]
    pub status: Option<String>,
    #[serde(default, alias = "type")]
    #[validate(custom(function = "validar_tipo_opt"))]
    pub request_type: Option<String>,
    #[validate(length(max = 320))]
    pub email: Option<String>,
    pub overdue: Option<bool>,
    #[validate(length(max = 200))]
    pub q: Option<String>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
    #[validate(length(max = 20))]
    pub sort: Option<String>,
    #[validate(length(max = 4))]
    pub dir: Option<String>,
}

impl PrivacyQuery {
    pub fn page_clamped(&self) -> i64 {
        self.page.unwrap_or(1).clamp(1, 10_000)
    }
    pub fn per_page_clamped(&self) -> i64 {
        self.per_page.unwrap_or(20).clamp(1, 100)
    }
    pub fn offset(&self) -> i64 {
        (self.page_clamped() - 1) * self.per_page_clamped()
    }
    /// Solo `created_at` | `due_at` (el `format!` nunca recibe input).
    pub fn sort_column(&self) -> &'static str {
        match self.sort.as_deref() {
            Some("due_at") => "due_at",
            _ => "created_at",
        }
    }
    pub fn sort_direction(&self) -> &'static str {
        match self.dir.as_deref() {
            Some("asc") => "ASC",
            _ => "DESC",
        }
    }
}

/// Acción del staff sobre una solicitud.
#[derive(Debug, Deserialize, Validate)]
pub struct UpdatePrivacyRequest {
    #[validate(length(min = 1, max = 30), custom(function = "validar_accion"))]
    pub accion: String,
    #[validate(length(max = 2000, message = "resolution max 2000 caracteres"))]
    pub resolution: Option<String>,
    #[validate(length(max = 2000, message = "legal_hold max 2000 caracteres"))]
    pub legal_hold: Option<String>,
}

/// Decisión de consentimiento (finalidades opcionales, opt-in).
/// Cada POST crea una fila: el historial es la evidencia (la vigente es la
/// última por `created_at`; revocar = registrar todo-falso).
#[derive(Debug, Deserialize, Validate)]
pub struct RecordConsentInput {
    pub marketing: bool,
    pub analytics: bool,
    #[validate(range(min = 1, max = 99))]
    pub version: i32,
    /// Origen de la decisión (`banner`, `registro`, `preferencias`).
    #[validate(length(min = 1, max = 20))]
    pub source: Option<String>,
}

/// Fila de `privacy_consents` para el frontend (`GET latest`).
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ConsentRecord {
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    pub marketing: bool,
    pub analytics: bool,
    pub version: i32,
    pub source: String,
    pub created_at: DateTime<Utc>,
}
