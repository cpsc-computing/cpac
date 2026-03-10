// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! HQC (Hamming Quasi-Cyclic) key encapsulation — NIST PQC Round 4 selection.
//!
//! **EXPERIMENTAL** — HQC was selected by NIST in March 2025 for future
//! standardisation, but the final standard (expected ~2027) is not yet
//! published.  Use behind the `hqc` feature flag for forward-looking
//! evaluation only.
//!
//! Provides HQC-128, HQC-192, and HQC-256 key encapsulation via the
//! `pqcrypto-hqc` crate (PQClean C bindings).

use cpac_types::{CpacError, CpacResult};

/// HQC security level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HqcLevel {
    /// HQC-128 (NIST Level 1, ~AES-128 equivalent).
    Hqc128,
    /// HQC-192 (NIST Level 3, ~AES-192 equivalent).
    Hqc192,
    /// HQC-256 (NIST Level 5, ~AES-256 equivalent).
    Hqc256,
}

impl HqcLevel {
    /// Wire ID.
    #[must_use]
    pub fn id(self) -> u8 {
        match self {
            HqcLevel::Hqc128 => 30,
            HqcLevel::Hqc192 => 31,
            HqcLevel::Hqc256 => 32,
        }
    }

    /// Decode from wire ID.
    pub fn from_id(id: u8) -> CpacResult<Self> {
        match id {
            30 => Ok(HqcLevel::Hqc128),
            31 => Ok(HqcLevel::Hqc192),
            32 => Ok(HqcLevel::Hqc256),
            _ => Err(CpacError::Encryption(format!(
                "unknown HQC level ID: {id}"
            ))),
        }
    }

    /// Human-readable label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            HqcLevel::Hqc128 => "HQC-128",
            HqcLevel::Hqc192 => "HQC-192",
            HqcLevel::Hqc256 => "HQC-256",
        }
    }
}

/// An HQC key pair.
#[derive(Clone)]
pub struct HqcKeyPair {
    pub level: HqcLevel,
    pub public_key: Vec<u8>,
    pub secret_key: Vec<u8>,
}

/// Generate an HQC keypair.
pub fn hqc_keygen(level: HqcLevel) -> CpacResult<HqcKeyPair> {
    use pqcrypto_traits::kem::*;
    match level {
        HqcLevel::Hqc128 => {
            let (pk, sk) = pqcrypto_hqc::hqc128::keypair();
            Ok(HqcKeyPair {
                level,
                public_key: pk.as_bytes().to_vec(),
                secret_key: sk.as_bytes().to_vec(),
            })
        }
        HqcLevel::Hqc192 => {
            let (pk, sk) = pqcrypto_hqc::hqc192::keypair();
            Ok(HqcKeyPair {
                level,
                public_key: pk.as_bytes().to_vec(),
                secret_key: sk.as_bytes().to_vec(),
            })
        }
        HqcLevel::Hqc256 => {
            let (pk, sk) = pqcrypto_hqc::hqc256::keypair();
            Ok(HqcKeyPair {
                level,
                public_key: pk.as_bytes().to_vec(),
                secret_key: sk.as_bytes().to_vec(),
            })
        }
    }
}

/// Encapsulate against an HQC public key.
///
/// Returns `(ciphertext, shared_secret)`.
pub fn hqc_encapsulate(public_key: &[u8], level: HqcLevel) -> CpacResult<(Vec<u8>, Vec<u8>)> {
    use pqcrypto_traits::kem::*;
    match level {
        HqcLevel::Hqc128 => {
            let pk = pqcrypto_hqc::hqc128::PublicKey::from_bytes(public_key)
                .map_err(|e| CpacError::Encryption(format!("HQC-128 bad pk: {e}")))?;
            let (ss, ct) = pqcrypto_hqc::hqc128::encapsulate(&pk);
            Ok((ct.as_bytes().to_vec(), ss.as_bytes().to_vec()))
        }
        HqcLevel::Hqc192 => {
            let pk = pqcrypto_hqc::hqc192::PublicKey::from_bytes(public_key)
                .map_err(|e| CpacError::Encryption(format!("HQC-192 bad pk: {e}")))?;
            let (ss, ct) = pqcrypto_hqc::hqc192::encapsulate(&pk);
            Ok((ct.as_bytes().to_vec(), ss.as_bytes().to_vec()))
        }
        HqcLevel::Hqc256 => {
            let pk = pqcrypto_hqc::hqc256::PublicKey::from_bytes(public_key)
                .map_err(|e| CpacError::Encryption(format!("HQC-256 bad pk: {e}")))?;
            let (ss, ct) = pqcrypto_hqc::hqc256::encapsulate(&pk);
            Ok((ct.as_bytes().to_vec(), ss.as_bytes().to_vec()))
        }
    }
}

