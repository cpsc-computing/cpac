// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Hybrid encryption: X25519 (classical) + ML-KEM-768 (post-quantum).
//!
//! Defence-in-depth: even if one primitive is broken, the other still
//! protects confidentiality.
//!
//! ## Wire format (CPHE)
//! ```text
//! "CPHE" (4B) | version (1B)
//! | x25519_public (32B) | mlkem_ciphertext_len (2B LE) | mlkem_ciphertext
//! | aead_nonce_len (1B) | aead_nonce | aead_ciphertext
//! ```

use cpac_types::{CpacError, CpacResult};

/// CPHE magic bytes.
pub const CPHE_MAGIC: &[u8; 4] = b"CPHE";

/// CPHE v1 (legacy, no algorithm IDs).
pub const CPHE_VERSION_V1: u8 = 1;
/// CPHE v2 (algorithm agility — includes KEM, AEAD, KDF IDs).
pub const CPHE_VERSION: u8 = 2;

/// A hybrid key pair (X25519 + ML-KEM-768).
#[derive(Clone, Debug)]
pub struct HybridKeyPair {
    /// X25519 secret key (32 bytes).
    pub x25519_secret: Vec<u8>,
    /// X25519 public key (32 bytes).
    pub x25519_public: Vec<u8>,
    /// ML-KEM-768 public key (encapsulation key).
    pub mlkem_public: Vec<u8>,
    /// ML-KEM-768 secret key (decapsulation key).
    pub mlkem_secret: Vec<u8>,
}

/// Generate a hybrid key pair (X25519 + ML-KEM-768).
pub fn hybrid_keygen() -> CpacResult<HybridKeyPair> {
    // X25519
    let x_kp = crate::keys::generate_x25519_keypair();
    let x_secret = x_kp.secret.to_bytes().to_vec();
    let x_public = x_kp.public.to_bytes().to_vec();

    // ML-KEM-768
    let pqc_kp = crate::pqc::pqc_keygen(crate::pqc::PqcAlgorithm::MlKem768)?;

    Ok(HybridKeyPair {
        x25519_secret: x_secret,
        x25519_public: x_public,
        mlkem_public: pqc_kp.public_key,
        mlkem_secret: pqc_kp.secret_key,
    })
}

/// Hybrid encrypt data using recipient's public keys.
///
/// 1. Generate ephemeral X25519 keypair, compute DH shared secret.
/// 2. Encapsulate against ML-KEM-768 public key.
/// 3. Combine both shared secrets via HKDF.
/// 4. Encrypt plaintext with ChaCha20-Poly1305 using derived key.
/// 5. Encode as CPHE wire format.
pub fn hybrid_encrypt(
    plaintext: &[u8],
    recipient_x25519_public: &[u8],
    recipient_mlkem_public: &[u8],
) -> CpacResult<Vec<u8>> {
    // 1. Ephemeral X25519
    let eph = crate::keys::generate_x25519_keypair();
    let their_pk: [u8; 32] = recipient_x25519_public
        .try_into()
        .map_err(|_| CpacError::Encryption("invalid X25519 public key length".into()))?;
    let their_x_pk = x25519_dalek::PublicKey::from(their_pk);
    let x_shared = crate::keys::x25519_shared_secret(&eph.secret, &their_x_pk);

    // 2. ML-KEM-768 encapsulate
    let (mlkem_ct, mlkem_ss) =
        crate::pqc::pqc_encapsulate(recipient_mlkem_public, crate::pqc::PqcAlgorithm::MlKem768)?;

    // 3. Combine shared secrets via HKDF
    let combined_key = derive_hybrid_key(&x_shared, &mlkem_ss)?;

    // 4. AEAD encrypt
    let (nonce, ciphertext) = crate::aead::encrypt_aead(
        plaintext,
        &combined_key,
        crate::aead::AeadAlgorithm::ChaCha20Poly1305,
    )?;

    // 5. Encode CPHE v2 frame (with algorithm agility IDs)
    //    v2 layout: CPHE(4) | version(1) | kem_id(1) | aead_id(1) | kdf_id(1) |
    //               x25519_pub(32) | mlkem_ct_len(2 LE) | mlkem_ct |
    //               nonce_len(1) | nonce | ciphertext
    let eph_pub = eph.public.to_bytes();
    let mlkem_ct_len = mlkem_ct.len() as u16;
    let nonce_len = nonce.len() as u8;

    let total = 4 + 1 + 3 + 32 + 2 + mlkem_ct.len() + 1 + nonce.len() + ciphertext.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(CPHE_MAGIC);
    out.push(CPHE_VERSION);                                          // v2
    out.push(crate::pqc::PqcAlgorithm::MlKem768.id());              // kem_id
    out.push(crate::aead::AeadAlgorithm::ChaCha20Poly1305.id());    // aead_id
    out.push(crate::kdf::KdfAlgorithm::HkdfSha256.id());            // kdf_id
    out.extend_from_slice(&eph_pub);
    out.extend_from_slice(&mlkem_ct_len.to_le_bytes());
    out.extend_from_slice(&mlkem_ct);
    out.push(nonce_len);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);

    Ok(out)
}

