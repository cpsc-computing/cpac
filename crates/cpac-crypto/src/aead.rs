// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! AEAD encryption: ChaCha20-Poly1305 and AES-256-GCM.

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce as AesNonce};
use chacha20poly1305::{ChaCha20Poly1305, Nonce as ChachaNonce};
use cpac_types::{CpacError, CpacResult};
use rand::RngCore;
/// Supported AEAD algorithms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AeadAlgorithm {
    ChaCha20Poly1305,
    Aes256Gcm,
}

impl AeadAlgorithm {
    /// Wire ID for this algorithm.
    #[must_use] 
    pub fn id(self) -> u8 {
        match self {
            AeadAlgorithm::ChaCha20Poly1305 => 1,
            AeadAlgorithm::Aes256Gcm => 2,
        }
    }

    /// Decode from wire ID.
    pub fn from_id(id: u8) -> CpacResult<Self> {
        match id {
            1 => Ok(AeadAlgorithm::ChaCha20Poly1305),
            2 => Ok(AeadAlgorithm::Aes256Gcm),
            _ => Err(CpacError::Encryption(format!(
                "unknown AEAD algorithm: {id}"
            ))),
        }
    }

    /// Nonce size in bytes.
    #[must_use] 
    pub fn nonce_size(self) -> usize {
        12 // Both use 96-bit nonces
    }
}

/// Encrypt data using the specified AEAD algorithm.
///
/// Returns `(nonce, ciphertext)`. The key must be 32 bytes.
pub fn encrypt_aead(
    plaintext: &[u8],
    key: &[u8],
    algo: AeadAlgorithm,
) -> CpacResult<(Vec<u8>, Vec<u8>)> {
    if key.len() != 32 {
        return Err(CpacError::Encryption(format!(
            "key must be 32 bytes, got {}",
            key.len()
        )));
    }

    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    let ciphertext = match algo {
        AeadAlgorithm::ChaCha20Poly1305 => {
            let cipher = ChaCha20Poly1305::new(key.into());
            let nonce = ChachaNonce::from_slice(&nonce_bytes);
            cipher
                .encrypt(nonce, plaintext)
                .map_err(|e| CpacError::Encryption(format!("ChaCha20 encrypt: {e}")))?
        }
        AeadAlgorithm::Aes256Gcm => {
            let cipher = Aes256Gcm::new(key.into());
            let nonce = AesNonce::from_slice(&nonce_bytes);
            cipher
                .encrypt(nonce, plaintext)
                .map_err(|e| CpacError::Encryption(format!("AES-GCM encrypt: {e}")))?
        }
    };

    Ok((nonce_bytes.to_vec(), ciphertext))
}

/// Decrypt data using the specified AEAD algorithm.
pub fn decrypt_aead(
    ciphertext: &[u8],
    key: &[u8],
    nonce: &[u8],
    algo: AeadAlgorithm,
) -> CpacResult<Vec<u8>> {
    if key.len() != 32 {
        return Err(CpacError::Encryption(format!(
            "key must be 32 bytes, got {}",
            key.len()
        )));
    }
    if nonce.len() != 12 {
        return Err(CpacError::Encryption(format!(
            "nonce must be 12 bytes, got {}",
            nonce.len()
        )));
    }

    match algo {
        AeadAlgorithm::ChaCha20Poly1305 => {
            let cipher = ChaCha20Poly1305::new(key.into());
            let n = ChachaNonce::from_slice(nonce);
            cipher
                .decrypt(n, ciphertext)
                .map_err(|e| CpacError::Encryption(format!("ChaCha20 decrypt: {e}")))
        }
        AeadAlgorithm::Aes256Gcm => {
            let cipher = Aes256Gcm::new(key.into());
            let n = AesNonce::from_slice(nonce);
            cipher
                .decrypt(n, ciphertext)
                .map_err(|e| CpacError::Encryption(format!("AES-GCM decrypt: {e}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chacha_roundtrip() {
        let key = [0x42u8; 32];
        let plaintext = b"Hello, CPAC crypto!";
        let (nonce, ct) = encrypt_aead(plaintext, &key, AeadAlgorithm::ChaCha20Poly1305).unwrap();
        let pt = decrypt_aead(&ct, &key, &nonce, AeadAlgorithm::ChaCha20Poly1305).unwrap();
        assert_eq!(pt, plaintext);
    }

    #[test]
    fn aes_roundtrip() {
        let key = [0x55u8; 32];
        let plaintext = b"AES-256-GCM test data";
        let (nonce, ct) = encrypt_aead(plaintext, &key, AeadAlgorithm::Aes256Gcm).unwrap();
        let pt = decrypt_aead(&ct, &key, &nonce, AeadAlgorithm::Aes256Gcm).unwrap();
        assert_eq!(pt, plaintext);
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let key = [0x42u8; 32];
        let (nonce, mut ct) = encrypt_aead(b"data", &key, AeadAlgorithm::ChaCha20Poly1305).unwrap();
        ct[0] ^= 0xFF; // tamper
        assert!(decrypt_aead(&ct, &key, &nonce, AeadAlgorithm::ChaCha20Poly1305).is_err());
    }
}
