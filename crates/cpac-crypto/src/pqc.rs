// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Post-Quantum Cryptography — ML-KEM (FIPS 203) + ML-DSA (FIPS 204).
//!
//! Uses pure-Rust `RustCrypto` implementations. Feature-gated behind `pqc`.
//!
//! - ML-KEM-768: key encapsulation (quantum-safe key exchange)
//! - ML-DSA-65: digital signatures (quantum-safe authentication)
//! - SLH-DSA: deferred (signature crate version conflict with ml-dsa)

use cpac_types::{CpacError, CpacResult};

/// Supported post-quantum algorithms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PqcAlgorithm {
    /// ML-KEM-768 (FIPS 203) — key encapsulation.
    MlKem768,
    /// ML-KEM-1024 (FIPS 203, Level 5) — key encapsulation.
    MlKem1024,
    /// ML-DSA-65 (FIPS 204) — digital signatures.
    MlDsa65,
    /// ML-DSA-87 (FIPS 204, Level 5) — digital signatures.
    MlDsa87,
    /// SLH-DSA-SHA2-128s (FIPS 205) — hash-based signatures.
    SlhDsaSha2128s,
}

impl PqcAlgorithm {
    /// Wire ID for serialisation in CPHE v2 / CPCE v2 headers.
    #[must_use]
    pub fn id(self) -> u8 {
        match self {
            PqcAlgorithm::MlKem768 => 1,
            PqcAlgorithm::MlKem1024 => 2,
            PqcAlgorithm::MlDsa65 => 10,
            PqcAlgorithm::MlDsa87 => 11,
            PqcAlgorithm::SlhDsaSha2128s => 20,
        }
    }

    /// Decode from wire ID.
    pub fn from_id(id: u8) -> cpac_types::CpacResult<Self> {
        match id {
            1 => Ok(PqcAlgorithm::MlKem768),
            2 => Ok(PqcAlgorithm::MlKem1024),
            10 => Ok(PqcAlgorithm::MlDsa65),
            11 => Ok(PqcAlgorithm::MlDsa87),
            20 => Ok(PqcAlgorithm::SlhDsaSha2128s),
            _ => Err(cpac_types::CpacError::Encryption(format!(
                "unknown PQC algorithm wire ID: {id}"
            ))),
        }
    }

    /// Human-readable label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            PqcAlgorithm::MlKem768 => "ML-KEM-768",
            PqcAlgorithm::MlKem1024 => "ML-KEM-1024",
            PqcAlgorithm::MlDsa65 => "ML-DSA-65",
            PqcAlgorithm::MlDsa87 => "ML-DSA-87",
            PqcAlgorithm::SlhDsaSha2128s => "SLH-DSA-SHA2-128s",
        }
    }
}

/// A PQC key pair (serialized byte blobs).
#[derive(Clone, Debug)]
pub struct PqcKeyPair {
    pub algorithm: PqcAlgorithm,
    pub public_key: Vec<u8>,
    pub secret_key: Vec<u8>,
}

/// Generate a PQC key pair.
pub fn pqc_keygen(algo: PqcAlgorithm) -> CpacResult<PqcKeyPair> {
    match algo {
        PqcAlgorithm::MlKem768 => keygen_mlkem768(),
        PqcAlgorithm::MlKem1024 => Err(CpacError::Encryption(
            "ML-KEM-1024 keygen not yet implemented (Level 5 parameterisation pending)".into(),
        )),
        PqcAlgorithm::MlDsa65 => keygen_mldsa65(),
        PqcAlgorithm::MlDsa87 => Err(CpacError::Encryption(
            "ML-DSA-87 keygen not yet implemented (Level 5 parameterisation pending)".into(),
        )),
        PqcAlgorithm::SlhDsaSha2128s => Err(CpacError::Encryption(
            "SLH-DSA not yet available (signature crate version conflict)".into(),
        )),
    }
}

/// PQC key encapsulation (ML-KEM).
///
/// Returns `(ciphertext, shared_secret)`.
pub fn pqc_encapsulate(public_key: &[u8], algo: PqcAlgorithm) -> CpacResult<(Vec<u8>, Vec<u8>)> {
    match algo {
        PqcAlgorithm::MlKem768 => encapsulate_mlkem768(public_key),
        _ => Err(CpacError::Encryption(format!(
            "{algo:?} does not support encapsulation"
        ))),
    }
}

