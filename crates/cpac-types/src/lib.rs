// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Shared types and error definitions for the CPAC engine.

#![allow(clippy::cast_precision_loss)]

use thiserror::Error;

/// Unified error type for all CPAC operations.
#[derive(Error, Debug)]
pub enum CpacError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid frame: {0}")]
    InvalidFrame(String),

    #[error("unsupported backend: {0}")]
    UnsupportedBackend(String),

    #[error("decompression failed: {0}")]
    DecompressFailed(String),

    #[error("compression failed: {0}")]
    CompressFailed(String),

    #[error("transform error: {0}")]
    Transform(String),

    #[error("encryption error: {0}")]
    Encryption(String),

    #[error("I/O error: {0}")]
    IoError(String),

    #[error("{0}")]
    Other(String),

    /// Compressor or decompressor used after it was already finalized.
    #[error("already finalized")]
    AlreadyFinalized,

    /// Domain-specific detection or extraction failure.
    #[error("domain error ({domain}): {message}")]
    DomainError {
        domain: &'static str,
        message: String,
    },
}

/// Result type alias for CPAC operations.
pub type CpacResult<T> = Result<T, CpacError>;

// ---------------------------------------------------------------------------
// Data types flowing through the DAG
// ---------------------------------------------------------------------------

/// Precision for floating-point columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloatPrecision {
    F32,
    F64,
}

/// The fundamental data types flowing through the compression DAG.
#[derive(Clone, Debug)]
pub enum CpacType {
    /// Raw byte stream (opaque).
    Serial(Vec<u8>),

    /// Fixed-width record table (columns × rows).
    Struct {
        columns: Vec<Vec<u8>>,
        row_count: usize,
        record_width: usize,
    },

    /// Typed integer array.
    IntColumn {
        values: Vec<i64>,
        original_width: u8,
    },

    /// Float array.
    FloatColumn {
        values: Vec<f64>,
        precision: FloatPrecision,
    },

    /// String column (variable-length values).
    StringColumn {
        values: Vec<String>,
        total_bytes: usize,
    },

    /// Multiple typed columns (after format parsing).
    ColumnSet { columns: Vec<(String, CpacType)> },
}

/// Tag identifying the variant of [`CpacType`] without carrying data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TypeTag {
    Serial,
    Struct,
    IntColumn,
    FloatColumn,
    StringColumn,
    ColumnSet,
}

impl CpacType {
    /// Get the type tag for this value.
    #[must_use]
    pub fn tag(&self) -> TypeTag {
        match self {
            CpacType::Serial(_) => TypeTag::Serial,
            CpacType::Struct { .. } => TypeTag::Struct,
            CpacType::IntColumn { .. } => TypeTag::IntColumn,
            CpacType::FloatColumn { .. } => TypeTag::FloatColumn,
            CpacType::StringColumn { .. } => TypeTag::StringColumn,
            CpacType::ColumnSet { .. } => TypeTag::ColumnSet,
        }
    }
}

// ---------------------------------------------------------------------------
// Compression level
// ---------------------------------------------------------------------------

/// Compression quality preset.
///
/// Controls the trade-off between compression ratio and speed.
/// `Default` preserves the historical CPAC behaviour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CompressionLevel {
    /// Fast: optimise for throughput (brotli-6 / zstd-1).
    Fast,
    /// Default: brotli-11 / zstd-3 — matches industry baselines for fair comparison.
    #[default]
    Default,
    /// High: brotli-11 / zstd-12 — batch jobs with strong ratio/speed balance.
    High,
    /// Best: brotli-11 / zstd-19 — cold storage, maximum compression.
    Best,
}

// ---------------------------------------------------------------------------
// Named presets
// ---------------------------------------------------------------------------

/// Named compression preset — auto-configures level, transforms, block size,
/// threading, and MSN in one shot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    /// Real-time ingest: zstd-1, 1 thread, no transforms, no MSN, 1 MB blocks.
    Turbo,
    /// General purpose (DEFAULT): zstd-3, auto threads, smart transforms ON,
    /// MSN OFF, 4 MB blocks.
    Balanced,
    /// Batch jobs: zstd-9, all threads, smart + MSN ON, 8 MB blocks.
    Maximum,
    /// Cold storage: zstd-19, all threads, aggressive transforms + MSN, 16 MB blocks.
    Archive,
}

