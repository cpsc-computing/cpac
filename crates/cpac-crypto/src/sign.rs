// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Digital signatures: Ed25519.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;

/// Ed25519 keypair.
pub struct Ed25519KeyPair {
    /// Signing (secret) key.
    pub signing: SigningKey,
    /// Verifying (public) key.
    pub verifying: VerifyingKey,
}

/// Generate a new Ed25519 keypair.
pub fn generate_ed25519_keypair() -> Ed25519KeyPair {
    let signing = SigningKey::generate(&mut OsRng);
    let verifying = signing.verifying_key();
    Ed25519KeyPair { signing, verifying }
}

/// Sign a message with Ed25519.
#[must_use]
pub fn ed25519_sign(key: &SigningKey, message: &[u8]) -> Vec<u8> {
    let sig: Signature = key.sign(message);
    sig.to_bytes().to_vec()
}

/// Verify an Ed25519 signature.
#[must_use]
pub fn ed25519_verify(key: &VerifyingKey, message: &[u8], signature: &[u8]) -> bool {
    if signature.len() != 64 {
        return false;
    }
    let sig_bytes: [u8; 64] = signature.try_into().unwrap();
    let sig = Signature::from_bytes(&sig_bytes);
    key.verify(message, &sig).is_ok()
}

// ---------------------------------------------------------------------------
// SLH-DSA-SHA2-128s (FIPS 205) — hash-based signatures
// ---------------------------------------------------------------------------

/// SLH-DSA-SHA2-128s signature stub.
///
/// Currently deferred due to `signature` crate version conflict between
/// `slh-dsa` and `ml-dsa`.  Placeholder returns clear error.
pub fn slh_dsa_sign(_message: &[u8], _secret_key: &[u8]) -> cpac_types::CpacResult<Vec<u8>> {
    Err(cpac_types::CpacError::Encryption(
        "SLH-DSA-SHA2-128s signing deferred (signature crate version conflict)".into(),
    ))
}

pub fn slh_dsa_verify(
    _message: &[u8],
    _signature: &[u8],
    _public_key: &[u8],
) -> cpac_types::CpacResult<bool> {
    Err(cpac_types::CpacError::Encryption(
        "SLH-DSA-SHA2-128s verify deferred (signature crate version conflict)".into(),
    ))
}

// ---------------------------------------------------------------------------
// Hybrid signatures: Ed25519 (classical) + ML-DSA-65 (PQC)
// ---------------------------------------------------------------------------

/// Wire format for hybrid signatures:
/// ```text
/// "CPHS" (4B) | version (1B) | ed25519_sig (64B) | mldsa_sig_len (4B LE) | mldsa_sig
/// ```
const CPHS_MAGIC: &[u8; 4] = b"CPHS";
#[cfg(feature = "pqc")]
const CPHS_VERSION: u8 = 1;

/// A hybrid keypair for dual-signing.
#[derive(Clone)]
pub struct HybridSignKeyPair {
    /// Ed25519 signing key (32 bytes).
    pub ed25519_signing: Vec<u8>,
    /// Ed25519 verifying key (32 bytes).
    pub ed25519_verifying: Vec<u8>,
    /// ML-DSA-65 signing key.
    pub mldsa_signing: Vec<u8>,
    /// ML-DSA-65 verifying key.
    pub mldsa_verifying: Vec<u8>,
}

/// Generate a hybrid signature keypair (Ed25519 + ML-DSA-65).
#[cfg(feature = "pqc")]
pub fn hybrid_sign_keygen() -> cpac_types::CpacResult<HybridSignKeyPair> {
    let ed_kp = generate_ed25519_keypair();
    let pqc_kp = crate::pqc::pqc_keygen(crate::pqc::PqcAlgorithm::MlDsa65)?;
    Ok(HybridSignKeyPair {
        ed25519_signing: ed_kp.signing.to_bytes().to_vec(),
        ed25519_verifying: ed_kp.verifying.to_bytes().to_vec(),
        mldsa_signing: pqc_kp.secret_key,
        mldsa_verifying: pqc_kp.public_key,
    })
}

