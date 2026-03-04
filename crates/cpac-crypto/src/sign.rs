// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
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
