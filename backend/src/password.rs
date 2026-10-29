//! Contraseñas con Argon2id (recomendación OWASP) + política mínima.
//!
//! - Longitud 12..=128 (cota superior = anti-DoS por hashing gigante).
//! - `verify_password` es tiempo-constante (lo garantiza argon2).

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};

use crate::error::AppError;

pub const MIN_PASSWORD_LEN: usize = 12;
pub const MAX_PASSWORD_LEN: usize = 128;

pub fn validate_password_policy(pw: &str) -> Result<(), AppError> {
    if pw.chars().count() < MIN_PASSWORD_LEN {
        return Err(AppError::Validation(format!(
            "la contraseña debe tener al menos {MIN_PASSWORD_LEN} caracteres"
        )));
    }
    if pw.len() > MAX_PASSWORD_LEN {
        return Err(AppError::Validation(format!(
            "la contraseña debe tener como máximo {MAX_PASSWORD_LEN} caracteres"
        )));
    }
    Ok(())
}

pub fn hash_password(pw: &str) -> Result<String, AppError> {
    validate_password_policy(pw)?;
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(pw.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|_| AppError::Internal)
}

pub fn verify_password(hash: &str, pw: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(pw.as_bytes(), &parsed)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_y_verify_ok() {
        let pw = "correcta-correcta-123";
        let hash = hash_password(pw).expect("hash");
        assert!(verify_password(&hash, pw));
        assert!(!verify_password(&hash, "otra-otra-otra-000"));
        assert!(!verify_password("no-es-un-hash", pw));
    }

    #[test]
    fn politica_rechaza_cortas_y_gigantes() {
        assert!(validate_password_policy("corta123").is_err());
        assert!(validate_password_policy(&"x".repeat(129)).is_err());
        assert!(validate_password_policy("longitud-suficiente-1").is_ok());
    }
}
