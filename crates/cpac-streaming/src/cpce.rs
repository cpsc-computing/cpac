// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! CPCE — CPAC Compressed-Encrypted wire format.
//!
//! Combines compression and encryption in a single pass:
//! 1. Compress data (standard CPAC or streaming frame).
//! 2. Encrypt the compressed frame with AEAD.
//!
//! ## Wire format
//! ```text
//! "CPCE" (4B magic)
//! version     (1B = 0x01)
//! enc_mode    (1B): 0x01 = password, 0x02 = pqc_hybrid
//! aead_algo   (1B): AEAD algorithm ID
//!
//! -- Password mode (enc_mode 0x01):
//!   salt_len  (1B) | salt (salt_len B)
//!
//! -- PQC Hybrid mode (enc_mode 0x02):
//!   x25519_ephemeral_pub (32B)
//!   mlkem_ct_len (2B LE) | mlkem_ciphertext (mlkem_ct_len B)
//!
//! nonce_len   (1B) | nonce (nonce_len B)
//! payload_len (8B LE)
//! encrypted_payload (payload_len B)   // AEAD ciphertext of inner CPAC frame
//! ```

use cpac_crypto::aead::AeadAlgorithm;
use cpac_types::{CpacError, CpacResult};

/// CPCE magic bytes.
pub const CPCE_MAGIC: &[u8; 4] = b"CPCE";

/// Current CPCE format version.
pub const CPCE_VERSION: u8 = 1;

/// Wire IDs for encryption mode.
const ENC_MODE_PASSWORD: u8 = 0x01;
const ENC_MODE_PQC_HYBRID: u8 = 0x02;

/// Check whether data starts with the CPCE magic.
#[must_use]
pub fn is_cpce(data: &[u8]) -> bool {
    data.len() >= 4 && &data[..4] == CPCE_MAGIC
}

// ---------------------------------------------------------------------------
// Password-based encrypt / decrypt
// ---------------------------------------------------------------------------

/// Encrypt a compressed frame with a password (Argon2id + AEAD).
///
/// Returns CPCE wire-format bytes.
pub fn cpce_encrypt_password(
    compressed_frame: &[u8],
    password: &[u8],
    algo: AeadAlgorithm,
) -> CpacResult<Vec<u8>> {
    let (salt, nonce, ciphertext) =
        cpac_crypto::encrypt_with_password(compressed_frame, password, algo)?;

    // Build CPCE frame
    let total = 4 + 1 + 1 + 1 // magic + version + enc_mode + algo
        + 1 + salt.len()      // salt_len + salt
        + 1 + nonce.len()     // nonce_len + nonce
        + 8                   // payload_len
        + ciphertext.len(); // encrypted payload

    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(CPCE_MAGIC);
    out.push(CPCE_VERSION);
    out.push(ENC_MODE_PASSWORD);
    out.push(algo.id());
    out.push(salt.len() as u8);
    out.extend_from_slice(&salt);
    out.push(nonce.len() as u8);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&(ciphertext.len() as u64).to_le_bytes());
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypt a CPCE password-mode frame.
///
/// Returns the inner compressed frame (CPAC or streaming).
pub fn cpce_decrypt_password(cpce_data: &[u8], password: &[u8]) -> CpacResult<Vec<u8>> {
    let (enc_mode, algo, offset) = parse_cpce_header(cpce_data)?;
    if enc_mode != ENC_MODE_PASSWORD {
        return Err(CpacError::Encryption(format!(
            "expected password mode (0x01), got 0x{enc_mode:02x}"
        )));
    }

    // Parse salt
    if offset >= cpce_data.len() {
        return Err(CpacError::Encryption("truncated CPCE salt".into()));
    }
    let salt_len = cpce_data[offset] as usize;
    let salt_end = offset + 1 + salt_len;
    if salt_end > cpce_data.len() {
        return Err(CpacError::Encryption("truncated CPCE salt data".into()));
    }
    let salt = &cpce_data[offset + 1..salt_end];

    // Parse nonce + payload
    parse_nonce_and_decrypt(cpce_data, salt_end, algo, |nonce, ciphertext| {
        cpac_crypto::decrypt_with_password(ciphertext, password, salt, nonce, algo)
    })
}

// ---------------------------------------------------------------------------
// PQC Hybrid encrypt / decrypt
// ---------------------------------------------------------------------------

