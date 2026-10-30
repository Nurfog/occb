//! Tenants (empresas clientes SaaS) y alta de sus admins.
//!
//! - El root instalado es superadmin (`users.is_superadmin`, tenant NULL).
//! - Cada empresa es un tenant `pendiente` -> `activa` <-> `suspendida`.
//! - Tenant suspendido = sus usuarios no entran (ver `auth_middleware`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::{Validate, ValidationError};

pub const TENANT_ESTADOS: &[&str] = &["pendiente", "activa", "suspendida"];

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Tenant {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub status: String,
    pub contact_name: Option<String>,
    pub contact_email: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct TenantWithCounts {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub status: String,
    pub contact_name: Option<String>,
    pub contact_email: Option<String>,
    pub users: i64,
    pub created_at: DateTime<Utc>,
}

fn validar_slug(s: &str) -> Result<(), ValidationError> {
    let t = s.trim();
    if t.len() >= 2
        && t.len() <= 40
        && t.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        Ok(())
    } else {
        Err(ValidationError::new("slug_invalido"))
    }
}

fn validar_estado(s: &str) -> Result<(), ValidationError> {
    if TENANT_ESTADOS.contains(&s) {
        Ok(())
    } else {
        Err(ValidationError::new("estado_invalido"))
    }
}

/// Transiciones legales de estado. Toda otra -> 422.
pub fn puede_transitar(de: &str, a: &str) -> bool {
    matches!(
        (de, a),
        ("pendiente", "activa")
            | ("pendiente", "suspendida")
            | ("activa", "suspendida")
            | ("suspendida", "activa")
    )
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateTenantInput {
    #[validate(length(min = 1, max = 200, message = "nombre de 1 a 200 caracteres"))]
    pub name: String,
    /// Slug opcional (si falta se deriva del nombre).
    #[validate(custom(function = "validar_slug_opt"))]
    pub slug: Option<String>,
    #[validate(length(max = 200))]
    pub contact_name: Option<String>,
    #[validate(email(message = "email inválido"))]
    pub contact_email: Option<String>,
}

fn validar_slug_opt(s: &str) -> Result<(), ValidationError> {
    validar_slug(s)
}

impl CreateTenantInput {
    /// Slug explícito o derivado del nombre (minúsculas, no alfanum -> `-`).
    pub fn slug_or_derived(&self) -> String {
        if let Some(s) = self
            .slug
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return s.to_lowercase();
        }
        let mut slug: String = self
            .name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        while slug.contains("--") {
            slug = slug.replace("--", "-");
        }
        slug.trim_matches('-').to_string()
    }
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateTenantInput {
    /// Nuevo estado (`activa`/`suspendida`; ver `puede_transitar`).
    #[validate(custom(function = "validar_estado"))]
    pub status: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateTenantAdminInput {
    #[validate(email(message = "email inválido"), length(max = 320))]
    pub email: String,
    #[validate(length(min = 1, max = 100, message = "nombre de 1 a 100 caracteres"))]
    pub name: String,
    /// Política 12..128 en `password::hash_password`.
    #[validate(length(min = 1, max = 128))]
    pub password: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transiciones_tenant() {
        assert!(puede_transitar("pendiente", "activa"));
        assert!(puede_transitar("activa", "suspendida"));
        assert!(puede_transitar("suspendida", "activa"));
        assert!(!puede_transitar("activa", "pendiente"));
        assert!(!puede_transitar("suspendida", "pendiente"));
        assert!(!puede_transitar("pendiente", "pendiente"));
    }

    #[test]
    fn slug_derivado_del_nombre() {
        let i = CreateTenantInput {
            name: "Colegio San Pedro 2026".into(),
            slug: None,
            contact_name: None,
            contact_email: None,
        };
        assert_eq!(i.slug_or_derived(), "colegio-san-pedro-2026");
    }
}
