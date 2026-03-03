# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | Yes       |

## Reporting a Vulnerability

**Do not file security vulnerabilities as public GitHub issues.**

Please report security issues by emailing: **security@bitconcepts.com**

Include:
- Description of the vulnerability
- Steps to reproduce
- Potential impact assessment
- Suggested fix (if any)

We will acknowledge receipt within 48 hours and provide a detailed
response within 7 business days.

## Scope

The following are in scope for security reports:
- Memory safety issues in compression/decompression
- Cryptographic implementation flaws (AEAD, KDF, PQC, hybrid encryption)
- Wire format parsing vulnerabilities (buffer overflows, integer overflows)
- Key material leakage
- Denial of service via crafted input

## Encryption Algorithms

CPAC uses the following cryptographic primitives:
- ChaCha20-Poly1305 (AEAD)
- AES-256-GCM (AEAD)
- X25519 (key exchange)
- ML-KEM-768 (post-quantum KEM, FIPS 203)
- ML-DSA-65 (post-quantum signatures, FIPS 204)
- Argon2id (password KDF)
- HKDF-SHA256 (key derivation)

All implementations use audited Rust crates from the RustCrypto project.
