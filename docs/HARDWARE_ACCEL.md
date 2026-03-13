# Hardware Acceleration

CPAC supports hardware-accelerated entropy coding through a pluggable
accelerator abstraction layer (`cpac-engine/src/accel.rs`).

## Supported Backends

| Backend | Feature Flag | Detection | Status |
|---------|-------------|-----------|--------|
| Software | (always on) | — | Production |
| Intel QAT | `accel-qat` | `/dev/qat_*`, `CPAC_QAT_ENABLED=1` | Stub |
| Intel IAA | `accel-iaa` | `/dev/iax*`, `CPAC_IAA_ENABLED=1` | Stub |
| GPU (CUDA/Vulkan) | `accel-gpu` | `libcuda.so`/`nvcuda.dll`, `CPAC_GPU_ENABLED=1` | Stub |
| AMD Xilinx FPGA | — | `CPAC_XILINX_ENABLED=1` | Stub |
| ARM SVE2 | `accel-sve2` | aarch64 + `CPAC_SVE2_ENABLED=1` | Stub |

## Enabling Accelerators

### Environment Variables

All accelerators can be force-enabled via environment variables:

```sh
export CPAC_QAT_ENABLED=1      # Intel QAT
export CPAC_IAA_ENABLED=1      # Intel IAA (Sapphire Rapids+)
export CPAC_GPU_ENABLED=1      # GPU Compute (CUDA/Vulkan)
export CPAC_XILINX_ENABLED=1   # AMD Xilinx Alveo FPGA
export CPAC_SVE2_ENABLED=1     # ARM SVE2 (AArch64 only)
```

### Cargo Feature Flags

Build with a specific accelerator enabled:

```sh
cargo build --release -p cpac-cli --features accel-qat
cargo build --release -p cpac-cli --features accel-gpu
```

### CLI Flag

```sh
cpac compress input.dat --accel qat
cpac compress input.dat --accel auto    # default: auto-detect best available
cpac info --host                        # show detected accelerators
```

## Auto-Detection

`detect_accelerators()` probes the host at runtime:

1. **Intel QAT**: Checks for `/dev/qat_*` device files (Linux) or env var
2. **Intel IAA**: Checks for `/dev/iax*` device files (Linux idxd driver) or env var
3. **GPU**: Checks for `libcuda.so` (Linux) or `nvcuda.dll` (Windows) or env var
4. **Xilinx**: Env var only (no device probing yet)
5. **ARM SVE2**: Compile-time `target_arch = "aarch64"` gate + env var

The `select_accelerator()` function picks the highest-priority available
backend: QAT > IAA > GPU > FPGA > SVE2 > Software.

## Architecture

The `HardwareAccelerator` trait provides a uniform interface:

```rust
pub trait HardwareAccelerator: Send + Sync {
    fn name(&self) -> &str;
    fn backend(&self) -> AccelBackend;
    fn is_available(&self) -> bool;
    fn compress(&self, data: &[u8], level: i32) -> CpacResult<Vec<u8>>;
    fn decompress(&self, data: &[u8], max_size: usize) -> CpacResult<Vec<u8>>;
    fn supports_backend(&self, backend: Backend) -> bool;
}
```

Each hardware backend implements this trait. Non-software backends are
currently stubs that return `is_available() = false` and error on
compress/decompress. Future SDK integration will replace the stubs.

## Future SDK Requirements

- **Intel QAT**: `qatlib` userspace driver + `qat_hw` backend
- **Intel IAA**: `idxd` kernel driver + `accel-config` userspace tool
- **CUDA**: NVIDIA `nvcomp` library for GPU-accelerated compression
- **Xilinx**: Vitis Alveo runtime + FPGA bitstream for zstd/deflate
- **ARM SVE2**: SVE2-capable kernel + compiler support for wide-vector ops
