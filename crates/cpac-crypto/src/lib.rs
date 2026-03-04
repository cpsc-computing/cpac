// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Cryptographic primitives for CPAC.
//!
//! Provides AEAD encryption (ChaCha20-Poly1305, AES-256-GCM),
//! key exchange (X25519), digital signatures (Ed25519),
//! key derivation (HKDF, Argon2), and a high-level encrypt/decrypt API.

#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::cast_possible_truncation,
    clippy::unnecessary_wraps,
)]

pub mod aead;
#[cfg(feature = "pqc")]
pub mod hybrid;
pub mod kdf;
pub mod keys;
#[cfg(feature = "pqc")]
pub mod pqc;
pub mod sign;

pub use aead::{decrypt_aead, encrypt_aead, AeadAlgorithm};
pub use kdf::{derive_key_argon2, derive_key_hkdf};
pub use keys::{generate_x25519_keypair, x25519_shared_secret, X25519KeyPair};
pub use sign::{ed25519_sign, ed25519_verify, generate_ed25519_keypair, Ed25519KeyPair};

/// Encrypt data with a password using Argon2 key derivation + AEAD.
///
/// Returns `(salt, nonce, ciphertext)`.
pub fn encrypt_with_password(
    data: &[u8],
    password: &[u8],
    algo: AeadAlgorithm,
) -> cpac_types::CpacResult<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    let salt = kdf::random_salt();
    let key = derive_key_argon2(password, &salt)?;
    let (nonce, ciphertext) = encrypt_aead(data, &key, algo)?;
    Ok((salt, nonce, ciphertext))
}

/// Decrypt data with a password.
pub fn decrypt_with_password(
    ciphertext: &[u8],
    password: &[u8],
    salt: &[u8],
    nonce: &[u8],
    algo: AeadAlgorithm,
) -> cpac_types::CpacResult<Vec<u8>> {
    let key = derive_key_argon2(password, salt)?;
    decrypt_aead(ciphertext, &key, nonce, algo)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_roundtrip_chacha() {
        let data = b"secret message for CPAC encryption test";
        let password = b"hunter2";
        let (salt, nonce, ct) =
            encrypt_with_password(data, password, AeadAlgorithm::ChaCha20Poly1305).unwrap();
        let pt = decrypt_with_password(
            &ct,
            password,
            &salt,
            &nonce,
            AeadAlgorithm::ChaCha20Poly1305,
        )
        .unwrap();
        assert_eq!(pt, data);
    }

    #[test]
    fn password_roundtrip_aes() {
        let data = b"another secret for AES-GCM";
        let password = b"correcthorsebatterystaple";
        let (salt, nonce, ct) =
            encrypt_with_password(data, password, AeadAlgorithm::Aes256Gcm).unwrap();
        let pt =
            decrypt_with_password(&ct, password, &salt, &nonce, AeadAlgorithm::Aes256Gcm).unwrap();
        assert_eq!(pt, data);
    }

    #[test]
    fn wrong_password_fails() {
        let data = b"cannot read this";
        let (salt, nonce, ct) =
            encrypt_with_password(data, b"right", AeadAlgorithm::ChaCha20Poly1305).unwrap();
        let result = decrypt_with_password(
            &ct,
            b"wrong",
            &salt,
            &nonce,
            AeadAlgorithm::ChaCha20Poly1305,
        );
        assert!(result.is_err());
    }
}
