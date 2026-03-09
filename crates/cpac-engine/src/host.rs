// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Host detection: CPU features, cores, memory, OS.
//!
//! Called once on startup to determine optimal compression settings
//! (SIMD dispatch, thread count, memory limits).

use std::fmt;

/// Detected host system information.
#[derive(Clone, Debug)]
pub struct HostInfo {
    /// CPU vendor string (e.g. "`GenuineIntel`", "`AuthenticAMD`").
    pub cpu_vendor: String,
    /// CPU brand/model string.
    pub cpu_brand: String,
    /// Number of physical CPU cores.
    pub physical_cores: usize,
    /// Number of logical CPU cores (with hyperthreading).
    pub logical_cores: usize,
    /// Total system RAM in megabytes.
    pub total_ram_mb: u64,
    /// Operating system name.
    pub os_name: String,
    /// Operating system version.
    pub os_version: String,
    /// CPU architecture (`x86_64`, aarch64, etc.).
    pub arch: String,
    /// Detected SIMD feature flags.
    pub cpu_features: Vec<String>,
    /// Best SIMD tier available.
    pub simd_tier: SimdTier,
    /// Hardware accelerators detected on this host.
    pub available_accelerators: Vec<cpac_types::AccelBackend>,
}

/// SIMD capability tier for dispatch decisions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SimdTier {
    /// No SIMD (scalar only).
    Scalar,
    /// ARM NEON (128-bit).
    Neon,
    /// x86 SSE2 (128-bit).
    Sse2,
    /// x86 SSE4.1 (128-bit, blend/extract).
    Sse41,
    /// x86 AVX2 (256-bit).
    Avx2,
    /// x86 AVX-512 (512-bit).
    Avx512,
    /// ARM SVE2 (scalable, 128-2048 bit).
    Sve2,
}

impl fmt::Display for SimdTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SimdTier::Scalar => write!(f, "Scalar"),
            SimdTier::Neon => write!(f, "NEON"),
            SimdTier::Sse2 => write!(f, "SSE2"),
            SimdTier::Sse41 => write!(f, "SSE4.1"),
            SimdTier::Avx2 => write!(f, "AVX2"),
            SimdTier::Avx512 => write!(f, "AVX-512"),
            SimdTier::Sve2 => write!(f, "SVE2"),
        }
    }
}

/// Detect host system information.
///
/// Queries CPU features, core counts, RAM, and OS at runtime.
/// This should be called once at startup and cached.
#[must_use]
pub fn detect_host() -> HostInfo {
    let (cpu_vendor, cpu_brand) = detect_cpu_info();
    let (physical_cores, logical_cores) = detect_core_counts();
    let total_ram_mb = detect_total_ram_mb();
    let cpu_features = detect_cpu_features();
    let simd_tier = determine_simd_tier(&cpu_features);

    let available_accelerators = crate::accel::detect_accelerators();

    HostInfo {
        cpu_vendor,
        cpu_brand,
        physical_cores,
        logical_cores,
        total_ram_mb,
        os_name: std::env::consts::OS.to_string(),
        os_version: sysinfo::System::os_version().unwrap_or_else(|| "unknown".into()),
        arch: std::env::consts::ARCH.to_string(),
        cpu_features,
        simd_tier,
        available_accelerators,
    }
}

impl fmt::Display for HostInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "CPAC Host Detection")?;
        writeln!(f, "  CPU:       {} {}", self.cpu_vendor, self.cpu_brand)?;
        writeln!(
            f,
            "  Cores:     {} physical, {} logical",
            self.physical_cores, self.logical_cores
        )?;
        writeln!(f, "  RAM:       {} MB", self.total_ram_mb)?;
        writeln!(
            f,
            "  OS:        {} {} ({})",
            self.os_name, self.os_version, self.arch
        )?;
        writeln!(f, "  SIMD tier: {}", self.simd_tier)?;
        writeln!(f, "  Features:  {}", self.cpu_features.join(", "))?;
        writeln!(f, "  GPU:       not yet supported (TODO)")
    }
}

/// Cached host info (initialized once on first access).
pub fn cached_host_info() -> &'static HostInfo {
    use std::sync::OnceLock;
    static HOST: OnceLock<HostInfo> = OnceLock::new();
    HOST.get_or_init(detect_host)
}

