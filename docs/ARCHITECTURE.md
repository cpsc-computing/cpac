# CPAC Architecture

## Compression Pipeline

```
Input data
  │
  ▼
┌─────────────┐
│   SSR        │  Structural Summary Record analysis
│   Analysis   │  → entropy, ASCII ratio, track, domain hint
└──────┬──────┘
       │
       ▼
┌─────────────┐
│  Preprocess  │  Transform selection & application
│  (Transforms)│  TP frame or DAG-driven chain
└──────┬──────┘
       │
       ▼
┌─────────────┐
│   Entropy    │  Zstd / Brotli / Raw
│   Coding     │  Auto-selected by entropy estimate
└──────┬──────┘
       │
       ▼
┌─────────────┐
│    Frame     │  CP wire format (self-describing)
│   Encoding   │  12-byte header + DAG descriptor + payload
└──────┬──────┘
       │
       ▼
  .cpac file
```

Decompression reverses the pipeline: decode frame → entropy decompress →
reverse transforms → original data. Size verification at every stage.

## Crate Dependency Graph

```
cpac-types (leaf — no internal deps)
  ├── cpac-ssr
  ├── cpac-transforms
  │     └── cpac-dag
  ├── cpac-entropy
  ├── cpac-frame
  └── cpac-engine (top-level API)
        ├── depends on: types, ssr, transforms, dag, entropy, frame
        ├── cpac-streaming (depends on: types, engine)
        ├── cpac-crypto (depends on: types)
        ├── cpac-domains (depends on: types, ssr, engine)
        ├── cpac-cas (depends on: types)
        └── cpac-archive (depends on: types, engine)

cpac-cli (binary — depends on all of the above)
```

No circular dependencies. `cpac-types` is always the leaf.

## Key Design Decisions

### SSR-Guided Adaptive Pipeline
Every input is analyzed by the Structural Summary Record (SSR) module
before compression. SSR computes entropy, ASCII ratio, data size, and
domain hints. These metrics drive automatic backend selection (low
entropy → Zstd, medium → Brotli, high → Raw) and transform selection.

### Two Transform Systems
1. **TP preprocess** — the default path. SSR metrics select a single
   transform (transpose, float-split, field-LZ, or ROLZ) and encode it
   in a self-describing TP frame. Simple, fast, always-on.
2. **DAG profiles** — for advanced use. Named profiles (e.g., "generic",
   "text", "binary") compile an ordered chain of transforms into a DAG.
   The DAG can also auto-select transforms by estimated gain.

### SIMD Tiered Dispatch
Transform kernels in `simd.rs` use runtime CPU feature detection:
AVX-512 (64B) → AVX2 (32B) → SSE4.1 (16B) → SSE2 (16B) → NEON → scalar.
Each `*_fast()` function probes and dispatches to the best available tier.
The scalar fallback is always correct.

### Block-Parallel Architecture
For inputs > 256 KiB, the engine splits data into 1 MiB blocks and
compresses each independently via rayon. The CPBL wire format stores
a block size table for random-access decompression. Each block is a
complete CP frame, enabling fully parallel decompression.

### Safe Resource Defaults
`auto_resource_config()` detects the host system and sets:
- Threads = physical core count (not hyperthreaded)
- Memory cap = 25% of system RAM, clamped to 256 MB – 8 GB
CLI flags `--threads` and `--max-memory` override these defaults.

### Defence-in-Depth Encryption
The CPHE hybrid encryption format combines:
- X25519 (classical Diffie-Hellman)
- ML-KEM-768 (NIST post-quantum KEM)
Both shared secrets are combined via HKDF-SHA256 before AEAD encryption.
Even if one primitive is broken, the other still protects confidentiality.

### Wire Format Stability
All wire formats use magic-byte identification and version fields.
The Rust engine must produce frames decompressible by the Python
engine and vice-versa. Format changes require version bumps.

## File Extensions

- `.cpac` — compressed file (CP or CPBL frame)
- `.cpar` — multi-file archive (CPAR frame)
- `.cpac-enc` — password-encrypted (AEAD)
- `.cpac-pqe` — hybrid PQC-encrypted (CPHE frame)
- `.cpac-sig` — ML-DSA-65 digital signature
- `.cpac-pub` / `.cpac-sec` — public/secret key files

## Future Work (TODO)

- GPU acceleration (compute shader compression kernels)
- Deduplication-aware archive mode
- Network streaming protocol
- FUSE/virtual filesystem mount for .cpar archives
- WebAssembly target for browser-side compression
