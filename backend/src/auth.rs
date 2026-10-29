//! JWT de acceso (HS256 explícito) + refresh tokens opacos.
//!
//! - Access: JWT corto (15 min), claims `typ=access`, `iss`/`aud` validados,
//!   algoritmo fijado a HS256 (sin `none`, sin confusión de algoritmos).
//! - Refresh: token aleatorio de 256 bits; en DB solo su SHA-256, con rotación
//!   (cada uso revoca el anterior) y detección de reuso.

use chrono::Utc;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{config::Config, error::AppError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,
    pub exp: i64,
    pub iat: i64,
    pub jti: Uuid,
    /// "access" (los refresh no son JWT).
    pub typ: String,
    pub iss: String,
    pub aud: String,
}

pub fn create_access_token(user_id: Uuid, cfg: &Config) -> Result<String, AppError> {
    let now = Utc::now().timestamp();
    let claims = Claims {
        sub: user_id,
        iat: now,
        exp: now + cfg.access_token_ttl_minutes * 60,
        jti: Uuid::new_v4(),
        typ: "access".to_string(),
        iss: cfg.jwt_issuer.clone(),
        aud: cfg.jwt_audience.clone(),
    };
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(cfg.jwt_secret.as_bytes()),
    )
    .map_err(|_| AppError::Internal)
}

pub fn verify_access_token(token: &str, cfg: &Config) -> Result<Claims, AppError> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.leeway = 30;
    validation.set_issuer(std::slice::from_ref(&cfg.jwt_issuer));
    validation.set_audience(std::slice::from_ref(&cfg.jwt_audience));
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(cfg.jwt_secret.as_bytes()),
        &validation,
    )
    .map_err(|_| AppError::Unauthorized)?;
    if data.claims.typ != "access" {
        return Err(AppError::Unauthorized);
    }
    Ok(data.claims)
}

/// Genera un refresh token opaco (256 bits, hex). Devuélvelo UNA vez al cliente.
pub fn new_refresh_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 hex para almacenar (nunca el token en claro).
pub fn hash_refresh_token(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> Config {
        Config {
            env: crate::config::AppEnv::Development,
            host: "127.0.0.1".into(),
            port: 8000,
            database_url: "postgres://u:p@h/db".into(),
            redis_url: None,
            jwt_secret: "secreto-de-prueba-con-32-o-mas-chars".into(),
            jwt_issuer: "occb-test".into(),
            jwt_audience: "occb-front-test".into(),
            access_token_ttl_minutes: 15,
            refresh_token_ttl_days: 30,
            cors_origins: vec![],
            frontend_url: "http://localhost:3000".into(),
            body_limit_bytes: 1024 * 1024,
            rate_limit_per_minute: 120,
            trusted_proxy: false,
        }
    }

    #[test]
    fn access_roundtrip_y_rechaza_otro_tipo() {
        let cfg = test_config();
        let tok = create_access_token(Uuid::new_v4(), &cfg).unwrap();
        let claims = verify_access_token(&tok, &cfg).unwrap();
        assert_eq!(claims.typ, "access");
        assert_eq!(claims.iss, "occb-test");
        // Token manipulado / otro secreto -> Unauthorized (sin detalle).
        assert!(verify_access_token("manipulado", &cfg).is_err());
    }

    #[test]
    fn refresh_es_aleatorio_y_hashea() {
        let a = new_refresh_token();
        let b = new_refresh_token();
        assert_ne!(a, b);
        assert_eq!(a.len(), 64);
        assert!(!hash_refresh_token(&a).contains(&a));
        assert_eq!(hash_refresh_token(&a), hash_refresh_token(&a));
    }
}
