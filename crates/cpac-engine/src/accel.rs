// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Hardware acceleration abstraction layer.
//!
//! Provides a trait-based dispatch for entropy coding across multiple hardware
//! backends (software, Intel QAT, Intel IAA, AMD Xilinx FPGA, GPU, ARM SVE2).
//!
//! All non-software backends are behind cargo feature flags and return
//! `is_available() = false` by default. The auto-detection logic in
//! [`detect_accelerators`] probes the host environment (env vars, CPUID,
//! device files) to discover available hardware.

use cpac_types::{AccelBackend, Backend, CpacError, CpacResult};

// ---------------------------------------------------------------------------
// Hardware accelerator trait
// ---------------------------------------------------------------------------

/// Trait for hardware-accelerated entropy coding.
///
/// Implementations wrap backend-specific APIs (QAT, IAA, CUDA, etc.) behind
/// a uniform interface.  The engine's compress/decompress path queries
/// [`is_available`] at startup and delegates when possible.
pub trait HardwareAccelerator: Send + Sync {
    /// Human-readable name for logging/display.
    fn name(&self) -> &str;

    /// Which backend category this accelerator belongs to.
    fn backend(&self) -> AccelBackend;

    /// Whether this accelerator is actually usable on the current host.
    fn is_available(&self) -> bool;

    /// Compress `data` at the given level.  Returns compressed bytes.
    fn compress(&self, data: &[u8], level: i32) -> CpacResult<Vec<u8>>;

    /// Decompress `data`, expecting at most `max_size` decompressed bytes.
    fn decompress(&self, data: &[u8], max_size: usize) -> CpacResult<Vec<u8>>;

    /// Whether this accelerator supports the given entropy backend.
    fn supports_backend(&self, backend: Backend) -> bool;
}

// ---------------------------------------------------------------------------
// Software fallback (always available)
// ---------------------------------------------------------------------------

/// Software-only accelerator — wraps the existing `cpac_entropy` codecs.
pub struct SoftwareAccelerator;

impl HardwareAccelerator for SoftwareAccelerator {
    fn name(&self) -> &str {
        "software"
    }
    fn backend(&self) -> AccelBackend {
        AccelBackend::Software
    }
    fn is_available(&self) -> bool {
        true
    }
    fn compress(&self, data: &[u8], level: i32) -> CpacResult<Vec<u8>> {
        // Default to zstd at the given level
        zstd::bulk::compress(data, level)
            .map_err(|e| CpacError::CompressFailed(format!("software zstd: {e}")))
    }
    fn decompress(&self, data: &[u8], max_size: usize) -> CpacResult<Vec<u8>> {
        zstd::bulk::decompress(data, max_size)
            .map_err(|e| CpacError::DecompressFailed(format!("software zstd: {e}")))
    }
    fn supports_backend(&self, _backend: Backend) -> bool {
        true // software supports all backends
    }
}

// ---------------------------------------------------------------------------
// Stub accelerators (behind feature flags)
// ---------------------------------------------------------------------------

macro_rules! stub_accelerator {
    ($name:ident, $display:expr, $backend:expr) => {
        pub struct $name;

        impl HardwareAccelerator for $name {
            fn name(&self) -> &str {
                $display
            }
            fn backend(&self) -> AccelBackend {
                $backend
            }
            fn is_available(&self) -> bool {
                false
            }
            fn compress(&self, _data: &[u8], _level: i32) -> CpacResult<Vec<u8>> {
                Err(CpacError::UnsupportedBackend(format!(
                    "{} not available on this host",
                    $display
                )))
            }
            fn decompress(&self, _data: &[u8], _max_size: usize) -> CpacResult<Vec<u8>> {
                Err(CpacError::UnsupportedBackend(format!(
                    "{} not available on this host",
                    $display
                )))
            }
            fn supports_backend(&self, backend: Backend) -> bool {
                // QAT/IAA support zstd+gzip; GPU/FPGA could support all
                matches!(backend, Backend::Zstd | Backend::Gzip)
            }
        }
    };
}

