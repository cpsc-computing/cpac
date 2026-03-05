// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Key derivation: HKDF-SHA256 and Argon2id.

use argon2::Argon2;
use cpac_types::{CpacError, CpacResult};
use hkdf::Hkdf;
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::Sha256;

/// Derive a 32-byte key using HKDF-SHA256.
pub fn derive_key_hkdf(ikm: &[u8], salt: &[u8], info: &[u8]) -> CpacResult<[u8; 32]> {
    let hk = Hkdf::<Sha256>::new(Some(salt), ikm);
    let mut okm = [0u8; 32];
    hk.expand(info, &mut okm)
        .map_err(|e| CpacError::Encryption(format!("HKDF expand: {e}")))?;
    Ok(okm)
}

/// Derive a 32-byte key from a password using Argon2id.
pub fn derive_key_argon2(password: &[u8], salt: &[u8]) -> CpacResult<Vec<u8>> {
    // Ensure salt is valid base64 for argon2 crate (at least 16 bytes)
    let salt_padded = if salt.len() < 16 {
        let mut s = vec![0u8; 16];
        s[..salt.len()].copy_from_slice(salt);
        s
    } else {
        salt.to_vec()
    };

    let argon2 = Argon2::default();
    // Use raw hash output instead of PHC string format
    let mut key = vec![0u8; 32];
    argon2
        .hash_password_into(password, &salt_padded, &mut key)
        .map_err(|e| CpacError::Encryption(format!("Argon2: {e}")))?;
    Ok(key)
}

/// Generate a random 16-byte salt.
#[must_use]
pub fn random_salt() -> Vec<u8> {
    let mut salt = vec![0u8; 16];
    OsRng.fill_bytes(&mut salt);
    salt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hkdf_deterministic() {
        let key1 = derive_key_hkdf(b"input", b"salt", b"info").unwrap();
        let key2 = derive_key_hkdf(b"input", b"salt", b"info").unwrap();
        assert_eq!(key1, key2);
    }

    #[test]
    fn hkdf_different_inputs() {
        let key1 = derive_key_hkdf(b"input1", b"salt", b"info").unwrap();
        let key2 = derive_key_hkdf(b"input2", b"salt", b"info").unwrap();
        assert_ne!(key1, key2);
    }

    #[test]
    fn argon2_derives_key() {
        let salt = random_salt();
        let key = derive_key_argon2(b"password", &salt).unwrap();
        assert_eq!(key.len(), 32);
    }

    #[test]
    fn argon2_deterministic_with_same_salt() {
        let salt = vec![0x42u8; 16];
        let key1 = derive_key_argon2(b"pass", &salt).unwrap();
        let key2 = derive_key_argon2(b"pass", &salt).unwrap();
        assert_eq!(key1, key2);
    }
}
