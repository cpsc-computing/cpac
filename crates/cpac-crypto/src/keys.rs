// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Key exchange: X25519 Diffie-Hellman.

use rand::rngs::OsRng;
use x25519_dalek::{PublicKey, StaticSecret};

/// X25519 keypair.
pub struct X25519KeyPair {
    /// Secret key (32 bytes).
    pub secret: StaticSecret,
    /// Public key (32 bytes).
    pub public: PublicKey,
}

/// Generate a new X25519 keypair.
pub fn generate_x25519_keypair() -> X25519KeyPair {
    let secret = StaticSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);
    X25519KeyPair { secret, public }
}

/// Compute shared secret from our secret key and their public key.
pub fn x25519_shared_secret(our_secret: &StaticSecret, their_public: &PublicKey) -> [u8; 32] {
    let shared = our_secret.diffie_hellman(their_public);
    *shared.as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_exchange_roundtrip() {
        let alice = generate_x25519_keypair();
        let bob = generate_x25519_keypair();

        let alice_shared = x25519_shared_secret(&alice.secret, &bob.public);
        let bob_shared = x25519_shared_secret(&bob.secret, &alice.public);

        assert_eq!(alice_shared, bob_shared);
    }

    #[test]
    fn different_keys_different_secrets() {
        let alice = generate_x25519_keypair();
        let bob = generate_x25519_keypair();
        let eve = generate_x25519_keypair();

        let ab = x25519_shared_secret(&alice.secret, &bob.public);
        let ae = x25519_shared_secret(&alice.secret, &eve.public);
        assert_ne!(ab, ae);
    }
}
