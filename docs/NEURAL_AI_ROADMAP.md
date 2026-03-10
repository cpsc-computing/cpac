# Neural/AI Compressor Integration Roadmap

## Overview

CPAC's modular backend system (`cpac-entropy`) can be extended with neural
compression adapters. This document assesses candidate neural compressors and
defines the integration path.

## Candidate Systems

### CoMERA (Meta, 2024)
- Tensor-decomposed neural compressor using LoRA-style weight sharing
- Reported 17× GPU memory reduction vs baseline neural codecs
- Targets: large model weights, embeddings, feature stores
- Integration: ONNX Runtime or libtorch FFI behind `neural` feature flag

### Learned Entropy Models (Google, 2023–2025)
- ANS-based entropy coder with learned context models
- Higher ratio than Zstd on structured data at the cost of 10–100× slower encode
- Integration: pre-trained model files + C API via `neural-ans` feature

### NNCP (Neural Network Compression Proxy)
- Lightweight neural predictor for byte-level compression
- Competitive with Zstd on text at ~10 MB/s encode
- Integration: Rust native via `tch-rs` or ONNX

## Integration Architecture

```
cpac-entropy
├── zstd.rs          (existing)
├── brotli.rs        (existing)
├── raw.rs           (existing)
└── neural/
    ├── mod.rs       — NeuralBackend trait
    ├── comera.rs    — CoMERA adapter (feature: neural-comera)
    ├── nncp.rs      — NNCP adapter (feature: neural-nncp)
    └── models/      — pre-trained model weights (git-lfs)
```

### NeuralBackend Trait

```rust
pub trait NeuralBackend: Send + Sync {
    fn name(&self) -> &str;
    fn compress(&self, data: &[u8]) -> CpacResult<Vec<u8>>;
    fn decompress(&self, data: &[u8]) -> CpacResult<Vec<u8>>;
    fn estimated_ratio(&self, sample: &[u8]) -> f64;
    fn supports_gpu(&self) -> bool;
}
```

## Benchmarking Strategy

1. Add neural codec entries to `benchmark-external.ps1` (already supports custom codecs)
2. Measure: ratio, encode throughput, decode throughput, GPU utilisation, VRAM
3. Compare against CPAC's traditional pipeline on same corpus
4. Key question: when does the ratio advantage justify 10–100× slower encode?

## Timeline

- **Phase 1** (current): Document architecture and trait definition
- **Phase 2**: ONNX Runtime integration spike (CoMERA weights)
- **Phase 3**: Benchmark neural vs classical on DC corpus
- **Phase 4**: Production-ready adapter behind feature flag

## Dependencies

- `ort` (ONNX Runtime bindings for Rust) — optional
- `tch` (libtorch bindings) — optional, for NNCP
- Pre-trained model weights via git-lfs or download on first use