/// Hybrid decrypt a CPHE-encoded message.
pub fn hybrid_decrypt(
    cphe_data: &[u8],
    our_x25519_secret: &[u8],
    our_mlkem_secret: &[u8],
) -> CpacResult<Vec<u8>> {
    // Parse CPHE header — supports both v1 (legacy) and v2 (agile)
    if cphe_data.len() < 4 + 1 + 32 + 2 {
        return Err(CpacError::Encryption("CPHE data too short".into()));
    }
    if &cphe_data[..4] != CPHE_MAGIC {
        return Err(CpacError::Encryption("not a CPHE frame".into()));
    }
    let version = cphe_data[4];

    // Determine layout offsets based on version
    let (kem_algo, aead_algo, data_start) = match version {
        CPHE_VERSION_V1 => (
            crate::pqc::PqcAlgorithm::MlKem768,
            crate::aead::AeadAlgorithm::ChaCha20Poly1305,
            5usize, // v1: data starts right after version byte
        ),
        CPHE_VERSION => {
            if cphe_data.len() < 8 {
                return Err(CpacError::Encryption("CPHE v2 header too short".into()));
            }
            let kem = crate::pqc::PqcAlgorithm::from_id(cphe_data[5])?;
            let aead = crate::aead::AeadAlgorithm::from_id(cphe_data[6])?;
            let _kdf = crate::kdf::KdfAlgorithm::from_id(cphe_data[7])?;
            (kem, aead, 8usize) // v2: 3 extra ID bytes
        }
        _ => {
            return Err(CpacError::Encryption(format!(
                "unsupported CPHE version: {version}"
            )));
        }
    };

    let eph_pub_bytes: [u8; 32] = cphe_data[data_start..data_start + 32]
        .try_into()
        .map_err(|_| CpacError::Encryption("bad ephemeral public key".into()))?;
    let eph_pub = x25519_dalek::PublicKey::from(eph_pub_bytes);

    let ct_len_offset = data_start + 32;
    let mlkem_ct_len =
        u16::from_le_bytes([cphe_data[ct_len_offset], cphe_data[ct_len_offset + 1]]) as usize;
    let mlkem_ct_start = ct_len_offset + 2;
    let mlkem_ct_end = mlkem_ct_start + mlkem_ct_len;
    if cphe_data.len() < mlkem_ct_end + 1 {
        return Err(CpacError::Encryption(
            "CPHE truncated ML-KEM ciphertext".into(),
        ));
    }
    let mlkem_ct = &cphe_data[mlkem_ct_start..mlkem_ct_end];

    let nonce_len = cphe_data[mlkem_ct_end] as usize;
    let nonce_end = mlkem_ct_end + 1 + nonce_len;
    if cphe_data.len() < nonce_end {
        return Err(CpacError::Encryption("CPHE truncated nonce".into()));
    }
    let nonce = &cphe_data[mlkem_ct_end + 1..nonce_end];
    let ciphertext = &cphe_data[nonce_end..];

    // 1. X25519 shared secret
    let our_sk_bytes: [u8; 32] = our_x25519_secret
        .try_into()
        .map_err(|_| CpacError::Encryption("invalid X25519 secret key length".into()))?;
    let our_sk = x25519_dalek::StaticSecret::from(our_sk_bytes);
    let x_shared = crate::keys::x25519_shared_secret(&our_sk, &eph_pub);

    // 2. ML-KEM decapsulate (algorithm from header)
    let mlkem_ss =
        crate::pqc::pqc_decapsulate(mlkem_ct, our_mlkem_secret, kem_algo)?;

    // 3. Combine shared secrets
    let combined_key = derive_hybrid_key(&x_shared, &mlkem_ss)?;

    // 4. AEAD decrypt (algorithm from header)
    crate::aead::decrypt_aead(ciphertext, &combined_key, nonce, aead_algo)
}

/// Derive a 32-byte key from two shared secrets via HKDF-SHA256.
fn derive_hybrid_key(x25519_ss: &[u8; 32], mlkem_ss: &[u8]) -> CpacResult<[u8; 32]> {
    let mut ikm = Vec::with_capacity(32 + mlkem_ss.len());
    ikm.extend_from_slice(x25519_ss);
    ikm.extend_from_slice(mlkem_ss);
    let key = crate::kdf::derive_key_hkdf(&ikm, b"CPHE-hybrid-salt", b"CPHE-hybrid-v1")?;
    Ok(key)
}

/// Check whether data starts with the CPHE magic.
#[must_use]
pub fn is_cphe(data: &[u8]) -> bool {
    data.len() >= 4 && &data[..4] == CPHE_MAGIC
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hybrid_roundtrip() {
        let kp = hybrid_keygen().unwrap();
        let plaintext = b"Hello hybrid post-quantum world!";
        let encrypted = hybrid_encrypt(plaintext, &kp.x25519_public, &kp.mlkem_public).unwrap();
        assert!(is_cphe(&encrypted));
        let decrypted = hybrid_decrypt(&encrypted, &kp.x25519_secret, &kp.mlkem_secret).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn hybrid_large_data() {
        let kp = hybrid_keygen().unwrap();
        let plaintext: Vec<u8> = (0u8..=255).cycle().take(100_000).collect();
        let encrypted = hybrid_encrypt(&plaintext, &kp.x25519_public, &kp.mlkem_public).unwrap();
        let decrypted = hybrid_decrypt(&encrypted, &kp.x25519_secret, &kp.mlkem_secret).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn hybrid_wrong_key_fails() {
        let kp1 = hybrid_keygen().unwrap();
        let kp2 = hybrid_keygen().unwrap();
        let plaintext = b"secret data";
        let encrypted = hybrid_encrypt(plaintext, &kp1.x25519_public, &kp1.mlkem_public).unwrap();
        // Decrypt with wrong keys should fail
        let result = hybrid_decrypt(&encrypted, &kp2.x25519_secret, &kp2.mlkem_secret);
        assert!(result.is_err());
    }

    #[test]
    fn is_cphe_detection() {
        assert!(is_cphe(b"CPHEsomething"));
        assert!(!is_cphe(b"CPsomething"));
        assert!(!is_cphe(b"XX"));
    }
}