/// Encrypt a compressed frame with PQC hybrid (X25519 + ML-KEM-768 + AEAD).
///
/// `recipient_public_key`: x25519_pub(32B) ++ mlkem_pub(N B).
/// Returns CPCE wire-format bytes.
#[cfg(feature = "pqc")]
pub fn cpce_encrypt_pqc(
    compressed_frame: &[u8],
    recipient_public_key: &[u8],
) -> CpacResult<Vec<u8>> {
    if recipient_public_key.len() < 33 {
        return Err(CpacError::Encryption(
            "recipient public key too short (need x25519 + mlkem)".into(),
        ));
    }
    let x25519_pub = &recipient_public_key[..32];
    let mlkem_pub = &recipient_public_key[32..];

    // Use hybrid encrypt which handles ephemeral keygen, encapsulation, and AEAD
    let cphe_data = cpac_crypto::hybrid::hybrid_encrypt(compressed_frame, x25519_pub, mlkem_pub)?;

    // Extract components from CPHE frame to re-encode as CPCE
    // CPHE v2 layout: "CPHE"(4) | version(1) | kem_id(1) | aead_id(1) | kdf_id(1)
    //                 | eph_pub(32) | mlkem_ct_len(2LE) | mlkem_ct
    //                 | nonce_len(1) | nonce | ciphertext
    // CPHE v1 layout: "CPHE"(4) | version(1) | eph_pub(32) | mlkem_ct_len(2LE) | ...
    if cphe_data.len() < 5 {
        return Err(CpacError::Encryption("CPHE output too short".into()));
    }
    let cphe_version = cphe_data[4];
    let data_start: usize = match cphe_version {
        cpac_crypto::hybrid::CPHE_VERSION => 8, // v2: 3 algo-ID bytes after version
        cpac_crypto::hybrid::CPHE_VERSION_V1 => 5, // v1: data right after version
        _ => {
            return Err(CpacError::Encryption(format!(
                "unsupported CPHE version {cphe_version}"
            )))
        }
    };
    if cphe_data.len() < data_start + 34 {
        return Err(CpacError::Encryption("CPHE output too short".into()));
    }
    let eph_pub = &cphe_data[data_start..data_start + 32];
    let ct_len_off = data_start + 32;
    let mlkem_ct_len =
        u16::from_le_bytes([cphe_data[ct_len_off], cphe_data[ct_len_off + 1]]) as usize;
    let mlkem_ct_end = ct_len_off + 2 + mlkem_ct_len;
    if cphe_data.len() < mlkem_ct_end + 1 {
        return Err(CpacError::Encryption("CPHE truncated".into()));
    }
    let mlkem_ct = &cphe_data[ct_len_off + 2..mlkem_ct_end];
    let nonce_len = cphe_data[mlkem_ct_end] as usize;
    let nonce_end = mlkem_ct_end + 1 + nonce_len;
    let nonce = &cphe_data[mlkem_ct_end + 1..nonce_end];
    let ciphertext = &cphe_data[nonce_end..];

    let algo = AeadAlgorithm::ChaCha20Poly1305; // hybrid always uses ChaCha20

    let total = 4 + 1 + 1 + 1 // magic + version + enc_mode + algo
        + 32                   // x25519 ephemeral pub
        + 2 + mlkem_ct.len()  // mlkem_ct_len + mlkem_ct
        + 1 + nonce.len()     // nonce_len + nonce
        + 8                   // payload_len
        + ciphertext.len(); // encrypted payload

    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(CPCE_MAGIC);
    out.push(CPCE_VERSION);
    out.push(ENC_MODE_PQC_HYBRID);
    out.push(algo.id());
    out.extend_from_slice(eph_pub);
    out.extend_from_slice(&(mlkem_ct.len() as u16).to_le_bytes());
    out.extend_from_slice(mlkem_ct);
    out.push(nonce.len() as u8);
    out.extend_from_slice(nonce);
    out.extend_from_slice(&(ciphertext.len() as u64).to_le_bytes());
    out.extend_from_slice(ciphertext);
    Ok(out)
}

