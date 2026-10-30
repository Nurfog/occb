//! Licencias por key para módulos DLC (SaaS).
//!
//! - Key en claro formato `occm_<64 hex>` (256 bits); se muestra UNA vez al
//!   emitirla. En DB solo su SHA-256 (reusa `auth::hash_refresh_token`).
//! - `key_prefix` (`occm_` + 4 hex) permite a soporte identificar la licencia
//!   sin exponerla.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::auth::{hash_refresh_token, new_refresh_token};

/// Genera una key de licencia (`occm_<64 hex>`). Devolver UNA vez al cliente.
pub fn new_license_key() -> String {
    format!("occm_{}", new_refresh_token())
}

/// Hash para almacenar (nunca la key en claro).
pub fn hash_license_key(key: &str) -> String {
    hash_refresh_token(key)
}

/// Prefijo de soporte (`occm_` + 4 hex): identifica sin exponer.
pub fn license_prefix(key: &str) -> String {
    key.chars().take(9).collect()
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ModuleInfo {
    pub slug: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub enabled: bool,
    pub route_prefix: String,
    pub licensed: bool,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct License {
    pub id: Uuid,
    pub label: String,
    pub key_prefix: String,
    pub modules: Vec<String>,
    pub seats: Option<i32>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

fn validar_slug(s: &str) -> Result<(), validator::ValidationError> {
    let t = s.trim();
    if t.len() >= 2
        && t.len() <= 40
        && t.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        Ok(())
    } else {
        Err(validator::ValidationError::new("slug_invalido"))
    }
}

#[derive(Debug, Deserialize, Validate)]
pub struct IssueLicenseInput {
    #[validate(length(min = 1, max = 200, message = "etiqueta de 1 a 200 caracteres"))]
    pub label: String,
    /// Slugs del catálogo (`modules`) o `'*'` para plan total.
    pub modules: Vec<String>,
    #[validate(range(min = 1, message = "seats debe ser > 0"))]
    pub seats: Option<i32>,
    /// Días de vigencia desde hoy (vacío = sin expiración).
    #[validate(range(min = 1, max = 3650, message = "vigencia de 1 a 3650 días"))]
    pub expires_in_days: Option<i64>,
    pub metadata: Option<serde_json::Value>,
}

impl IssueLicenseInput {
    pub fn validated_modules(&self) -> Result<Vec<String>, crate::error::AppError> {
        if self.modules.is_empty() || self.modules.len() > 32 {
            return Err(crate::error::AppError::Validation(
                "modules: de 1 a 32 slugs".into(),
            ));
        }
        for m in &self.modules {
            if m != "*" && validar_slug(m).is_err() {
                return Err(crate::error::AppError::Validation(format!(
                    "slug de módulo inválido: {m}"
                )));
            }
        }
        Ok(self.modules.clone())
    }
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateModuleInput {
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_con_prefijo_y_hash_sin_fuga() {
        let key = new_license_key();
        assert!(key.starts_with("occm_"));
        assert_eq!(key.len(), 5 + 64);
        let h = hash_license_key(&key);
        assert!(!h.contains(&key[5..]));
        assert_eq!(license_prefix(&key).len(), 9);
    }

    #[test]
    fn slugs_validos_e_invalidos() {
        assert!(validar_slug("flow").is_ok());
        assert!(validar_slug("mi-modulo2").is_ok());
        assert!(validar_slug("*").is_err());
        assert!(validar_slug("Flow").is_err());
        assert!(validar_slug("x").is_err());
    }
}