impl Preset {
    /// Parse from string (case-insensitive).
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "turbo" | "t" => Some(Self::Turbo),
            "balanced" | "b" | "default" => Some(Self::Balanced),
            "maximum" | "max" | "m" => Some(Self::Maximum),
            "archive" | "arc" | "a" => Some(Self::Archive),
            _ => None,
        }
    }

    /// Compression level for this preset.
    #[must_use]
    pub fn level(self) -> CompressionLevel {
        match self {
            Self::Turbo => CompressionLevel::Fast,
            Self::Balanced => CompressionLevel::Default,
            Self::Maximum => CompressionLevel::High,
            Self::Archive => CompressionLevel::Best,
        }
    }

    /// Whether smart transforms are enabled.
    #[must_use]
    pub fn smart_transforms(self) -> bool {
        !matches!(self, Self::Turbo)
    }

    /// Whether MSN is enabled.
    #[must_use]
    pub fn msn_enabled(self) -> bool {
        matches!(self, Self::Maximum | Self::Archive)
    }

    /// MSN confidence threshold.
    #[must_use]
    pub fn msn_confidence(self) -> f64 {
        match self {
            Self::Archive => 0.3,
            _ => 0.5,
        }
    }

    /// Block size in bytes for parallel compression.
    #[must_use]
    pub fn block_size(self) -> usize {
        match self {
            Self::Turbo => 1 << 20,    // 1 MB
            Self::Balanced => 4 << 20, // 4 MB
            Self::Maximum => 8 << 20,  // 8 MB
            Self::Archive => 16 << 20, // 16 MB
        }
    }
}

impl std::fmt::Display for Preset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Turbo => write!(f, "turbo"),
            Self::Balanced => write!(f, "balanced"),
            Self::Maximum => write!(f, "maximum"),
            Self::Archive => write!(f, "archive"),
        }
    }
}

// ---------------------------------------------------------------------------
// Process priority
// ---------------------------------------------------------------------------

/// Thread/process priority hint for datacenter workloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Priority {
    /// Lower priority — yield to other workloads.
    Low,
    /// Normal (default).
    #[default]
    Normal,
    /// Higher priority — latency-sensitive.
    High,
}

impl Priority {
    /// Parse from string (case-insensitive).
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "low" | "l" => Some(Self::Low),
            "normal" | "n" => Some(Self::Normal),
            "high" | "h" => Some(Self::High),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Hardware accelerator backend
// ---------------------------------------------------------------------------

/// Hardware acceleration backend for entropy coding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AccelBackend {
    /// Software-only (current default path).
    Software,
    /// Intel QuickAssist Technology — hardware zstd/deflate.
    IntelQat,
    /// Intel In-Memory Analytics Accelerator (Sapphire Rapids+).
    IntelIaa,
    /// AMD Xilinx Alveo FPGA acceleration.
    AmdXilinx,
    /// GPU compute (CUDA/Vulkan).
    GpuCompute,
    /// ARM SVE2 wide-vector acceleration.
    ArmSve2,
}

impl AccelBackend {
    /// Parse from string (case-insensitive).
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "software" | "sw" | "cpu" => Some(Self::Software),
            "qat" | "intel-qat" => Some(Self::IntelQat),
            "iaa" | "intel-iaa" => Some(Self::IntelIaa),
            "xilinx" | "amd-xilinx" | "fpga" => Some(Self::AmdXilinx),
            "gpu" | "cuda" | "vulkan" => Some(Self::GpuCompute),
            "sve2" | "arm-sve2" => Some(Self::ArmSve2),
            "auto" => None, // None = auto-detect best available
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Entropy backend identifier
// ---------------------------------------------------------------------------

/// Entropy coding backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Backend {
    /// No compression (raw passthrough).
    Raw = 0,
    /// Zstandard compression.
    Zstd = 1,
    /// Brotli compression.
    Brotli = 2,
    /// Gzip/Deflate compression (RFC 1952).
    Gzip = 3,
    /// LZMA compression (7z/xz).
    Lzma = 4,
}

impl Backend {
    /// Decode backend from its wire ID.
    ///
    /// # Errors
    ///
    /// Returns [`CpacError::UnsupportedBackend`] if the ID is not in the range 0-4.
    pub fn from_id(id: u8) -> CpacResult<Self> {
        match id {
            0 => Ok(Backend::Raw),
            1 => Ok(Backend::Zstd),
            2 => Ok(Backend::Brotli),
            3 => Ok(Backend::Gzip),
            4 => Ok(Backend::Lzma),
            _ => Err(CpacError::UnsupportedBackend(format!(
                "unknown backend id: {id}"
            ))),
        }
    }

