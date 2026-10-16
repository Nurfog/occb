//! Cifrado simétrico de tokens OAuth (AES-256-GCM).
//!
//! - Formato: `nonce_hex:ct_hex` (nonce de 96 bits + ciphertext+tag).
//! - Fail-closed: sin clave (`None`) devuelve error salvo en dev, donde el
//!   llamador ya advirtió al arrancar. La clave NUNCA se loguea.

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use rand::RngCore;

/// Cifra `plaintext` con `key` (32 bytes). Devuelve `nonce_hex:ct_hex`.
/// FASE 2: lo usan los handlers de calendar/oauth para guardar tokens.
pub fn encrypt_to_hex(key: &[u8; 32], plaintext: &str) -> Result<String, crate::error::AppError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| {
        tracing::error!("crypto: clave inválida");
        let _ = e;
        crate::error::AppError::Internal
    })?;
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher.encrypt(nonce, plaintext.as_bytes()).map_err(|_| {
        tracing::error!("crypto: fallo de cifrado");
        crate::error::AppError::Internal
    })?;
    Ok(format!("{}:{}", hex::encode(nonce_bytes), hex::encode(ct)))
}

/// Descifra un valor `nonce_hex:ct_hex` con `key`.
/// FASE 2: lo usan los handlers de calendar/oauth para leer tokens.
pub fn decrypt_from_hex(key: &[u8; 32], packed: &str) -> Result<String, crate::error::AppError> {
    let (nonce_hex, ct_hex) = packed.split_once(':').ok_or_else(|| {
        tracing::error!("crypto: formato de token cifrado inválido");
        crate::error::AppError::Internal
    })?;
    let nonce_bytes = hex::decode(nonce_hex).map_err(|_| {
        tracing::error!("crypto: nonce no-hex");
        crate::error::AppError::Internal
    })?;
    let ct = hex::decode(ct_hex).map_err(|_| {
        tracing::error!("crypto: ciphertext no-hex");
        crate::error::AppError::Internal
    })?;
    if nonce_bytes.len() != 12 {
        tracing::error!("crypto: nonce de longitud inválida");
        return Err(crate::error::AppError::Internal);
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| crate::error::AppError::Internal)?;
    let pt = cipher
        .decrypt(Nonce::from_slice(&nonce_bytes), ct.as_ref())
        .map_err(|_| {
            // Clave rotada o dato manipulado: no revelar detalles.
            tracing::error!("crypto: fallo de descifrado (clave o dato inválido)");
            crate::error::AppError::Internal
        })?;
    String::from_utf8(pt).map_err(|_| crate::error::AppError::Internal)
}

/// Resuelve la clave de cifrado desde el estado.
/// Fail-closed fuera de dev: error si no hay clave.
/// FASE 2: la usan los handlers de calendar/oauth.
pub fn require_key(
    key: Option<&[u8; 32]>,
    is_prod: bool,
) -> Result<[u8; 32], crate::error::AppError> {
    match key {
        Some(k) => Ok(*k),
        None => {
            if is_prod {
                tracing::error!("crypto: sin clave de cifrado en prod");
            }
            Err(crate::error::AppError::Unavailable)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> [u8; 32] {
        [0x42; 32]
    }

    #[test]
    fn roundtrip_cifra_y_descifra() {
        let packed = encrypt_to_hex(&test_key(), "refresh-token-secreto").unwrap();
        // Formato nonce_hex:ct_hex.
        let parts: Vec<&str> = packed.split(':').collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].len(), 24); // 12 bytes en hex
        assert_eq!(
            decrypt_from_hex(&test_key(), &packed).unwrap(),
            "refresh-token-secreto"
        );
    }

    #[test]
    fn nonces_aleatorios_no_reutilizan() {
        let a = encrypt_to_hex(&test_key(), "mismo").unwrap();
        let b = encrypt_to_hex(&test_key(), "mismo").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn otra_clave_o_dato_manipulado_falla() {
        let packed = encrypt_to_hex(&test_key(), "secreto").unwrap();
        assert!(decrypt_from_hex(&[0x99; 32], &packed).is_err());
        let mut tampered = packed.clone();
        tampered.push('0');
        assert!(decrypt_from_hex(&test_key(), &tampered).is_err());
        assert!(decrypt_from_hex(&test_key(), "sin-separador").is_err());
    }

    #[test]
    fn require_key_fail_closed_sin_clave() {
        assert!(require_key(None, true).is_err());
        assert!(require_key(None, false).is_err());
        assert_eq!(require_key(Some(&test_key()), true).unwrap(), test_key());
    }
}