/// Decapsulate an HQC ciphertext with the secret key.
pub fn hqc_decapsulate(
    ciphertext: &[u8],
    secret_key: &[u8],
    level: HqcLevel,
) -> CpacResult<Vec<u8>> {
    use pqcrypto_traits::kem::*;
    match level {
        HqcLevel::Hqc128 => {
            let sk = pqcrypto_hqc::hqc128::SecretKey::from_bytes(secret_key)
                .map_err(|e| CpacError::Encryption(format!("HQC-128 bad sk: {e}")))?;
            let ct = pqcrypto_hqc::hqc128::Ciphertext::from_bytes(ciphertext)
                .map_err(|e| CpacError::Encryption(format!("HQC-128 bad ct: {e}")))?;
            let ss = pqcrypto_hqc::hqc128::decapsulate(&ct, &sk);
            Ok(ss.as_bytes().to_vec())
        }
        HqcLevel::Hqc192 => {
            let sk = pqcrypto_hqc::hqc192::SecretKey::from_bytes(secret_key)
                .map_err(|e| CpacError::Encryption(format!("HQC-192 bad sk: {e}")))?;
            let ct = pqcrypto_hqc::hqc192::Ciphertext::from_bytes(ciphertext)
                .map_err(|e| CpacError::Encryption(format!("HQC-192 bad ct: {e}")))?;
            let ss = pqcrypto_hqc::hqc192::decapsulate(&ct, &sk);
            Ok(ss.as_bytes().to_vec())
        }
        HqcLevel::Hqc256 => {
            let sk = pqcrypto_hqc::hqc256::SecretKey::from_bytes(secret_key)
                .map_err(|e| CpacError::Encryption(format!("HQC-256 bad sk: {e}")))?;
            let ct = pqcrypto_hqc::hqc256::Ciphertext::from_bytes(ciphertext)
                .map_err(|e| CpacError::Encryption(format!("HQC-256 bad ct: {e}")))?;
            let ss = pqcrypto_hqc::hqc256::decapsulate(&ct, &sk);
            Ok(ss.as_bytes().to_vec())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hqc128_roundtrip() {
        let kp = hqc_keygen(HqcLevel::Hqc128).unwrap();
        let (ct, ss_enc) = hqc_encapsulate(&kp.public_key, HqcLevel::Hqc128).unwrap();
        let ss_dec = hqc_decapsulate(&ct, &kp.secret_key, HqcLevel::Hqc128).unwrap();
        assert_eq!(ss_enc, ss_dec);
        assert!(!ss_enc.is_empty());
    }

    #[test]
    fn hqc192_roundtrip() {
        let kp = hqc_keygen(HqcLevel::Hqc192).unwrap();
        let (ct, ss_enc) = hqc_encapsulate(&kp.public_key, HqcLevel::Hqc192).unwrap();
        let ss_dec = hqc_decapsulate(&ct, &kp.secret_key, HqcLevel::Hqc192).unwrap();
        assert_eq!(ss_enc, ss_dec);
    }

    #[test]
    fn hqc256_roundtrip() {
        let kp = hqc_keygen(HqcLevel::Hqc256).unwrap();
        let (ct, ss_enc) = hqc_encapsulate(&kp.public_key, HqcLevel::Hqc256).unwrap();
        let ss_dec = hqc_decapsulate(&ct, &kp.secret_key, HqcLevel::Hqc256).unwrap();
        assert_eq!(ss_enc, ss_dec);
    }

    #[test]
    fn hqc_level_wire_ids() {
        assert_eq!(HqcLevel::from_id(30).unwrap(), HqcLevel::Hqc128);
        assert_eq!(HqcLevel::from_id(31).unwrap(), HqcLevel::Hqc192);
        assert_eq!(HqcLevel::from_id(32).unwrap(), HqcLevel::Hqc256);
        assert!(HqcLevel::from_id(99).is_err());
    }
}