    /// Wire ID for this backend.
    #[must_use]
    pub fn id(self) -> u8 {
        self as u8
    }
}

// ---------------------------------------------------------------------------
// SSR Track
// ---------------------------------------------------------------------------

/// Compression track selected by SSR analysis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Track {
    /// Track 1: domain-aware (MSN + CPSC projection).
    Track1,
    /// Track 2: generic entropy coding.
    Track2,
}

// ---------------------------------------------------------------------------
// Domain hints
// ---------------------------------------------------------------------------

/// Hint about the data's domain format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DomainHint {
    Csv,
    Json,
    Xml,
    Yaml,
    Log,
    Binary,
    Unknown,
}

// ---------------------------------------------------------------------------
// Compression config + results
// ---------------------------------------------------------------------------

/// Resource limits for compression/decompression.
///
/// Safe defaults (all zeros = auto):
/// - **Threads**: physical CPU cores (not hyper-threaded logical cores),
///   matching the behaviour of zstd, pigz, and similar tools for
///   CPU-bound workloads. Falls back to `available_parallelism()` when
///   physical core count is unavailable.
/// - **Memory**: 25 % of total system RAM, clamped to \[256 MB, 8 GB\].
///   This keeps the compressor from starving the rest of the system
///   while still giving plenty of headroom for large inputs.
/// - **GPU**: disabled (placeholder for future CUDA/Vulkan offload).
#[derive(Clone, Debug, Default)]
pub struct ResourceConfig {
    /// Maximum worker threads (0 = auto-detect: physical cores).
    pub max_threads: usize,
    /// Maximum memory budget in MB (0 = auto-detect: 25 % of RAM,
    /// clamped to 256 MB .. 8 GB).
    pub max_memory_mb: usize,
    /// Whether GPU acceleration is enabled (always false for now — TODO).
    pub gpu_enabled: bool,

    // --- Datacenter resource knobs ---
    /// Cap CPU usage as a percentage of physical cores (1-100).
    /// `effective_threads = (physical_cores * pct / 100).max(1)`.
    /// `None` = no cap (use `max_threads` or auto).
    pub max_cpu_percent: Option<u8>,
    /// Memory budget as a percentage of system RAM (1-100).
    /// Overrides `max_memory_mb` when set.
    pub max_memory_percent: Option<u8>,
    /// Per-file time budget in milliseconds.  If compression exceeds
    /// this, remaining parallel blocks bail out to `CompressionLevel::Fast`.
    /// `None` = no time limit.
    pub budget_ms: Option<u64>,
    /// Thread/process scheduling priority hint.
    pub priority: Option<Priority>,
    /// I/O bandwidth cap in MB/s (applied in CLI read path).
    pub io_bandwidth_mbps: Option<u32>,
    /// Maximum files compressed simultaneously in `--recursive` mode.
    pub batch_concurrency: Option<usize>,
}

impl ResourceConfig {
    /// Effective thread count.
    ///
    /// When `max_threads == 0` (the default) this returns
    /// `available_parallelism` (logical cores) as a portable fallback.
    /// Callers that have access to host detection should prefer
    /// [`cpac_engine::auto_resource_config`] which uses *physical* cores.
    #[must_use]
    pub fn effective_threads(&self) -> usize {
        if self.max_threads == 0 {
            std::thread::available_parallelism()
                .map(std::num::NonZero::get)
                .unwrap_or(1)
        } else {
            self.max_threads
        }
    }

    /// Effective memory budget in MB.
    ///
    /// When `max_memory_mb == 0` returns a conservative 1024 MB
    /// (callers with host detection should use
    /// [`cpac_engine::auto_resource_config`] for a smarter default).
    #[must_use]
    pub fn effective_memory_mb(&self) -> usize {
        if self.max_memory_mb == 0 {
            1024 // 1 GB fallback when no host info available
        } else {
            self.max_memory_mb
        }
    }
}

