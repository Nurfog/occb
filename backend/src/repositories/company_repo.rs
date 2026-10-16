//! Lista blanca de orden para `companies` (port origen adaptado).
//!
//! El origen reutilizaba la lista blanca de contactos (bug:
//! `ORDER BY first_name` no existe en `companies`). Aquí hay lista blanca
//! propia. El CRUD vive en `handlers/companies.rs` con SQL directo; este
//! módulo solo expone el normalizador de orden.

/// Lista blanca de columnas ordenables de `companies` (constantes; seguro
/// interpolar en ORDER BY). Todo lo demás cae a `created_at`.
pub fn normalize_company_sort(
    sort_by: Option<&str>,
    sort_order: Option<&str>,
) -> (&'static str, &'static str) {
    let col = match sort_by {
        Some("name") => "name",
        Some("industry") => "industry",
        Some("city") => "city",
        Some("country") => "country",
        Some("created_at") => "created_at",
        _ => "created_at",
    };
    let dir = match sort_order {
        Some("asc") => "ASC",
        _ => "DESC",
    };
    (col, dir)
}