/// Decrypt a CPCE PQC-hybrid-mode frame.
///
/// `secret_key`: x25519_sec(32B) ++ mlkem_sec(N B).
/// Returns the inner compressed frame.
#[cfg(feature = "pqc")]
pub fn cpce_decrypt_pqc(cpce_data: &[u8], secret_key: &[u8]) -> CpacResult<Vec<u8>> {
    let (enc_mode, algo, offset) = parse_cpce_header(cpce_data)?;
    if enc_mode != ENC_MODE_PQC_HYBRID {
        return Err(CpacError::Encryption(format!(
            "expected PQC hybrid mode (0x02), got 0x{enc_mode:02x}"
        )));
    }

    // Parse ephemeral X25519 public key
    if offset + 32 > cpce_data.len() {
        return Err(CpacError::Encryption(
            "truncated ephemeral public key".into(),
        ));
    }
    let eph_pub = &cpce_data[offset..offset + 32];

    // Parse ML-KEM ciphertext
    let ct_len_offset = offset + 32;
    if ct_len_offset + 2 > cpce_data.len() {
        return Err(CpacError::Encryption("truncated ML-KEM ct length".into()));
    }
    let mlkem_ct_len =
        u16::from_le_bytes([cpce_data[ct_len_offset], cpce_data[ct_len_offset + 1]]) as usize;
    let mlkem_ct_end = ct_len_offset + 2 + mlkem_ct_len;
    if mlkem_ct_end > cpce_data.len() {
        return Err(CpacError::Encryption("truncated ML-KEM ciphertext".into()));
    }
    let mlkem_ct = &cpce_data[ct_len_offset + 2..mlkem_ct_end];

    // Split secret key
    if secret_key.len() < 33 {
        return Err(CpacError::Encryption(
            "secret key too short (need x25519 + mlkem)".into(),
        ));
    }
    let x25519_sec = &secret_key[..32];
    let mlkem_sec = &secret_key[32..];

    // Derive shared secret: X25519 DH
    let x_sec_bytes: [u8; 32] = x25519_sec
        .try_into()
        .map_err(|_| CpacError::Encryption("invalid x25519 secret key".into()))?;
    let our_sk = x25519_dalek::StaticSecret::from(x_sec_bytes);
    let eph_pub_bytes: [u8; 32] = eph_pub
        .try_into()
        .map_err(|_| CpacError::Encryption("invalid ephemeral public key".into()))?;
    let eph_pk = x25519_dalek::PublicKey::from(eph_pub_bytes);
    let x_shared = cpac_crypto::keys::x25519_shared_secret(&our_sk, &eph_pk);

    // ML-KEM decapsulate
    let mlkem_ss = cpac_crypto::pqc::pqc_decapsulate(
        mlkem_ct,
        mlkem_sec,
        cpac_crypto::pqc::PqcAlgorithm::MlKem768,
    )?;

    // Combine shared secrets via HKDF
    let mut ikm = Vec::with_capacity(32 + mlkem_ss.len());
    ikm.extend_from_slice(&x_shared);
    ikm.extend_from_slice(&mlkem_ss);
    let combined_key =
        cpac_crypto::kdf::derive_key_hkdf(&ikm, b"CPHE-hybrid-salt", b"CPHE-hybrid-v1")?;

    // Parse nonce + payload and decrypt
    parse_nonce_and_decrypt(cpce_data, mlkem_ct_end, algo, |nonce, ciphertext| {
        cpac_crypto::aead::decrypt_aead(ciphertext, &combined_key, nonce, algo)
    })
}

