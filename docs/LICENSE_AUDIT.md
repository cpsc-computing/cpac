# Third-Party License Audit & SBOM

## Overview

This document tracks the license compliance status of all direct and transitive
dependencies used by CPAC. An SBOM (Software Bill of Materials) is generated
automatically via `cargo-deny` and `cargo-about`.

## Audit Methodology

1. **cargo-deny** — policy enforcement (deny GPL, require OSI-approved)
2. **cargo-about** — generate HTML/JSON third-party notice file
3. **cargo-license** — quick summary of all dependency licenses
4. **Manual review** — for any non-standard or dual-licensed crates

## License Policy

### Allowed Licenses
- MIT
- Apache-2.0
- BSD-2-Clause / BSD-3-Clause
- ISC
- Zlib
- CC0-1.0 / Unlicense (public domain)
- MPL-2.0 (with notice requirements)

### Prohibited Licenses
- GPL-2.0 / GPL-3.0 (copyleft incompatible with proprietary licensing)
- AGPL-3.0
- SSPL
- Any "non-commercial" or "evaluation only" restriction (except CPAC's own)

### Special Cases
- **OpenSSL License**: Allowed for optional crypto backends
- **Unicode License**: Allowed (used by regex/unicode-* crates)
- **ring**: ISC-style — allowed

## Generating the SBOM

```bash
# Install tools
cargo install cargo-deny cargo-about cargo-license

# Run license check
cargo deny check licenses

# Generate third-party notice
cargo about generate about.hbs > THIRD_PARTY_NOTICES.html

# Quick license summary
cargo license --all-features --avoid-dev-deps > LICENSES.txt
```

## cargo-deny Configuration

See `deny.toml` in the workspace root:

```toml
[licenses]
unlicensed = "deny"
allow = [
    "MIT",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Zlib",
    "CC0-1.0",
    "Unlicense",
    "MPL-2.0",
    "Unicode-DFS-2016",
    "OpenSSL",
]
deny = ["GPL-2.0", "GPL-3.0", "AGPL-3.0"]
```

## Known Dependencies by Category

### Compression
- zstd / zstd-sys: BSD-3-Clause
- brotli: MIT/Apache-2.0
- lz4-flex: MIT

### Cryptography
- chacha20poly1305: MIT/Apache-2.0
- aes-gcm: MIT/Apache-2.0
- ml-kem: MIT/Apache-2.0
- ml-dsa: MIT/Apache-2.0
- ed25519-dalek: BSD-3-Clause
- x25519-dalek: BSD-3-Clause
- blake3: CC0-1.0/Apache-2.0
- argon2: MIT/Apache-2.0
- pqcrypto-hqc: MIT/Apache-2.0 (PQClean: public domain)

### Serialization
- serde / serde_json: MIT/Apache-2.0
- rmp-serde: MIT
- toml: MIT/Apache-2.0

### CLI / IO
- clap: MIT/Apache-2.0
- indicatif: MIT
- rayon: MIT/Apache-2.0
- memmap2: MIT/Apache-2.0

### Cloud (optional)
- aws-sdk-s3: Apache-2.0
- tokio: MIT

## Compliance Checklist

- [ ] Run `cargo deny check` on every CI build
- [ ] Regenerate THIRD_PARTY_NOTICES.html before each release
- [ ] Review new transitive deps when updating Cargo.lock
- [ ] Maintain this document when adding new dependencies
- [ ] Include THIRD_PARTY_NOTICES.html in binary distributions