/// Build a [`ResourceConfig`] with safe auto-detected defaults.
///
/// - **Threads** = physical cores (avoids hyper-threading contention on
///   CPU-bound compression, matching zstd / pigz behaviour).
/// - **Memory** = 25 % of total RAM, clamped to \[256 MB, 8 GB\].
/// - **GPU** = false (placeholder).
///
/// Pass the returned config through [`ResourceConfig::effective_threads`]
/// / [`ResourceConfig::effective_memory_mb`] — or override individual
/// fields from CLI flags before use.
#[must_use]
pub fn auto_resource_config() -> cpac_types::ResourceConfig {
    let host = cached_host_info();

    // Physical cores for CPU-bound work (minimum 1).
    let threads = host.physical_cores.max(1);

    // 25 % of RAM, clamped to [256 MB, 8 GB].
    let quarter_ram = (host.total_ram_mb / 4) as usize;
    let memory_mb = quarter_ram.clamp(256, 8192);

    cpac_types::ResourceConfig {
        max_threads: threads,
        max_memory_mb: memory_mb,
        gpu_enabled: false,
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Internal detection helpers
// ---------------------------------------------------------------------------

fn detect_cpu_info() -> (String, String) {
    let mut sys = sysinfo::System::new();
    sys.refresh_cpu_all();
    let cpus = sys.cpus();
    if let Some(cpu) = cpus.first() {
        (cpu.vendor_id().to_string(), cpu.brand().to_string())
    } else {
        ("unknown".into(), "unknown".into())
    }
}

fn detect_core_counts() -> (usize, usize) {
    let mut sys = sysinfo::System::new();
    sys.refresh_cpu_all();
    let logical = sys.cpus().len().max(1);
    // sysinfo 0.38: physical_core_count is an associated function
    let physical = sysinfo::System::physical_core_count()
        .unwrap_or(logical / 2)
        .max(1);
    (physical, logical)
}

fn detect_total_ram_mb() -> u64 {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.total_memory() / (1024 * 1024)
}

fn detect_cpu_features() -> Vec<String> {
    let mut features = Vec::new();

    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("sse2") {
            features.push("sse2".into());
        }
        if is_x86_feature_detected!("sse4.1") {
            features.push("sse4.1".into());
        }
        if is_x86_feature_detected!("sse4.2") {
            features.push("sse4.2".into());
        }
        if is_x86_feature_detected!("avx") {
            features.push("avx".into());
        }
        if is_x86_feature_detected!("avx2") {
            features.push("avx2".into());
        }
        if is_x86_feature_detected!("fma") {
            features.push("fma".into());
        }
        if is_x86_feature_detected!("bmi1") {
            features.push("bmi1".into());
        }
        if is_x86_feature_detected!("bmi2") {
            features.push("bmi2".into());
        }
        if is_x86_feature_detected!("popcnt") {
            features.push("popcnt".into());
        }
        if is_x86_feature_detected!("aes") {
            features.push("aes-ni".into());
        }
        // AVX-512 family — detection available on stable Rust 1.72+
        if is_x86_feature_detected!("avx512f") {
            features.push("avx512f".into());
        }
        if is_x86_feature_detected!("avx512bw") {
            features.push("avx512bw".into());
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        // NEON is mandatory on aarch64
        features.push("neon".into());
        // Additional ARM features can be detected on nightly
    }

    if features.is_empty() {
        features.push("scalar".into());
    }

    features
}

fn determine_simd_tier(features: &[String]) -> SimdTier {
    if features.contains(&"avx512f".to_string()) && features.contains(&"avx512bw".to_string()) {
        SimdTier::Avx512
    } else if features.contains(&"avx2".to_string()) {
        SimdTier::Avx2
    } else if features.contains(&"sse4.1".to_string()) {
        SimdTier::Sse41
    } else if features.contains(&"sse2".to_string()) {
        SimdTier::Sse2
    } else if features.contains(&"neon".to_string()) {
        SimdTier::Neon
    } else {
        SimdTier::Scalar
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_host_returns_valid_info() {
        let info = detect_host();
        assert!(info.logical_cores >= 1);
        assert!(info.physical_cores >= 1);
        assert!(info.total_ram_mb > 0);
        assert!(!info.os_name.is_empty());
        assert!(!info.arch.is_empty());
        assert!(!info.cpu_features.is_empty());
    }

    #[test]
    fn simd_tier_ordering() {
        assert!(SimdTier::Avx512 > SimdTier::Avx2);
        assert!(SimdTier::Avx2 > SimdTier::Sse41);
        assert!(SimdTier::Sse41 > SimdTier::Sse2);
        assert!(SimdTier::Sse2 > SimdTier::Neon);
        assert!(SimdTier::Neon > SimdTier::Scalar);
    }

    #[test]
    fn cached_host_info_consistent() {
        let a = cached_host_info();
        let b = cached_host_info();
        assert_eq!(a.logical_cores, b.logical_cores);
        assert_eq!(a.total_ram_mb, b.total_ram_mb);
    }

    #[test]
    fn display_format() {
        let info = detect_host();
        let s = format!("{info}");
        assert!(s.contains("CPAC Host Detection"));
        assert!(s.contains("Cores:"));
        assert!(s.contains("RAM:"));
    }

    #[test]
    fn auto_resource_config_sane() {
        let rc = auto_resource_config();
        // Must have at least 1 thread.
        assert!(rc.max_threads >= 1);
        // Memory must be within the clamp range.
        assert!(rc.max_memory_mb >= 256);
        assert!(rc.max_memory_mb <= 8192);
        // GPU disabled by default.
        assert!(!rc.gpu_enabled);
    }
}