/// Configuration for compression.
#[derive(Clone, Debug)]
pub struct CompressConfig {
    /// Force a specific backend (None = auto-select via SSR).
    pub backend: Option<Backend>,
    /// Force a specific track (None = auto via SSR).
    pub force_track: Option<Track>,
    /// Filename hint for domain detection.
    pub filename: Option<String>,
    /// Resource limits (threads, memory, GPU).
    pub resources: Option<ResourceConfig>,
    /// Optional pre-trained dictionary (raw zstd dict format).
    pub dictionary: Option<Vec<u8>>,
    /// Enable Multi-Scale Normalization (MSN) for domain-specific semantic extraction.
    /// When enabled, extracts repeated structure from JSON, CSV, XML, logs, etc.
    /// Default: false.
    pub enable_msn: bool,
    /// MSN minimum confidence threshold for auto-detection (0.0-1.0).
    /// Higher values require more certainty before applying MSN.
    /// Default: 0.5.
    pub msn_confidence: f64,
    /// Force a specific MSN domain (overrides auto-detection).
    /// Format: "category.type" (e.g., "text.json", "log.apache").
    /// None = auto-detect based on content.
    pub msn_domain: Option<String>,
    /// Compression quality preset.
    /// Controls brotli quality and zstd level. `Default` preserves previous behaviour.
    pub level: CompressionLevel,
    /// If true, print MSN decision trace per block to stderr.
    /// Enabled via `-vvv` in the CLI or `CPAC_MSN_VERBOSE=1` env var.
    /// Default: false.
    pub msn_verbose: bool,
    /// Enable data-driven smart transform selection.
    /// When enabled, the analyzer recommends transforms based on SSR/MSN analysis
    /// and empirical calibration from corpus benchmarks. The selected transforms
    /// are serialized into the frame header as a DAG descriptor.
    /// Default: true.
    pub enable_smart_transforms: bool,
    /// Internal: disable parallel compression (prevents recursive parallel calls).
    #[doc(hidden)]
    pub disable_parallel: bool,

    // --- Phase 1+2+4 additions ---
    /// Named preset that was used to build this config (for logging/introspection).
    pub preset: Option<Preset>,
    /// Block size override for parallel compression (0 = use preset default).
    pub block_size: usize,
    /// Hardware accelerator preference (None = auto-detect best available).
    pub accelerator: Option<AccelBackend>,
}

impl Default for CompressConfig {
    fn default() -> Self {
        Self {
            backend: None,
            force_track: None,
            filename: None,
            resources: None,
            dictionary: None,
            enable_msn: false,
            msn_confidence: 0.5,
            msn_domain: None,
            level: CompressionLevel::Default,
            msn_verbose: false,
            enable_smart_transforms: true,
            disable_parallel: false,
            preset: None,
            block_size: 0,
            accelerator: None,
        }
    }
}

impl CompressConfig {
    /// Build a config from a named preset.
    ///
    /// Individual fields can be overridden after construction.
    #[must_use]
    pub fn from_preset(preset: Preset) -> Self {
        Self {
            level: preset.level(),
            enable_smart_transforms: preset.smart_transforms(),
            enable_msn: preset.msn_enabled(),
            msn_confidence: preset.msn_confidence(),
            block_size: preset.block_size(),
            preset: Some(preset),
            ..Self::default()
        }
    }
}

/// Result of compression.
#[derive(Clone, Debug)]
pub struct CompressResult {
    /// Compressed frame bytes.
    pub data: Vec<u8>,
    /// Original (uncompressed) size.
    pub original_size: usize,
    /// Compressed size.
    pub compressed_size: usize,
    /// Track used.
    pub track: Track,
    /// Backend used.
    pub backend: Backend,
}

impl CompressResult {
    /// Compression ratio (original / compressed).
    #[must_use]
    pub fn ratio(&self) -> f64 {
        if self.compressed_size == 0 {
            return 0.0;
        }
        self.original_size as f64 / self.compressed_size as f64
    }
}

/// Result of decompression.
#[derive(Clone, Debug)]
pub struct DecompressResult {
    /// Decompressed bytes.
    pub data: Vec<u8>,
    /// Whether decompression succeeded.
    pub success: bool,
    /// Error message if decompression failed.
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_roundtrip() {
        for b in [Backend::Raw, Backend::Zstd, Backend::Brotli] {
            assert_eq!(Backend::from_id(b.id()).unwrap(), b);
        }
    }

    #[test]
    fn backend_invalid_id() {
        assert!(Backend::from_id(99).is_err());
    }

    #[test]
    fn cpac_type_tag() {
        let s = CpacType::Serial(vec![1, 2, 3]);
        assert_eq!(s.tag(), TypeTag::Serial);
    }
}
