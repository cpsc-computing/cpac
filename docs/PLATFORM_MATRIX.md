# Datacenter Platform Matrix & FPGA/RTL Acceleration

## Platform Support Matrix

### Operating Systems

| OS | Status | Notes |
|---|---|---|
| Linux x86_64 (glibc) | Primary | CI + production target |
| Linux x86_64 (musl) | Supported | Static binaries via `cross` |
| Linux aarch64 | Supported | AWS Graviton, Ampere Altra |
| Windows x86_64 | Supported | Development + edge deployment |
| macOS x86_64 | Supported | Intel Mac |
| macOS aarch64 | Supported | Apple Silicon (M-series) |
| FreeBSD x86_64 | Planned | Community request |

### CPU Architectures & SIMD

| Architecture | SIMD Tier | Acceleration |
|---|---|---|
| x86_64 AVX2 | Tier 1 | 32B/iter SSR, parallel BLAKE3, Zstd SIMD |
| x86_64 SSE2 | Tier 2 | 16B/iter SSR fallback |
| x86_64 AVX-512 | Planned | 64B/iter SSR, wider histogram |
| aarch64 NEON | Tier 2 | 16B/iter SSR, ARM crypto extensions |
| aarch64 SVE/SVE2 | Planned | Variable-width SIMD for Graviton3+ |
| RISC-V V | Research | Vector extension support |
| WASM (wasm32) | Planned | Browser/edge deployment via wasm-pack |

### Hardware Acceleration Backends

| Backend | Feature Flag | Status |
|---|---|---|
| CPU (software) | (default) | Production |
| Intel QAT | `accel-qat` | Planned — zlib/deflate offload |
| NVIDIA nvCOMP | `accel-nvcomp` | Planned — GPU LZ4/Snappy/ANS |
| AMD Alveo/Xilinx | `accel-fpga` | Research (see FPGA section below) |
| AWS Graviton LZMA | `accel-graviton` | Planned — ARM LZMA instructions |

## FPGA/RTL Acceleration Plan

### Candidate Offload Operations

1. **Entropy Coding** (Zstd FSE/Huffman encode/decode)
   - Highest throughput gain potential: 10-50 GB/s on modern FPGAs
   - Well-studied in academic literature
   - Target: Xilinx Alveo U250 or Intel Agilex

2. **BLAKE3 Hashing** (dedup fingerprinting)
   - Embarrassingly parallel: 16 lanes on FPGA
   - Used in CDC dedup pipeline and integrity checks

3. **CDC Chunking** (Gear hash boundary detection)
   - Simple rolling hash → ideal for hardware pipeline
   - Can run at wire speed (100 Gbps NIC → FPGA → chunked output)

4. **ML-KEM Encapsulation** (PQC lattice operations)
   - NTT (Number Theoretic Transform) is well-suited to FPGA
   - Potential 100× speedup over CPU for high-volume key exchange

### Integration Architecture

```
                    ┌─────────────┐
  Input ──────────►│  FPGA Card   │──────► Compressed Output
                    │             │
                    │ ┌─────────┐ │
                    │ │ Entropy │ │  ← Zstd FSE in hardware
                    │ │ Engine  │ │
                    │ └─────────┘ │
                    │ ┌─────────┐ │
                    │ │ BLAKE3  │ │  ← Dedup hashing
                    │ │ Hasher  │ │
                    │ └─────────┘ │
                    │ ┌─────────┐ │
                    │ │ CDC     │ │  ← Gear hash chunker
                    │ │ Splitter│ │
                    │ └─────────┘ │
                    └─────────────┘
                          │
                    PCIe/CXL DMA
```

### Test Harness Strategy

1. **Simulation**: Verilator/cocotb testbench for each RTL module
2. **Functional Model**: Rust golden model (existing `cpac-engine` code)
3. **Bit-exact Comparison**: FPGA output must match CPU output byte-for-byte
4. **Throughput Benchmarks**: PCIe bandwidth saturation tests
5. **Latency Profiling**: Per-operation cycle counts via AXI perf counters

### RTL Repository

The FPGA/RTL implementations live in the separate `cpsc-engine-rtl` repository
and interface with CPAC via the `cpac-accel` crate's `AccelBackend::Fpga` variant.

### Timeline

- **Phase 1** (current): Document architecture, define interfaces
- **Phase 2**: Verilator simulation of Gear-hash CDC splitter
- **Phase 3**: BLAKE3 hash pipeline RTL + cocotb tests
- **Phase 4**: Entropy engine (Zstd FSE) RTL prototype
- **Phase 5**: PCIe DMA integration on Alveo U250