/// Auto-detect encryption mode and decrypt a CPCE frame.
///
/// For password mode, reads `CPAC_PASSWORD` env var or returns an error.
/// For PQC mode, requires `secret_key`.
pub fn cpce_auto_decrypt(
    cpce_data: &[u8],
    password: Option<&[u8]>,
    #[cfg(feature = "pqc")] secret_key: Option<&[u8]>,
    #[cfg(not(feature = "pqc"))] _secret_key: Option<&[u8]>,
) -> CpacResult<Vec<u8>> {
    if !is_cpce(cpce_data) {
        return Err(CpacError::Encryption("not a CPCE frame".into()));
    }
    let (enc_mode, _algo, _offset) = parse_cpce_header(cpce_data)?;
    match enc_mode {
        ENC_MODE_PASSWORD => {
            let pw = password.ok_or_else(|| {
                CpacError::Encryption(
                    "CPCE password-encrypted: set CPAC_PASSWORD env var or provide --password"
                        .into(),
                )
            })?;
            cpce_decrypt_password(cpce_data, pw)
        }
        #[cfg(feature = "pqc")]
        ENC_MODE_PQC_HYBRID => {
            let sk = secret_key.ok_or_else(|| {
                CpacError::Encryption(
                    "CPCE PQC-encrypted: provide --encrypt-key with secret key file".into(),
                )
            })?;
            cpce_decrypt_pqc(cpce_data, sk)
        }
        #[cfg(not(feature = "pqc"))]
        ENC_MODE_PQC_HYBRID => Err(CpacError::Encryption(
            "CPCE PQC mode requires the 'pqc' feature".into(),
        )),
        other => Err(CpacError::Encryption(format!(
            "unknown CPCE encryption mode: 0x{other:02x}"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Parse and validate the fixed CPCE header.
/// Returns `(enc_mode, aead_algo, offset_after_header)`.
fn parse_cpce_header(data: &[u8]) -> CpacResult<(u8, AeadAlgorithm, usize)> {
    if data.len() < 7 {
        return Err(CpacError::Encryption("CPCE frame too short".into()));
    }
    if &data[..4] != CPCE_MAGIC {
        return Err(CpacError::Encryption("not a CPCE frame".into()));
    }
    if data[4] != CPCE_VERSION {
        return Err(CpacError::Encryption(format!(
            "unsupported CPCE version: {}",
            data[4]
        )));
    }
    let enc_mode = data[5];
    let algo = AeadAlgorithm::from_id(data[6])?;
    Ok((enc_mode, algo, 7))
}

/// Parse nonce + payload_len + payload, then decrypt with the provided closure.
fn parse_nonce_and_decrypt<F>(
    data: &[u8],
    offset: usize,
    _algo: AeadAlgorithm,
    decrypt_fn: F,
) -> CpacResult<Vec<u8>>
where
    F: FnOnce(&[u8], &[u8]) -> CpacResult<Vec<u8>>,
{
    if offset >= data.len() {
        return Err(CpacError::Encryption("truncated CPCE nonce header".into()));
    }
    let nonce_len = data[offset] as usize;
    let nonce_end = offset + 1 + nonce_len;
    if nonce_end + 8 > data.len() {
        return Err(CpacError::Encryption("truncated CPCE nonce/payload".into()));
    }
    let nonce = &data[offset + 1..nonce_end];

    let payload_len = u64::from_le_bytes([
        data[nonce_end],
        data[nonce_end + 1],
        data[nonce_end + 2],
        data[nonce_end + 3],
        data[nonce_end + 4],
        data[nonce_end + 5],
        data[nonce_end + 6],
        data[nonce_end + 7],
    ]) as usize;

    let payload_start = nonce_end + 8;
    let payload_end = payload_start + payload_len;
    if payload_end > data.len() {
        return Err(CpacError::Encryption("truncated CPCE payload".into()));
    }
    let ciphertext = &data[payload_start..payload_end];

    decrypt_fn(nonce, ciphertext)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_cpce_detection() {
        assert!(is_cpce(b"CPCEsomething"));
        assert!(!is_cpce(b"CPsomething"));
        assert!(!is_cpce(b"CPH"));
        assert!(!is_cpce(b""));
    }

    #[test]
    fn password_chacha_roundtrip() {
        let plaintext = b"Hello, CPCE password encryption!";
        let password = b"test-password-42";

        let encrypted =
            cpce_encrypt_password(plaintext, password, AeadAlgorithm::ChaCha20Poly1305).unwrap();
        assert!(is_cpce(&encrypted));
        assert!(encrypted.len() > plaintext.len()); // overhead

        let decrypted = cpce_decrypt_password(&encrypted, password).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn password_aes_roundtrip() {
        let plaintext = b"AES-256-GCM CPCE test payload";
        let password = b"another-password";

        let encrypted =
            cpce_encrypt_password(plaintext, password, AeadAlgorithm::Aes256Gcm).unwrap();
        assert!(is_cpce(&encrypted));

        let decrypted = cpce_decrypt_password(&encrypted, password).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn password_wrong_password_fails() {
        let plaintext = b"secret data";
        let encrypted =
            cpce_encrypt_password(plaintext, b"right", AeadAlgorithm::ChaCha20Poly1305).unwrap();
        let result = cpce_decrypt_password(&encrypted, b"wrong");
        assert!(result.is_err());
    }

    #[test]
    fn password_large_payload() {
        let plaintext: Vec<u8> = (0u8..=255).cycle().take(100_000).collect();
        let password = b"large-payload-test";
        let encrypted =
            cpce_encrypt_password(&plaintext, password, AeadAlgorithm::ChaCha20Poly1305).unwrap();
        let decrypted = cpce_decrypt_password(&encrypted, password).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn auto_decrypt_password() {
        let plaintext = b"auto-detect password mode";
        let password = b"auto-test";
        let encrypted =
            cpce_encrypt_password(plaintext, password, AeadAlgorithm::ChaCha20Poly1305).unwrap();

        let decrypted = cpce_auto_decrypt(&encrypted, Some(password), None).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn auto_decrypt_no_password_fails() {
        let plaintext = b"need password";
        let encrypted =
            cpce_encrypt_password(plaintext, b"pw", AeadAlgorithm::ChaCha20Poly1305).unwrap();

        let result = cpce_auto_decrypt(&encrypted, None, None);
        assert!(result.is_err());
    }

    #[test]
    fn truncated_frame_fails() {
        assert!(cpce_decrypt_password(b"CPCE", b"pw").is_err());
        assert!(cpce_decrypt_password(b"CPCEx\x01\x01", b"pw").is_err());
    }

    #[test]
    fn compress_then_encrypt_roundtrip() {
        // Simulate real compress+encrypt: compress first, then CPCE encrypt
        let original = b"repeated text repeated text repeated text for compression test!";
        let config = cpac_types::CompressConfig::default();
        let compressed = cpac_engine::compress(original, &config).unwrap();

        let password = b"compress-encrypt-test";
        let encrypted =
            cpce_encrypt_password(&compressed.data, password, AeadAlgorithm::ChaCha20Poly1305)
                .unwrap();
        assert!(is_cpce(&encrypted));

        // Decrypt then decompress
        let decrypted_frame = cpce_decrypt_password(&encrypted, password).unwrap();
        let decompressed = cpac_engine::decompress(&decrypted_frame).unwrap();
        assert_eq!(decompressed.data, original);
    }

    #[cfg(feature = "pqc")]
    mod pqc_tests {
        use super::*;

        #[test]
        fn pqc_roundtrip() {
            let plaintext = b"Post-quantum encrypted CPCE payload!";
            let kp = cpac_crypto::hybrid::hybrid_keygen().unwrap();

            // Build combined pub/sec key blobs
            let mut pub_key = Vec::new();
            pub_key.extend_from_slice(&kp.x25519_public);
            pub_key.extend_from_slice(&kp.mlkem_public);

            let mut sec_key = Vec::new();
            sec_key.extend_from_slice(&kp.x25519_secret);
            sec_key.extend_from_slice(&kp.mlkem_secret);

            let encrypted = cpce_encrypt_pqc(plaintext, &pub_key).unwrap();
            assert!(is_cpce(&encrypted));

            let decrypted = cpce_decrypt_pqc(&encrypted, &sec_key).unwrap();
            assert_eq!(decrypted, plaintext);
        }

        #[test]
        fn pqc_wrong_key_fails() {
            let plaintext = b"secret PQC data";
            let kp1 = cpac_crypto::hybrid::hybrid_keygen().unwrap();
            let kp2 = cpac_crypto::hybrid::hybrid_keygen().unwrap();

            let mut pub1 = Vec::new();
            pub1.extend_from_slice(&kp1.x25519_public);
            pub1.extend_from_slice(&kp1.mlkem_public);

            let mut sec2 = Vec::new();
            sec2.extend_from_slice(&kp2.x25519_secret);
            sec2.extend_from_slice(&kp2.mlkem_secret);

            let encrypted = cpce_encrypt_pqc(plaintext, &pub1).unwrap();
            let result = cpce_decrypt_pqc(&encrypted, &sec2);
            assert!(result.is_err());
        }

        #[test]
        fn pqc_compress_then_encrypt_roundtrip() {
            let original = b"PQC compress+encrypt test with repeated data repeated data!";
            let config = cpac_types::CompressConfig::default();
            let compressed = cpac_engine::compress(original, &config).unwrap();

            let kp = cpac_crypto::hybrid::hybrid_keygen().unwrap();
            let mut pub_key = Vec::new();
            pub_key.extend_from_slice(&kp.x25519_public);
            pub_key.extend_from_slice(&kp.mlkem_public);
            let mut sec_key = Vec::new();
            sec_key.extend_from_slice(&kp.x25519_secret);
            sec_key.extend_from_slice(&kp.mlkem_secret);

            let encrypted = cpce_encrypt_pqc(&compressed.data, &pub_key).unwrap();
            let decrypted_frame = cpce_decrypt_pqc(&encrypted, &sec_key).unwrap();
            let decompressed = cpac_engine::decompress(&decrypted_frame).unwrap();
            assert_eq!(decompressed.data, original);
        }

        #[test]
        fn auto_decrypt_pqc() {
            let plaintext = b"auto-detect PQC mode";
            let kp = cpac_crypto::hybrid::hybrid_keygen().unwrap();
            let mut pub_key = Vec::new();
            pub_key.extend_from_slice(&kp.x25519_public);
            pub_key.extend_from_slice(&kp.mlkem_public);
            let mut sec_key = Vec::new();
            sec_key.extend_from_slice(&kp.x25519_secret);
            sec_key.extend_from_slice(&kp.mlkem_secret);

            let encrypted = cpce_encrypt_pqc(plaintext, &pub_key).unwrap();
            let decrypted = cpce_auto_decrypt(&encrypted, None, Some(&sec_key)).unwrap();
            assert_eq!(decrypted, plaintext);
        }
    }
}
