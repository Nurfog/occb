//! Paginación con listas blancas de orden (grupo A, port origen).
//!
//! `PaginationParams` es el formato del origen (`page/per_page/search/`
//! `sort_by/sort_order`). `sort_column`/`sort_direction` solo devuelven
//! constantes de la lista blanca de contactos; otros handlers usan sus
//! propias funciones `normalize_*_sort` locales (nunca interpolar input).

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
    pub search: Option<String>,
    pub sort_by: Option<String>,
    pub sort_order: Option<String>,
}

impl PaginationParams {
    pub fn page(&self) -> i64 {
        self.page.unwrap_or(1).clamp(1, 10000)
    }

    pub fn per_page(&self) -> i64 {
        self.per_page.unwrap_or(20).clamp(1, 200)
    }

    pub fn offset(&self) -> i64 {
        (self.page() - 1) * self.per_page()
    }

    /// Lista blanca de columnas ordenables (tabla `contacts`).
    pub fn sort_column(&self) -> &str {
        match self.sort_by.as_deref() {
            Some("name") | Some("first_name") => "first_name",
            Some("last_name") => "last_name",
            Some("email") => "email",
            Some("phone") => "phone",
            Some("position") => "position",
            Some("created_at") => "created_at",
            _ => "created_at",
        }
    }

    pub fn sort_direction(&self) -> &str {
        match self.sort_order.as_deref() {
            Some("asc") => "ASC",
            _ => "DESC",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
    pub total_pages: i64,
}

impl<T> PaginatedResponse<T> {
    pub fn new(data: Vec<T>, total: i64, page: i64, per_page: i64) -> Self {
        let total_pages = if per_page > 0 {
            (total + per_page - 1) / per_page
        } else {
            1
        };
        PaginatedResponse {
            data,
            total,
            page,
            per_page,
            total_pages,
        }
    }
}