/// PQC key decapsulation (ML-KEM).
pub fn pqc_decapsulate(
    ciphertext: &[u8],
    secret_key: &[u8],
    algo: PqcAlgorithm,
) -> CpacResult<Vec<u8>> {
    match algo {
        PqcAlgorithm::MlKem768 => decapsulate_mlkem768(ciphertext, secret_key),
        _ => Err(CpacError::Encryption(format!(
            "{algo:?} does not support decapsulation"
        ))),
    }
}

/// PQC digital signature.
pub fn pqc_sign(message: &[u8], secret_key: &[u8], algo: PqcAlgorithm) -> CpacResult<Vec<u8>> {
    match algo {
        PqcAlgorithm::MlDsa65 => sign_mldsa65(message, secret_key),
        PqcAlgorithm::MlDsa87 => Err(CpacError::Encryption(
            "ML-DSA-87 signing not yet implemented".into(),
        )),
        PqcAlgorithm::SlhDsaSha2128s => {
            Err(CpacError::Encryption("SLH-DSA not yet available".into()))
        }
        PqcAlgorithm::MlKem768 | PqcAlgorithm::MlKem1024 => Err(CpacError::Encryption(format!(
            "{algo:?} does not support signing"
        ))),
    }
}

/// PQC signature verification.
pub fn pqc_verify(
    message: &[u8],
    signature: &[u8],
    public_key: &[u8],
    algo: PqcAlgorithm,
) -> CpacResult<bool> {
    match algo {
        PqcAlgorithm::MlDsa65 => verify_mldsa65(message, signature, public_key),
        PqcAlgorithm::MlDsa87 => Err(CpacError::Encryption(
            "ML-DSA-87 verification not yet implemented".into(),
        )),
        PqcAlgorithm::SlhDsaSha2128s => {
            Err(CpacError::Encryption("SLH-DSA not yet available".into()))
        }
        PqcAlgorithm::MlKem768 | PqcAlgorithm::MlKem1024 => Err(CpacError::Encryption(format!(
            "{algo:?} does not support verification"
        ))),
    }
}

// ---------------------------------------------------------------------------
// ML-KEM-768 implementation (FIPS 203)
// ---------------------------------------------------------------------------

fn keygen_mlkem768() -> CpacResult<PqcKeyPair> {
    use ml_kem::{EncodedSizeUser, KemCore, MlKem768};
    let mut rng = rand::thread_rng();
    let (dk, ek) = MlKem768::generate(&mut rng);
    Ok(PqcKeyPair {
        algorithm: PqcAlgorithm::MlKem768,
        public_key: ek.as_bytes().as_slice().to_vec(),
        secret_key: dk.as_bytes().as_slice().to_vec(),
    })
}

fn encapsulate_mlkem768(public_key: &[u8]) -> CpacResult<(Vec<u8>, Vec<u8>)> {
    use ml_kem::{
        kem::{Encapsulate, EncapsulationKey},
        Encoded, EncodedSizeUser, MlKem768Params,
    };
    type Ek = EncapsulationKey<MlKem768Params>;
    let ek_arr: Encoded<Ek> = public_key
        .try_into()
        .map_err(|_| CpacError::Encryption("invalid ML-KEM-768 public key length".into()))?;
    let ek = Ek::from_bytes(&ek_arr);
    let mut rng = rand::thread_rng();
    let (ct, ss) = ek
        .encapsulate(&mut rng)
        .map_err(|()| CpacError::Encryption("ML-KEM-768 encapsulation failed".into()))?;
    Ok((ct.as_slice().to_vec(), ss.as_slice().to_vec()))
}

fn decapsulate_mlkem768(ciphertext: &[u8], secret_key: &[u8]) -> CpacResult<Vec<u8>> {
    use ml_kem::{
        kem::{Decapsulate, DecapsulationKey},
        Ciphertext, Encoded, EncodedSizeUser, MlKem768, MlKem768Params,
    };
    type Dk = DecapsulationKey<MlKem768Params>;
    let dk_arr: Encoded<Dk> = secret_key
        .try_into()
        .map_err(|_| CpacError::Encryption("invalid ML-KEM-768 secret key length".into()))?;
    let dk = Dk::from_bytes(&dk_arr);
    let ct: Ciphertext<MlKem768> = ciphertext
        .try_into()
        .map_err(|_| CpacError::Encryption("invalid ML-KEM-768 ciphertext length".into()))?;
    let ss = dk
        .decapsulate(&ct)
        .map_err(|()| CpacError::Encryption("ML-KEM-768 decapsulation failed".into()))?;
    Ok(ss.as_slice().to_vec())
}

