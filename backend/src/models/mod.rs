//! Modelos + validación de inputs (primera barrera anti-inyección).
//!
//! Reglas SQL (aplicadas en handlers):
//! 1. Valores SIEMPRE con binds `$1, $2...` — jamás `format!` con input.
//! 2. Identificadores dinámicos (ORDER BY) SOLO desde listas blancas.
//! 3. LIKE con `escape_like` + `ESCAPE '\'`.
//! 4. LIMIT/OFFSET como enteros acotados y bindeados.
//!
//! Submódulos CRM (grupo A): activity, company, contact, deal, pagination,
//! csv, date_flex. Nota: `Deal.value` es `f64` (sin `rust_decimal` en la
//! plantilla); el SQL usa `value::float8` en SELECT/RETURNING y agregados.

pub mod activity;
pub mod company;
pub mod contact;
pub mod csv;
pub mod date_flex;
pub mod deal;
// Submódulos CRM (grupo A): activity, company, contact, deal, pagination,
// csv, date_flex. Nota: `Deal.value` es `f64` (sin `rust_decimal` en la
// plantilla); el SQL usa `value::float8` en SELECT/RETURNING y agregados.
//
// Submódulos CRM (grupo B): lead, tag, audit, document, notification, report.
pub mod audit;
pub mod calendar;
pub mod document;
pub mod email;
pub mod lead;
pub mod notification;
pub mod pagination;
pub mod report;
pub mod tag;

pub use activity::{Activity, CreateActivity, RecurrenceType, UpdateActivity};
pub use calendar::{
    CalendarConnectionStatus, CalendarEvent, CalendarQuery, CreateCalendarEvent,
    UpdateCalendarEvent,
};
pub use company::{Company, CreateCompany, UpdateCompany};
pub use contact::{Contact, ContactFilters, CreateContact, UpdateContact};
pub use csv::{ImportResult, escape_csv, parse_csv_rows, parse_deal_import_row};
pub use deal::{CreateDeal, Deal, DealStage, UpdateDeal, UpdateDealStage};
pub use email::{CreateEmailTemplate, EmailLog, EmailTemplate, SendEmail, UpdateEmailTemplate};
pub use pagination::{PaginatedResponse, PaginationParams};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Deserialize, Validate)]
pub struct RegisterInput {
    #[validate(email(message = "email inválido"), length(max = 320))]
    pub email: String,
    #[validate(length(min = 1, max = 100, message = "nombre de 1 a 100 caracteres"))]
    pub name: String,
    /// La política (12..128) la aplica `password::hash_password`.
    pub password: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct LoginInput {
    #[validate(email(message = "email inválido"), length(max = 320))]
    pub email: String,
    #[validate(length(min = 1, max = 128))]
    pub password: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct RefreshInput {
    #[validate(length(min = 32, max = 128))]
    pub refresh_token: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct UserPublic {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub page: Option<u32>,
    pub per_page: Option<u32>,
    pub sort: Option<String>,
    pub dir: Option<String>,
    pub q: Option<String>,
}

/// Normaliza paginación/orden a valores seguros (lista blanca).
pub fn normalize_list(q: &ListQuery) -> (i64, i64, &'static str, &'static str) {
    let page = q.page.unwrap_or(1).clamp(1, 10_000) as i64;
    let per_page = q.per_page.unwrap_or(20).clamp(1, 100) as i64;
    let sort: &'static str = match q.sort.as_deref().unwrap_or("created_at") {
        "email" => "email",
        "name" => "name",
        "created_at" => "created_at",
        _ => "created_at",
    };
    let dir: &'static str = match q.dir.as_deref().unwrap_or("desc") {
        "asc" => "ASC",
        _ => "DESC",
    };
    (page, per_page, sort, dir)
}

/// Escapa `%`, `_` y `\` para búsquedas LIKE seguras.
pub fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paginacion_acotada_y_sort_con_lista_blanca() {
        let q = ListQuery {
            page: Some(0),
            per_page: Some(9999),
            sort: Some("password_hash".into()),
            dir: Some("nonsense".into()),
            q: None,
        };
        let (page, per_page, sort, dir) = normalize_list(&q);
        assert_eq!((page, per_page), (1, 100));
        assert_eq!(sort, "created_at"); // inyección de columna neutralizada
        assert_eq!(dir, "DESC");
    }

    #[test]
    fn escape_like_neutraliza_comodines() {
        assert_eq!(escape_like("100%_\\x"), "100\\%\\_\\\\x");
    }
}