/// Sign with both Ed25519 and ML-DSA-65, producing a CPHS frame.
#[cfg(feature = "pqc")]
pub fn hybrid_sign(
    message: &[u8],
    ed_signing_key: &[u8],
    mldsa_signing_key: &[u8],
) -> cpac_types::CpacResult<Vec<u8>> {
    // Ed25519
    let ed_sk_bytes: [u8; 32] = ed_signing_key
        .try_into()
        .map_err(|_| cpac_types::CpacError::Encryption("invalid Ed25519 key length".into()))?;
    let ed_sk = SigningKey::from_bytes(&ed_sk_bytes);
    let ed_sig = ed25519_sign(&ed_sk, message);

    // ML-DSA-65
    let mldsa_sig = crate::pqc::pqc_sign(message, mldsa_signing_key, crate::pqc::PqcAlgorithm::MlDsa65)?;

    // Build CPHS frame
    let total = 4 + 1 + 64 + 4 + mldsa_sig.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(CPHS_MAGIC);
    out.push(CPHS_VERSION);
    out.extend_from_slice(&ed_sig);
    out.extend_from_slice(&(mldsa_sig.len() as u32).to_le_bytes());
    out.extend_from_slice(&mldsa_sig);
    Ok(out)
}

/// Verify a CPHS hybrid signature. Both signatures must be valid.
#[cfg(feature = "pqc")]
pub fn hybrid_verify(
    message: &[u8],
    cphs_data: &[u8],
    ed_verifying_key: &[u8],
    mldsa_verifying_key: &[u8],
) -> cpac_types::CpacResult<bool> {
    if cphs_data.len() < 4 + 1 + 64 + 4 {
        return Err(cpac_types::CpacError::Encryption("CPHS data too short".into()));
    }
    if &cphs_data[..4] != CPHS_MAGIC || cphs_data[4] != CPHS_VERSION {
        return Err(cpac_types::CpacError::Encryption("not a CPHS frame".into()));
    }
    let ed_sig = &cphs_data[5..69];
    let mldsa_sig_len = u32::from_le_bytes([
        cphs_data[69], cphs_data[70], cphs_data[71], cphs_data[72],
    ]) as usize;
    if cphs_data.len() < 73 + mldsa_sig_len {
        return Err(cpac_types::CpacError::Encryption("CPHS truncated".into()));
    }
    let mldsa_sig = &cphs_data[73..73 + mldsa_sig_len];

    // Verify Ed25519
    let vk_bytes: [u8; 32] = ed_verifying_key
        .try_into()
        .map_err(|_| cpac_types::CpacError::Encryption("invalid Ed25519 verifying key".into()))?;
    let vk = VerifyingKey::from_bytes(&vk_bytes)
        .map_err(|e| cpac_types::CpacError::Encryption(format!("bad Ed25519 verifying key: {e}")))?;
    if !ed25519_verify(&vk, message, ed_sig) {
        return Ok(false);
    }

    // Verify ML-DSA-65
    crate::pqc::pqc_verify(
        message,
        mldsa_sig,
        mldsa_verifying_key,
        crate::pqc::PqcAlgorithm::MlDsa65,
    )
}

/// Check if data starts with the CPHS magic.
#[must_use]
pub fn is_cphs(data: &[u8]) -> bool {
    data.len() >= 4 && &data[..4] == CPHS_MAGIC
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_verify_roundtrip() {
        let kp = generate_ed25519_keypair();
        let message = b"CPAC integrity check";
        let sig = ed25519_sign(&kp.signing, message);
        assert!(ed25519_verify(&kp.verifying, message, &sig));
    }

    #[test]
    fn verify_tampered_fails() {
        let kp = generate_ed25519_keypair();
        let sig = ed25519_sign(&kp.signing, b"original");
        assert!(!ed25519_verify(&kp.verifying, b"tampered", &sig));
    }

    #[test]
    fn wrong_key_fails() {
        let kp1 = generate_ed25519_keypair();
        let kp2 = generate_ed25519_keypair();
        let sig = ed25519_sign(&kp1.signing, b"message");
        assert!(!ed25519_verify(&kp2.verifying, b"message", &sig));
    }
}