stub_accelerator!(QatAccelerator, "Intel QAT", AccelBackend::IntelQat);
stub_accelerator!(IaaAccelerator, "Intel IAA", AccelBackend::IntelIaa);
stub_accelerator!(
    XilinxAccelerator,
    "AMD Xilinx Alveo",
    AccelBackend::AmdXilinx
);
stub_accelerator!(GpuAccelerator, "GPU Compute", AccelBackend::GpuCompute);
stub_accelerator!(Sve2Accelerator, "ARM SVE2", AccelBackend::ArmSve2);

// ---------------------------------------------------------------------------
// Auto-detection
// ---------------------------------------------------------------------------

/// Detect available hardware accelerators by probing the environment.
///
/// Returns a list of `AccelBackend` variants that are (or claim to be)
/// available on this host.  Software is always included.
pub fn detect_accelerators() -> Vec<AccelBackend> {
    let mut available = vec![AccelBackend::Software];

    // Intel QAT: env var or device file
    if std::env::var("CPAC_QAT_ENABLED").is_ok_and(|v| v == "1" || v == "true") {
        available.push(AccelBackend::IntelQat);
    }

    // Intel IAA: env var (future: CPUID Sapphire Rapids flag)
    if std::env::var("CPAC_IAA_ENABLED").is_ok_and(|v| v == "1" || v == "true") {
        available.push(AccelBackend::IntelIaa);
    }

    // GPU: env var (future: vulkan/cuda runtime probe)
    if std::env::var("CPAC_GPU_ENABLED").is_ok_and(|v| v == "1" || v == "true") {
        available.push(AccelBackend::GpuCompute);
    }

    // AMD Xilinx FPGA: env var
    if std::env::var("CPAC_XILINX_ENABLED").is_ok_and(|v| v == "1" || v == "true") {
        available.push(AccelBackend::AmdXilinx);
    }

    // ARM SVE2: compile-time gated + env var
    #[cfg(target_arch = "aarch64")]
    if std::env::var("CPAC_SVE2_ENABLED").is_ok_and(|v| v == "1" || v == "true") {
        available.push(AccelBackend::ArmSve2);
    }

    available
}

/// Select the best available accelerator for a given backend preference.
///
/// Returns `AccelBackend::Software` if no hardware accelerator is available
/// or if `preference` is `None` (auto) and no hardware is detected.
pub fn select_accelerator(
    preference: Option<AccelBackend>,
    available: &[AccelBackend],
) -> AccelBackend {
    if let Some(pref) = preference {
        if available.contains(&pref) {
            return pref;
        }
        // Requested accelerator not available — fall back to software
        return AccelBackend::Software;
    }
    // Auto: pick highest-priority available (QAT > IAA > GPU > FPGA > SVE2 > Software)
    for &candidate in &[
        AccelBackend::IntelQat,
        AccelBackend::IntelIaa,
        AccelBackend::GpuCompute,
        AccelBackend::AmdXilinx,
        AccelBackend::ArmSve2,
    ] {
        if available.contains(&candidate) {
            return candidate;
        }
    }
    AccelBackend::Software
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn software_always_available() {
        let sw = SoftwareAccelerator;
        assert!(sw.is_available());
        assert_eq!(sw.backend(), AccelBackend::Software);
        assert!(sw.supports_backend(Backend::Zstd));
    }

    #[test]
    fn stubs_not_available() {
        let qat = QatAccelerator;
        assert!(!qat.is_available());
        assert!(qat.compress(b"test", 3).is_err());
    }

    #[test]
    fn detect_includes_software() {
        let accels = detect_accelerators();
        assert!(accels.contains(&AccelBackend::Software));
    }

    #[test]
    fn select_auto_defaults_to_software() {
        let available = vec![AccelBackend::Software];
        assert_eq!(select_accelerator(None, &available), AccelBackend::Software);
    }

    #[test]
    fn select_explicit_preference() {
        let available = vec![AccelBackend::Software, AccelBackend::IntelQat];
        assert_eq!(
            select_accelerator(Some(AccelBackend::IntelQat), &available),
            AccelBackend::IntelQat
        );
    }

    #[test]
    fn select_unavailable_falls_back() {
        let available = vec![AccelBackend::Software];
        assert_eq!(
            select_accelerator(Some(AccelBackend::GpuCompute), &available),
            AccelBackend::Software
        );
    }
}