// ---------------------------------------------------------------------------
// ML-DSA-65 implementation (FIPS 204)
// ---------------------------------------------------------------------------

fn keygen_mldsa65() -> CpacResult<PqcKeyPair> {
    use ml_dsa::{MlDsa65, Seed, SigningKey};
    use rand::RngCore;
    let mut seed_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut seed_bytes);
    let seed = Seed::from(seed_bytes);
    let sk = SigningKey::<MlDsa65>::from_seed(&seed);
    let vk = sk.verifying_key();
    Ok(PqcKeyPair {
        algorithm: PqcAlgorithm::MlDsa65,
        public_key: vk.encode().as_slice().to_vec(),
        secret_key: seed_bytes.to_vec(),
    })
}

fn sign_mldsa65(message: &[u8], secret_key: &[u8]) -> CpacResult<Vec<u8>> {
    use ml_dsa::{MlDsa65, Seed, SigningKey};
    let seed: Seed = secret_key.try_into().map_err(|_| {
        CpacError::Encryption("invalid ML-DSA-65 seed length (expected 32 bytes)".into())
    })?;
    let sk = SigningKey::<MlDsa65>::from_seed(&seed);
    let sig = sk
        .sign_deterministic(message, &[])
        .map_err(|e| CpacError::Encryption(format!("ML-DSA-65 signing failed: {e}")))?;
    Ok(sig.encode().as_slice().to_vec())
}

fn verify_mldsa65(message: &[u8], sig_bytes: &[u8], public_key: &[u8]) -> CpacResult<bool> {
    use ml_dsa::{EncodedSignature, EncodedVerifyingKey, MlDsa65, Signature, VerifyingKey};
    let vk_arr: EncodedVerifyingKey<MlDsa65> = public_key
        .try_into()
        .map_err(|_| CpacError::Encryption("invalid ML-DSA-65 verifying key length".into()))?;
    let vk = VerifyingKey::<MlDsa65>::decode(&vk_arr);
    let sig_arr: EncodedSignature<MlDsa65> = sig_bytes
        .try_into()
        .map_err(|_| CpacError::Encryption("invalid ML-DSA-65 signature length".into()))?;
    let sig = Signature::<MlDsa65>::decode(&sig_arr)
        .ok_or_else(|| CpacError::Encryption("invalid ML-DSA-65 signature".into()))?;
    Ok(vk.verify_with_context(message, &[], &sig))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mlkem768_keygen_encap_decap_roundtrip() {
        let kp = pqc_keygen(PqcAlgorithm::MlKem768).unwrap();
        assert!(!kp.public_key.is_empty());
        assert!(!kp.secret_key.is_empty());

        let (ct, ss_sender) = pqc_encapsulate(&kp.public_key, PqcAlgorithm::MlKem768).unwrap();
        let ss_receiver = pqc_decapsulate(&ct, &kp.secret_key, PqcAlgorithm::MlKem768).unwrap();
        assert_eq!(ss_sender, ss_receiver, "shared secrets must match");
        assert_eq!(ss_sender.len(), 32, "shared secret should be 32 bytes");
    }

    #[test]
    fn mldsa65_keygen_sign_verify_roundtrip() {
        let kp = pqc_keygen(PqcAlgorithm::MlDsa65).unwrap();
        let message = b"CPAC post-quantum signature test";
        let sig = pqc_sign(message, &kp.secret_key, PqcAlgorithm::MlDsa65).unwrap();
        assert!(!sig.is_empty());
        let valid = pqc_verify(message, &sig, &kp.public_key, PqcAlgorithm::MlDsa65).unwrap();
        assert!(valid, "signature must verify");
    }

    #[test]
    fn mldsa65_wrong_message_fails() {
        let kp = pqc_keygen(PqcAlgorithm::MlDsa65).unwrap();
        let sig = pqc_sign(b"original", &kp.secret_key, PqcAlgorithm::MlDsa65).unwrap();
        let valid = pqc_verify(b"tampered", &sig, &kp.public_key, PqcAlgorithm::MlDsa65).unwrap();
        assert!(!valid, "tampered message must not verify");
    }

    #[test]
    fn slh_dsa_returns_error() {
        assert!(pqc_keygen(PqcAlgorithm::SlhDsaSha2128s).is_err());
        assert!(pqc_sign(b"msg", b"sk", PqcAlgorithm::SlhDsaSha2128s).is_err());
    }

    #[test]
    fn kem_on_signature_algo_returns_error() {
        assert!(pqc_encapsulate(b"pk", PqcAlgorithm::MlDsa65).is_err());
    }
}
