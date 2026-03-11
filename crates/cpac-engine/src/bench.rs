// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Benchmarking framework: `BenchmarkRunner`, `CorpusManager`, report generation.

use cpac_types::{Backend, CompressConfig, CompressionLevel, CpacError, CpacResult, Track};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// Result of benchmarking a single file/engine combination.
#[derive(Clone, Debug)]
pub struct BenchResult {
    pub file: PathBuf,
    /// Label: CPAC backend name or standalone codec label.
    pub engine_label: String,
    pub original_size: usize,
    pub compressed_size: usize,
    pub ratio: f64,
    pub compress_time: Duration,
    pub decompress_time: Duration,
    pub compress_throughput_mbs: f64,
    pub decompress_throughput_mbs: f64,
    /// Peak memory in bytes (measured via process RSS delta, with
    /// estimate fallback when sysinfo returns 0).
    pub peak_memory_bytes: usize,
    /// Whether a full compress→decompress lossless roundtrip was verified.
    pub lossless_verified: bool,
    /// SSR analysis time (separate from compress pipeline).
    pub ssr_time: Option<Duration>,
    /// MSN extraction time (None for standalone/non-Track1 runs).
    pub msn_time: Option<Duration>,
    /// Which track produced this result (None for standalone benchmarks).
    pub track: Option<Track>,
    /// The CPAC compression level used for this benchmark run.
    pub compression_level: CompressionLevel,
}

/// A standalone codec at a specific CPAC level for baseline comparison.
///
/// Measures raw compressor performance without CPAC pipeline overhead
/// (no SSR, MSN, transforms, or framing).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StandaloneCodec {
    /// Which backend to use.
    pub backend: Backend,
    /// CPAC compression level (mapped to the backend's native level internally).
    pub cpac_level: CompressionLevel,
}

/// Map a (backend, CPAC level) pair to the raw compressor level for display.
///
/// Returns `None` only for `Backend::Raw` which is not a standalone codec.
#[must_use]
pub fn standalone_raw_level(backend: Backend, level: CompressionLevel) -> Option<i32> {
    use Backend::*;
    use CompressionLevel::*;
    match backend {
        Raw => None,
        Zstd => Some(match level {
            UltraFast => 1,
            Fast => 3,
            Default => 6,
            High => 12,
            Best => 19,
        }),
        Brotli => Some(match level {
            UltraFast => 1,
            Fast => 4,
            Default => 6,
            High => 9,
            Best => 11,
        }),
        Gzip => Some(match level {
            UltraFast => 1,
            Fast => 3,
            Default => 6,
            High => 8,
            Best => 9,
        }),
        Lzma => Some(match level {
            UltraFast => 0,
            Fast => 2,
            Default => 6,
            High => 8,
            Best => 9,
        }),
        Xz => Some(match level {
            UltraFast => 0,
            Fast => 2,
            Default => 6,
            High => 8,
            Best => 9,
        }),
        Lz4 => Some(match level {
            // Display levels (lz4 CLI convention): 1-3 = fast, 4-12 = HC
            UltraFast => 1,
            Fast => 3,
            Default => 4, // HC
            High => 9,    // HC
            Best => 12,   // HC
        }),
        Snappy => Some(0), // single speed, no levels
        Lzham => Some(match level {
            // LZHAM has 5 levels (0-4): FASTEST, FASTER, DEFAULT, BETTER, UBER
            UltraFast => 0,
            Fast => 1,
            Default => 2,
            High => 3,
            Best => 4,
        }),
        Lizard => Some(match level {
            UltraFast => 10,
            Fast => 13,
            Default => 26,
            High => 32,
            Best => 49,
        }),
        ZlibNg => Some(match level {
            UltraFast => 1,
            Fast => 3,
            Default => 6,
            High => 8,
            Best => 9,
        }),
        OpenZl => Some(match level {
            UltraFast => 1,
            Fast => 3,
            Default => 6,
            High => 12,
            Best => 19,
        }),
    }
}

impl StandaloneCodec {
    /// Short name for the backend (lowercase, CLI-friendly).
    #[must_use]
    pub fn backend_name(&self) -> &'static str {
        match self.backend {
            Backend::Zstd => "zstd",
            Backend::Brotli => "brotli",
            Backend::Gzip => "gzip",
            Backend::Lzma => "lzma",
            Backend::Xz => "xz",
            Backend::Lz4 => "lz4",
            Backend::Snappy => "snappy",
            Backend::Lzham => "lzham",
            Backend::Lizard => "lizard",
            Backend::ZlibNg => "zlib-ng",
            Backend::OpenZl => "openzl",
            Backend::Raw => "raw",
        }
    }

    /// Human-readable label, e.g. `"zstd-6"`, `"snappy"`, `"brotli-11"`.
    #[must_use]
    pub fn label(&self) -> String {
        match standalone_raw_level(self.backend, self.cpac_level) {
            Some(_) if self.backend == Backend::Snappy => "snappy".to_string(),
            Some(l) => format!("{}-{l}", self.backend_name()),
            None => self.backend_name().to_string(),
        }
    }

    /// Compress data using the standalone codec (no CPAC framing/transforms).
    pub fn compress(&self, data: &[u8]) -> CpacResult<Vec<u8>> {
        cpac_entropy::compress_at_level(data, self.backend, self.cpac_level, None)
    }

    /// Decompress data produced by [`compress`](Self::compress).
    pub fn decompress(&self, data: &[u8]) -> CpacResult<Vec<u8>> {
        cpac_entropy::decompress(data, self.backend)
    }

    /// Parse a standalone codec from a label string (e.g. `"zstd-6"`, `"snappy"`).
    ///
    /// Reverse-maps the raw level to find the matching CPAC level from the table.
    pub fn from_label(s: &str) -> Option<StandaloneCodec> {
        let s_lower = s.to_ascii_lowercase();
        // Try "codec-rawlevel" format (rsplit_once handles "zlib-ng-6" correctly)
        if let Some((codec_str, level_str)) = s_lower.rsplit_once('-') {
            if let Ok(raw_level) = level_str.parse::<i32>() {
                let backend = Self::parse_backend_name(codec_str)?;
                // Find which CPAC level produces this raw level
                for &cpac_level in &[
                    CompressionLevel::UltraFast,
                    CompressionLevel::Fast,
                    CompressionLevel::Default,
                    CompressionLevel::High,
                    CompressionLevel::Best,
                ] {
                    if standalone_raw_level(backend, cpac_level) == Some(raw_level) {
                        return Some(StandaloneCodec {
                            backend,
                            cpac_level,
                        });
                    }
                }
            }
        }
        // Plain name without level (e.g. "snappy", "zlib-ng")
        let backend = Self::parse_backend_name(&s_lower)?;
        Some(StandaloneCodec {
            backend,
            cpac_level: CompressionLevel::Default,
        })
    }

    fn parse_backend_name(s: &str) -> Option<Backend> {
        match s {
            "zstd" | "zstandard" => Some(Backend::Zstd),
            "brotli" => Some(Backend::Brotli),
            "gzip" | "gz" => Some(Backend::Gzip),
            "lzma" => Some(Backend::Lzma),
            "xz" => Some(Backend::Xz),
            "lz4" => Some(Backend::Lz4),
            "snappy" => Some(Backend::Snappy),
            "lzham" => Some(Backend::Lzham),
            "lizard" => Some(Backend::Lizard),
            "zlib-ng" | "zlibng" | "zlib_ng" => Some(Backend::ZlibNg),
            "openzl" => Some(Backend::OpenZl),
            _ => None,
        }
    }
}

/// Return standalone codecs matching a given CPAC [`CompressionLevel`].
///
/// Returns one `StandaloneCodec` per backend (excluding `Raw`) at the
/// table-matched level. All backends now have valid mappings at all levels.
#[must_use]
pub fn matched_baselines(level: CompressionLevel) -> Vec<StandaloneCodec> {
    [
        Backend::Zstd,
        Backend::Brotli,
        Backend::Gzip,
        Backend::Lzma,
        Backend::Xz,
        Backend::Lz4,
        Backend::Snappy,
        Backend::Lzham,
        Backend::Lizard,
        Backend::ZlibNg,
        Backend::OpenZl,
    ]
    .iter()
    .filter_map(|&backend| {
        standalone_raw_level(backend, level).map(|_| StandaloneCodec {
            backend,
            cpac_level: level,
        })
    })
    .collect()
}

/// Parse a [`CompressionLevel`] from a string.
pub fn parse_compression_level(s: &str) -> Option<CompressionLevel> {
    match s.to_ascii_lowercase().as_str() {
        "ultrafast" | "uf" | "0" => Some(CompressionLevel::UltraFast),
        "fast" | "f" | "1" => Some(CompressionLevel::Fast),
        "default" | "d" | "2" => Some(CompressionLevel::Default),
        "high" | "h" | "3" => Some(CompressionLevel::High),
        "best" | "b" | "max" | "4" => Some(CompressionLevel::Best),
        _ => None,
    }
}

/// Aggregate summary over a corpus run.
#[derive(Clone, Debug)]
pub struct CorpusSummary {
    pub corpus_name: String,
    pub results: Vec<BenchResult>,
    pub total_original: usize,
    pub total_compressed: usize,
    pub overall_ratio: f64,
    pub mean_compress_mbs: f64,
    pub mean_decompress_mbs: f64,
    /// Total peak memory across all runs.
    pub total_peak_memory_bytes: usize,
    /// All results verified lossless.
    pub all_lossless: bool,
}

/// Benchmark profile controlling iteration count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchProfile {
    Quick,    // 1 iteration
    Balanced, // 3 iterations
    Full,     // 10 iterations
}

impl BenchProfile {
    #[must_use]
    pub fn iterations(self) -> usize {
        match self {
            BenchProfile::Quick => 1,
            BenchProfile::Balanced => 3,
            BenchProfile::Full => 10,
        }
    }
}

// ---------------------------------------------------------------------------
// Corpus manager
// ---------------------------------------------------------------------------

/// Discovers files in a directory for benchmarking.
pub struct CorpusManager;

impl CorpusManager {
    /// Scan a directory recursively, returning files up to `max_size_mb`.
    #[must_use]
    pub fn scan_directory(dir: &Path, max_size_mb: Option<u64>) -> Vec<PathBuf> {
        let max_bytes = max_size_mb.map(|mb| mb * 1024 * 1024);
        let mut files = Vec::new();
        Self::scan_recursive(dir, &mut files, max_bytes);
        files.sort();
        files
    }

    fn scan_recursive(dir: &Path, out: &mut Vec<PathBuf>, max_bytes: Option<u64>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                Self::scan_recursive(&path, out, max_bytes);
            } else if path.is_file() {
                if let Some(max) = max_bytes {
                    if let Ok(meta) = path.metadata() {
                        if meta.len() > max {
                            continue;
                        }
                    }
                }
                // Skip already-compressed CPAC files.
                if path.extension().is_some_and(|e| e == "cpac") {
                    continue;
                }
                out.push(path);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// BenchmarkRunner
// ---------------------------------------------------------------------------

/// Run benchmarks across files and backends.
pub struct BenchmarkRunner {
    pub profile: BenchProfile,
    pub backends: Vec<Backend>,
    /// CPAC level used for baseline comparisons in [`bench_directory`](Self::bench_directory).
    pub bench_level: CompressionLevel,
    pub skip_baselines: bool,
}

impl Default for BenchmarkRunner {
    fn default() -> Self {
        Self {
            profile: BenchProfile::Balanced,
            backends: vec![
                Backend::Zstd,
                Backend::Brotli,
                Backend::Gzip,
                Backend::Lzma,
                Backend::Xz,
                Backend::Lz4,
                Backend::Snappy,
                Backend::Lzham,
                Backend::Lizard,
                Backend::ZlibNg,
                Backend::OpenZl,
                Backend::Raw,
            ],
            bench_level: CompressionLevel::Default,
            skip_baselines: false,
        }
    }
}

impl BenchmarkRunner {
    #[must_use]
    pub fn new(profile: BenchProfile) -> Self {
        Self {
            profile,
            ..Default::default()
        }
    }

    /// Benchmark a single file with a single CPAC backend.
    ///
    /// Averages over `profile.iterations`, verifies lossless roundtrip,
    /// and estimates peak memory.
    pub fn bench_file(&self, path: &Path, backend: Backend) -> CpacResult<BenchResult> {
        self.bench_file_with_level(path, backend, Some(self.bench_level))
    }

    /// Benchmark a single file with a specific CPAC backend and optional
    /// compression level override.
    pub fn bench_file_with_level(
        &self,
        path: &Path,
        backend: Backend,
        level: Option<cpac_types::CompressionLevel>,
    ) -> CpacResult<BenchResult> {
        let data = std::fs::read(path)
            .map_err(|e| cpac_types::CpacError::IoError(format!("{}: {e}", path.display())))?;
        let original_size = data.len();
        let iterations = self.profile.iterations();
        let effective_level = level.unwrap_or(self.bench_level);
        let config = CompressConfig {
            backend: Some(backend),
            level: effective_level,
            // Pass filename so smart transforms get the same extension hint
            // as the auto-routing path (bench_file_auto).  Without this,
            // the analyzer may recommend different transforms, leading to
            // lossless roundtrip failures on certain data patterns.
            filename: Some(path.to_string_lossy().into_owned()),
            ..Default::default()
        };

        // Measure SSR analysis time separately
        let ssr_start = Instant::now();
        let _ = cpac_ssr::analyze(&data);
        let ssr_time = ssr_start.elapsed();

        let rss_before = measure_rss_bytes();

        // Warmup iteration (cache/allocator warm-up)
        let warmup_result = crate::compress(&data, &config)?;
        let selected_track = warmup_result.track;

        let mut total_compress = Duration::ZERO;
        let mut compressed_data = Vec::new();
        for _ in 0..iterations {
            let start = Instant::now();
            let result = crate::compress(&data, &config)?;
            total_compress += start.elapsed();
            compressed_data = result.data;
        }
        let avg_compress = total_compress / iterations as u32;
        let compressed_size = compressed_data.len();

        // Lossless verification
        let decompressed = crate::decompress(&compressed_data)?;
        let lossless_verified = decompressed.data == data;

        // Warmup decompress
        let _ = crate::decompress(&compressed_data)?;

        // Decompress timing
        let mut total_decompress = Duration::ZERO;
        for _ in 0..iterations {
            let start = Instant::now();
            let _result = crate::decompress(&compressed_data)?;
            total_decompress += start.elapsed();
        }
        let avg_decompress = total_decompress / iterations as u32;

        let ratio = calc_ratio(original_size, compressed_size);
        let compress_mbs = calc_throughput(original_size, &avg_compress);
        let decompress_mbs = calc_throughput(original_size, &avg_decompress);
        let peak_memory_bytes =
            peak_memory_or_fallback(rss_before, original_size + compressed_size + original_size);

        // Build label with raw backend level for verifiable fairness.
        let raw_level = standalone_raw_level(backend, effective_level);
        let engine_label = match raw_level {
            Some(_) if backend == Backend::Snappy => format!("{backend:?}"),
            Some(l) => format!("{backend:?}@{l}"),
            None => format!("{backend:?}"),
        };

        Ok(BenchResult {
            file: path.to_path_buf(),
            engine_label,
            original_size,
            compressed_size,
            ratio,
            compress_time: avg_compress,
            decompress_time: avg_decompress,
            compress_throughput_mbs: compress_mbs,
            decompress_throughput_mbs: decompress_mbs,
            peak_memory_bytes,
            lossless_verified,
            ssr_time: Some(ssr_time),
            msn_time: None, // MSN timing requires pipeline instrumentation
            track: Some(selected_track),
            compression_level: effective_level,
        })
    }

    /// Benchmark a single file with a specific CPAC backend **and MSN enabled**.
    ///
    /// Like [`bench_file_with_level`](Self::bench_file_with_level) but forces
    /// `enable_msn = true` so MSN domain preprocessing is applied before the
    /// chosen backend compresses the data.  Used to measure the ratio uplift
    /// MSN gives each backend independently.
    pub fn bench_file_msn(
        &self,
        path: &Path,
        backend: Backend,
        level: CompressionLevel,
    ) -> CpacResult<BenchResult> {
        let data = std::fs::read(path)
            .map_err(|e| CpacError::IoError(format!("{}: {e}", path.display())))?;
        let original_size = data.len();
        let iterations = self.profile.iterations();
        let config = CompressConfig {
            backend: Some(backend),
            enable_msn: true,
            msn_confidence: 0.5,
            level,
            filename: Some(path.to_string_lossy().into_owned()),
            ..Default::default()
        };

        let rss_before = measure_rss_bytes();

        // Warmup
        let _ = crate::compress(&data, &config)?;

        let mut total_compress = Duration::ZERO;
        let mut compressed_data = Vec::new();
        let mut selected_track = Track::Track2;
        for _ in 0..iterations {
            let start = Instant::now();
            let result = crate::compress(&data, &config)?;
            total_compress += start.elapsed();
            selected_track = result.track;
            compressed_data = result.data;
        }
        let avg_compress = total_compress / iterations as u32;
        let compressed_size = compressed_data.len();

        // Lossless verification
        let decompressed = crate::decompress(&compressed_data)?;
        let lossless_verified = decompressed.data == data;

        // Warmup decompress
        let _ = crate::decompress(&compressed_data)?;

        let mut total_decompress = Duration::ZERO;
        for _ in 0..iterations {
            let start = Instant::now();
            let _r = crate::decompress(&compressed_data)?;
            total_decompress += start.elapsed();
        }
        let avg_decompress = total_decompress / iterations as u32;

        let ratio = calc_ratio(original_size, compressed_size);
        let raw_lvl = standalone_raw_level(backend, level)
            .map(|l| format!("@{l}"))
            .unwrap_or_default();
        let track_str = match selected_track {
            Track::Track1 => "T1",
            Track::Track2 => "T2",
        };
        let engine_label = format!("{track_str}(MSN/{backend:?}{raw_lvl})");

        Ok(BenchResult {
            file: path.to_path_buf(),
            engine_label,
            original_size,
            compressed_size,
            ratio,
            compress_time: avg_compress,
            decompress_time: avg_decompress,
            compress_throughput_mbs: calc_throughput(original_size, &avg_compress),
            decompress_throughput_mbs: calc_throughput(original_size, &avg_decompress),
            peak_memory_bytes: peak_memory_or_fallback(
                rss_before,
                original_size + compressed_size + original_size,
            ),
            lossless_verified,
            ssr_time: None,
            msn_time: None,
            track: Some(selected_track),
            compression_level: level,
        })
    }

    /// Benchmark a file using SSR auto-routing, optionally with MSN (Track 1).
    ///
    /// Unlike [`bench_file`](Self::bench_file) which forces a specific backend,
    /// this lets SSR select the backend automatically — matching the production
    /// `cpac compress` path. The result label reflects the actual track + backend
    /// chosen by SSR, e.g. `"T1(SSR/Brotli)"` or `"T1(MSN/Zstd)"`.
    pub fn bench_file_auto(
        &self,
        path: &Path,
        enable_msn: bool,
        level: CompressionLevel,
    ) -> CpacResult<BenchResult> {
        let data = std::fs::read(path)
            .map_err(|e| CpacError::IoError(format!("{}: {e}", path.display())))?;
        let original_size = data.len();
        let iterations = self.profile.iterations();
        let config = CompressConfig {
            backend: None, // SSR auto-selects
            enable_msn,
            msn_confidence: 0.5,
            level,
            // Pass filename so MSN can use extension-based domain detection
            // (e.g. .log → syslog confidence bump, .csv → CSV domain, .jsonl → JSONL).
            filename: Some(path.to_string_lossy().into_owned()),
            ..Default::default()
        };

        // Measure SSR analysis time
        let ssr_start = Instant::now();
        let _ = cpac_ssr::analyze(&data);
        let ssr_time = ssr_start.elapsed();

        let rss_before = measure_rss_bytes();

        // Warmup iteration
        let _ = crate::compress(&data, &config)?;

        let mut total_compress = Duration::ZERO;
        let mut compressed_data = Vec::new();
        let mut selected_backend = Backend::Zstd;
        let mut selected_track = Track::Track2;

        for _ in 0..iterations {
            let start = Instant::now();
            let result = crate::compress(&data, &config)?;
            total_compress += start.elapsed();
            selected_backend = result.backend;
            selected_track = result.track;
            compressed_data = result.data;
        }
        let avg_compress = total_compress / iterations as u32;
        let compressed_size = compressed_data.len();

        // Lossless verification
        let decompressed = crate::decompress(&compressed_data)?;
        let lossless_verified = decompressed.data == data;

        // Warmup decompress
        let _ = crate::decompress(&compressed_data)?;

        // Decompress timing
        let mut total_decompress = Duration::ZERO;
        for _ in 0..iterations {
            let start = Instant::now();
            let _r = crate::decompress(&compressed_data)?;
            total_decompress += start.elapsed();
        }
        let avg_decompress = total_decompress / iterations as u32;

        let track_str = match selected_track {
            Track::Track1 => "T1",
            Track::Track2 => "T2",
        };
        let mode_str = if enable_msn { "MSN" } else { "SSR" };
        let raw_lvl = standalone_raw_level(selected_backend, level)
            .map(|l| format!("@{l}"))
            .unwrap_or_default();
        let engine_label = format!("{track_str}({mode_str}/{selected_backend:?}{raw_lvl})");

        Ok(BenchResult {
            file: path.to_path_buf(),
            engine_label,
            original_size,
            compressed_size,
            ratio: calc_ratio(original_size, compressed_size),
            compress_time: avg_compress,
            decompress_time: avg_decompress,
            compress_throughput_mbs: calc_throughput(original_size, &avg_compress),
            decompress_throughput_mbs: calc_throughput(original_size, &avg_decompress),
            peak_memory_bytes: peak_memory_or_fallback(
                rss_before,
                original_size + compressed_size + original_size,
            ),
            lossless_verified,
            ssr_time: Some(ssr_time),
            msn_time: None,
            track: Some(selected_track),
            compression_level: level,
        })
    }

    /// Benchmark a file with a forced track override (discovery/research mode).
    ///
    /// `force_track = Some(Track::Track1)` — MSN applied to ALL blocks regardless of SSR.
    /// `force_track = Some(Track::Track2)` — MSN never applied (same as enable_msn=false).
    /// `force_track = None` — normal SSR auto-routing.
    pub fn bench_file_forced_track(
        &self,
        path: &Path,
        force_track: Option<Track>,
        level: CompressionLevel,
    ) -> CpacResult<BenchResult> {
        let data = std::fs::read(path)
            .map_err(|e| CpacError::IoError(format!("{}: {e}", path.display())))?;
        let original_size = data.len();
        let iterations = self.profile.iterations();
        let enable_msn = force_track != Some(Track::Track2);
        let config = CompressConfig {
            backend: None,
            enable_msn,
            msn_confidence: 0.5,
            force_track,
            level,
            filename: Some(path.to_string_lossy().into_owned()),
            ..Default::default()
        };

        let rss_before = measure_rss_bytes();

        // Warmup iteration
        let _ = crate::compress(&data, &config)?;

        let mut total_compress = Duration::ZERO;
        let mut compressed_data = Vec::new();
        let mut selected_backend = Backend::Zstd;

        for _ in 0..iterations {
            let start = Instant::now();
            let result = crate::compress(&data, &config)?;
            total_compress += start.elapsed();
            selected_backend = result.backend;
            compressed_data = result.data;
        }
        let avg_compress = total_compress / iterations as u32;
        let compressed_size = compressed_data.len();

        let decompressed = crate::decompress(&compressed_data)?;
        let lossless_verified = decompressed.data == data;

        // Warmup decompress
        let _ = crate::decompress(&compressed_data)?;

        let mut total_decompress = Duration::ZERO;
        for _ in 0..iterations {
            let start = Instant::now();
            let _r = crate::decompress(&compressed_data)?;
            total_decompress += start.elapsed();
        }
        let avg_decompress = total_decompress / iterations as u32;

        let raw_lvl = standalone_raw_level(selected_backend, level)
            .map(|l| format!("@{l}"))
            .unwrap_or_default();
        let label = match force_track {
            Some(Track::Track1) => format!("ForceT1(MSN/{selected_backend:?}{raw_lvl})"),
            Some(Track::Track2) => format!("ForceT2(noMSN/{selected_backend:?}{raw_lvl})"),
            None => format!("Auto(SSR/{selected_backend:?}{raw_lvl})"),
        };

        let effective_track = match force_track {
            Some(Track::Track1) => Track::Track1,
            Some(Track::Track2) => Track::Track2,
            None => Track::Track2,
        };

        Ok(BenchResult {
            file: path.to_path_buf(),
            engine_label: label,
            original_size,
            compressed_size,
            ratio: calc_ratio(original_size, compressed_size),
            compress_time: avg_compress,
            decompress_time: avg_decompress,
            compress_throughput_mbs: calc_throughput(original_size, &avg_compress),
            decompress_throughput_mbs: calc_throughput(original_size, &avg_decompress),
            peak_memory_bytes: peak_memory_or_fallback(
                rss_before,
                original_size + compressed_size + original_size,
            ),
            lossless_verified,
            ssr_time: None,
            msn_time: None,
            track: Some(effective_track),
            compression_level: level,
        })
    }

    /// Benchmark a file against a standalone codec
    pub fn bench_standalone(
        &self,
        path: &Path,
        codec: &StandaloneCodec,
    ) -> CpacResult<BenchResult> {
        let data = std::fs::read(path)
            .map_err(|e| cpac_types::CpacError::IoError(format!("{}: {e}", path.display())))?;
        let original_size = data.len();
        let iterations = self.profile.iterations();

        let rss_before = measure_rss_bytes();

        // Warmup iteration
        let _ = codec.compress(&data)?;

        let mut total_compress = Duration::ZERO;
        let mut compressed_data = Vec::new();
        for _ in 0..iterations {
            let start = Instant::now();
            compressed_data = codec.compress(&data)?;
            total_compress += start.elapsed();
        }
        let avg_compress = total_compress / iterations as u32;
        let compressed_size = compressed_data.len();

        // Lossless verification
        let decompressed = codec.decompress(&compressed_data)?;
        let lossless_verified = decompressed == data;

        // Warmup decompress
        let _ = codec.decompress(&compressed_data)?;

        // Decompress timing
        let mut total_decompress = Duration::ZERO;
        for _ in 0..iterations {
            let start = Instant::now();
            let _ = codec.decompress(&compressed_data)?;
            total_decompress += start.elapsed();
        }
        let avg_decompress = total_decompress / iterations as u32;

        let ratio = calc_ratio(original_size, compressed_size);
        let peak_memory_bytes =
            peak_memory_or_fallback(rss_before, original_size + compressed_size + original_size);

        Ok(BenchResult {
            file: path.to_path_buf(),
            engine_label: codec.label(),
            original_size,
            compressed_size,
            ratio,
            compress_time: avg_compress,
            decompress_time: avg_decompress,
            compress_throughput_mbs: calc_throughput(original_size, &avg_compress),
            decompress_throughput_mbs: calc_throughput(original_size, &avg_decompress),
            peak_memory_bytes,
            lossless_verified,
            ssr_time: None, // standalone: no SSR pipeline
            msn_time: None,
            track: None, // standalone: no CPAC track
            compression_level: codec.cpac_level,
        })
    }

    /// Benchmark all files in a directory with CPAC backends + matched standalone baselines.
    #[must_use]
    pub fn bench_directory(&self, dir: &Path, max_size_mb: Option<u64>) -> Vec<BenchResult> {
        let files = CorpusManager::scan_directory(dir, max_size_mb);
        let mut results = Vec::new();
        for file in &files {
            for &backend in &self.backends {
                if let Ok(r) = self.bench_file(file, backend) {
                    results.push(r);
                }
            }
            if !self.skip_baselines {
                let baselines = matched_baselines(self.bench_level);
                for codec in &baselines {
                    if let Ok(r) = self.bench_standalone(file, codec) {
                        results.push(r);
                    }
                }
            }
        }
        results
    }

    /// Summarize results into a corpus summary.
    #[must_use]
    pub fn summarize(corpus_name: &str, results: &[BenchResult]) -> CorpusSummary {
        let total_original: usize = results.iter().map(|r| r.original_size).sum();
        let total_compressed: usize = results.iter().map(|r| r.compressed_size).sum();
        let overall_ratio = if total_compressed > 0 {
            total_original as f64 / total_compressed as f64
        } else {
            0.0
        };
        let n = results.len().max(1) as f64;
        let mean_compress_mbs: f64 = results
            .iter()
            .map(|r| r.compress_throughput_mbs)
            .sum::<f64>()
            / n;
        let mean_decompress_mbs: f64 = results
            .iter()
            .map(|r| r.decompress_throughput_mbs)
            .sum::<f64>()
            / n;
        let total_peak_memory_bytes: usize = results.iter().map(|r| r.peak_memory_bytes).sum();
        let all_lossless = results.iter().all(|r| r.lossless_verified);

        CorpusSummary {
            corpus_name: corpus_name.to_string(),
            results: results.to_vec(),
            total_original,
            total_compressed,
            overall_ratio,
            mean_compress_mbs,
            mean_decompress_mbs,
            total_peak_memory_bytes,
            all_lossless,
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn calc_ratio(original: usize, compressed: usize) -> f64 {
    if compressed > 0 {
        original as f64 / compressed as f64
    } else {
        f64::INFINITY
    }
}

fn calc_throughput(size_bytes: usize, duration: &Duration) -> f64 {
    let secs = duration.as_secs_f64();
    if secs > 0.0 {
        size_bytes as f64 / 1_048_576.0 / secs
    } else {
        f64::INFINITY
    }
}

/// Query the current process RSS (Resident Set Size) in bytes.
///
/// Returns 0 if the measurement is unavailable (e.g. PID lookup fails).
fn measure_rss_bytes() -> usize {
    let Ok(pid) = sysinfo::get_current_pid() else {
        return 0;
    };
    let mut sys = sysinfo::System::new();
    sys.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::Some(&[pid]),
        true,
        sysinfo::ProcessRefreshKind::nothing().with_memory(),
    );
    sys.process(pid).map_or(0, |p| p.memory() as usize)
}

/// Measure RSS delta across a section; fall back to `fallback` if zero.
fn peak_memory_or_fallback(rss_before: usize, fallback: usize) -> usize {
    let rss_after = measure_rss_bytes();
    let measured = rss_after.saturating_sub(rss_before);
    if measured > 0 {
        measured
    } else {
        fallback
    }
}

// ---------------------------------------------------------------------------
// Regression detection
// ---------------------------------------------------------------------------

/// A single entry in a stored regression baseline.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BaselineEntry {
    /// File name (stem only, for cross-machine portability).
    pub file_name: String,
    /// Engine/backend label.
    pub engine_label: String,
    /// Stored compression ratio.
    pub ratio: f64,
    /// Stored compress throughput (MB/s).
    pub compress_mbs: f64,
    /// Stored decompress throughput (MB/s).
    pub decompress_mbs: f64,
}

/// A regression violation detected during a check.
#[derive(Clone, Debug)]
pub struct RegressionViolation {
    pub file_name: String,
    pub engine_label: String,
    pub kind: RegressionKind,
    pub baseline_value: f64,
    pub current_value: f64,
    pub drop_pct: f64,
}

/// Kind of regression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegressionKind {
    Ratio,
    CompressSpeed,
    DecompressSpeed,
}

impl std::fmt::Display for RegressionViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} [{}] {:?} regression: baseline={:.3} current={:.3} drop={:.1}%",
            self.file_name,
            self.engine_label,
            self.kind,
            self.baseline_value,
            self.current_value,
            self.drop_pct
        )
    }
}

/// Save benchmark results as a JSON regression baseline file.
///
/// Only stores the file stem (not full path) for cross-machine portability.
pub fn save_baseline(path: &Path, results: &[BenchResult]) -> CpacResult<()> {
    let entries: Vec<BaselineEntry> = results
        .iter()
        .map(|r| BaselineEntry {
            file_name: r
                .file
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            engine_label: r.engine_label.clone(),
            ratio: r.ratio,
            compress_mbs: r.compress_throughput_mbs,
            decompress_mbs: r.decompress_throughput_mbs,
        })
        .collect();
    let json = serde_json::to_string_pretty(&entries)
        .map_err(|e| CpacError::IoError(format!("baseline serialize: {e}")))?;
    std::fs::write(path, json).map_err(|e| CpacError::IoError(format!("baseline write: {e}")))
}

/// Load a regression baseline from a JSON file.
pub fn load_baseline(path: &Path) -> CpacResult<Vec<BaselineEntry>> {
    let json = std::fs::read_to_string(path)
        .map_err(|e| CpacError::IoError(format!("baseline read: {e}")))?;
    serde_json::from_str(&json).map_err(|e| CpacError::IoError(format!("baseline parse: {e}")))
}

/// Check current results against a stored baseline for regressions.
///
/// - `ratio_tolerance`: fraction drop allowed (e.g. `0.05` = 5% drop OK)
/// - `speed_tolerance`: fraction drop allowed (e.g. `0.10` = 10% drop OK)
///
/// Returns a list of violations (empty = no regressions).
#[must_use]
pub fn check_regressions(
    baseline: &[BaselineEntry],
    current: &[BenchResult],
    ratio_tolerance: f64,
    speed_tolerance: f64,
) -> Vec<RegressionViolation> {
    let mut violations = Vec::new();

    for result in current {
        let file_name = result
            .file
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();

        let Some(entry) = baseline
            .iter()
            .find(|e| e.file_name == file_name && e.engine_label == result.engine_label)
        else {
            continue; // No baseline for this entry — skip
        };

        let check = |kind: RegressionKind, baseline_val: f64, current_val: f64, tol: f64| {
            if baseline_val > 0.0 {
                let drop = (baseline_val - current_val) / baseline_val;
                if drop > tol {
                    Some(RegressionViolation {
                        file_name: file_name.clone(),
                        engine_label: result.engine_label.clone(),
                        kind,
                        baseline_value: baseline_val,
                        current_value: current_val,
                        drop_pct: drop * 100.0,
                    })
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(v) = check(
            RegressionKind::Ratio,
            entry.ratio,
            result.ratio,
            ratio_tolerance,
        ) {
            violations.push(v);
        }
        if let Some(v) = check(
            RegressionKind::CompressSpeed,
            entry.compress_mbs,
            result.compress_throughput_mbs,
            speed_tolerance,
        ) {
            violations.push(v);
        }
        if let Some(v) = check(
            RegressionKind::DecompressSpeed,
            entry.decompress_mbs,
            result.decompress_throughput_mbs,
            speed_tolerance,
        ) {
            violations.push(v);
        }
    }

    violations
}

// ---------------------------------------------------------------------------
// Report generation
// ---------------------------------------------------------------------------

/// Generate a Markdown report from a corpus summary.
#[must_use]
pub fn generate_markdown_report(summary: &CorpusSummary) -> String {
    let mut md = String::new();
    write!(md, "# CPAC Benchmark Report: {}\n\n", summary.corpus_name).ok();
    writeln!(md, "- **Total original**: {} bytes", summary.total_original).ok();
    writeln!(
        md,
        "- **Total compressed**: {} bytes",
        summary.total_compressed
    )
    .ok();
    writeln!(md, "- **Overall ratio**: {:.2}x", summary.overall_ratio).ok();
    writeln!(
        md,
        "- **Mean compress throughput**: {:.1} MB/s",
        summary.mean_compress_mbs
    )
    .ok();
    writeln!(
        md,
        "- **Mean decompress throughput**: {:.1} MB/s",
        summary.mean_decompress_mbs
    )
    .ok();
    writeln!(
        md,
        "- **Lossless verified**: {}",
        if summary.all_lossless { "YES" } else { "FAIL" }
    )
    .ok();
    write!(
        md,
        "- **Peak memory (est.)**: {:.1} MB\n\n",
        summary.total_peak_memory_bytes as f64 / 1_048_576.0
    )
    .ok();
    md.push_str("## Per-file results\n\n");
    for r in &summary.results {
        let track_str = r.track.as_ref().map_or(String::new(), |t| match t {
            Track::Track1 => " T1".to_string(),
            Track::Track2 => " T2".to_string(),
        });
        let ssr_str = r.ssr_time.map_or(String::new(), |d| {
            format!(" SSR:{:.1}ms", d.as_secs_f64() * 1000.0)
        });
        writeln!(
            md,
            "- `{}` [{}]: {:.2}x ratio, {:.1}/{:.1} MB/s (c/d), {:.1} MB mem, lossless={}{}{}",
            r.file.display(),
            r.engine_label,
            r.ratio,
            r.compress_throughput_mbs,
            r.decompress_throughput_mbs,
            r.peak_memory_bytes as f64 / 1_048_576.0,
            r.lossless_verified,
            track_str,
            ssr_str,
        )
        .ok();
    }
    md
}

/// Generate a CSV export from results.
#[must_use]
pub fn generate_csv_export(results: &[BenchResult]) -> String {
    let mut csv = String::from(
        "file,engine,level,original_bytes,compressed_bytes,ratio,compress_ms,decompress_ms,compress_mbs,decompress_mbs,peak_memory_bytes,lossless,ssr_ms,msn_ms,track\n",
    );
    for r in results {
        let ssr_ms = r.ssr_time.map_or(String::new(), |d| {
            format!("{:.3}", d.as_secs_f64() * 1000.0)
        });
        let msn_ms = r.msn_time.map_or(String::new(), |d| {
            format!("{:.3}", d.as_secs_f64() * 1000.0)
        });
        let track_str = r.track.as_ref().map_or("", |t| match t {
            Track::Track1 => "T1",
            Track::Track2 => "T2",
        });
        writeln!(
            csv,
            "{},{},{:?},{},{},{:.4},{:.3},{:.3},{:.2},{:.2},{},{},{},{},{}",
            r.file.display(),
            r.engine_label,
            r.compression_level,
            r.original_size,
            r.compressed_size,
            r.ratio,
            r.compress_time.as_secs_f64() * 1000.0,
            r.decompress_time.as_secs_f64() * 1000.0,
            r.compress_throughput_mbs,
            r.decompress_throughput_mbs,
            r.peak_memory_bytes,
            r.lossless_verified,
            ssr_ms,
            msn_ms,
            track_str,
        )
        .ok();
    }
    csv
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn create_temp_corpus() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("create tempdir");
        // Create test files large enough for XZ/LZMA2 to engage
        // compression (lzma-rs xz_compress stores data uncompressed
        // when the input is below ~64 KB).
        for (name, content) in [
            (
                "text.txt",
                b"Hello World! ".repeat(10_000).as_slice().to_vec(),
            ),
            (
                "binary.bin",
                (0u8..=255).cycle().take(131_072).collect::<Vec<_>>(),
            ),
        ] {
            let path = dir.path().join(name);
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(&content).unwrap();
        }
        dir
    }

    #[test]
    fn corpus_scan() {
        let dir = create_temp_corpus();
        let files = CorpusManager::scan_directory(dir.path(), None);
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn bench_single_file() {
        let dir = create_temp_corpus();
        let file = dir.path().join("text.txt");
        let runner = BenchmarkRunner::new(BenchProfile::Quick);
        let result = runner.bench_file(&file, Backend::Zstd).unwrap();
        assert!(result.ratio > 1.0);
        assert!(result.compress_throughput_mbs > 0.0);
        assert!(result.lossless_verified);
        assert!(result.peak_memory_bytes > 0);
    }

    #[test]
    fn bench_standalone_gzip() {
        let dir = create_temp_corpus();
        let file = dir.path().join("text.txt");
        let runner = BenchmarkRunner::new(BenchProfile::Quick);
        let codec = StandaloneCodec {
            backend: Backend::Gzip,
            cpac_level: CompressionLevel::Best,
        };
        let result = runner.bench_standalone(&file, &codec).unwrap();
        assert!(result.ratio > 1.0);
        assert!(result.lossless_verified);
        assert_eq!(result.engine_label, "gzip-9");
    }

    #[test]
    fn bench_standalone_lzma() {
        let dir = create_temp_corpus();
        let file = dir.path().join("text.txt");
        let runner = BenchmarkRunner::new(BenchProfile::Quick);
        let codec = StandaloneCodec {
            backend: Backend::Lzma,
            cpac_level: CompressionLevel::Default,
        };
        let result = runner.bench_standalone(&file, &codec).unwrap();
        assert!(result.lossless_verified);
        assert_eq!(result.engine_label, "lzma-6");
    }

    #[test]
    fn bench_directory_with_baselines() {
        let dir = create_temp_corpus();
        let runner = BenchmarkRunner::new(BenchProfile::Quick);
        let results = runner.bench_directory(dir.path(), None);
        // 2 files × (12 CPAC backends + 11 standalone baselines) = 46
        assert_eq!(results.len(), 46);
    }

    #[test]
    fn bench_directory_skip_baselines() {
        let dir = create_temp_corpus();
        let mut runner = BenchmarkRunner::new(BenchProfile::Quick);
        runner.skip_baselines = true;
        let results = runner.bench_directory(dir.path(), None);
        // 2 files × 12 CPAC backends = 24
        assert_eq!(results.len(), 24);
    }

    #[test]
    fn standalone_codec_from_label() {
        let codec = StandaloneCodec::from_label("zstd-6").unwrap();
        assert_eq!(codec.backend, Backend::Zstd);
        assert_eq!(codec.cpac_level, CompressionLevel::Default);
        assert_eq!(codec.label(), "zstd-6");

        let codec = StandaloneCodec::from_label("snappy").unwrap();
        assert_eq!(codec.backend, Backend::Snappy);
        assert_eq!(codec.label(), "snappy");

        let codec = StandaloneCodec::from_label("zlib-ng-6").unwrap();
        assert_eq!(codec.backend, Backend::ZlibNg);
        assert_eq!(codec.label(), "zlib-ng-6");

        assert!(StandaloneCodec::from_label("bogus-99").is_none());
    }

    #[test]
    fn summary_and_report() {
        let dir = create_temp_corpus();
        let mut runner = BenchmarkRunner::new(BenchProfile::Quick);
        runner.skip_baselines = true;
        let results = runner.bench_directory(dir.path(), None);
        let summary = BenchmarkRunner::summarize("test", &results);
        assert_eq!(summary.corpus_name, "test");
        assert!(summary.overall_ratio > 0.0);
        assert!(summary.all_lossless);
        let md = generate_markdown_report(&summary);
        assert!(md.contains("CPAC Benchmark Report"));
        assert!(md.contains("Lossless verified**: YES"));
    }

    #[test]
    fn csv_export() {
        let dir = create_temp_corpus();
        let mut runner = BenchmarkRunner::new(BenchProfile::Quick);
        runner.skip_baselines = true;
        let results = runner.bench_directory(dir.path(), None);
        let csv = generate_csv_export(&results);
        assert!(csv.starts_with("file,engine,"));
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 25); // header + 24 data rows
    }

    #[test]
    fn regression_baseline_roundtrip() {
        let dir = create_temp_corpus();
        let mut runner = BenchmarkRunner::new(BenchProfile::Quick);
        runner.skip_baselines = true;
        let results = runner.bench_directory(dir.path(), None);

        // Save baseline to a temp file.
        let baseline_path = dir.path().join("baseline.json");
        save_baseline(&baseline_path, &results).unwrap();

        // Load it back and verify entry count.
        let loaded = load_baseline(&baseline_path).unwrap();
        assert_eq!(loaded.len(), results.len());

        // Check that no regressions are detected against itself.
        let violations = check_regressions(&loaded, &results, 0.05, 0.10);
        assert!(
            violations.is_empty(),
            "Self-check should produce no regressions: {violations:?}"
        );
    }

    #[test]
    fn regression_detects_ratio_drop() {
        let dir = create_temp_corpus();
        let file = dir.path().join("text.txt");
        let runner = BenchmarkRunner::new(BenchProfile::Quick);
        let result = runner.bench_file(&file, Backend::Zstd).unwrap();

        // Build a baseline with an artificially high ratio.
        let baseline = vec![BaselineEntry {
            file_name: "text".to_string(),
            engine_label: result.engine_label.clone(),
            ratio: result.ratio * 10.0, // Baseline claims 10x better ratio
            compress_mbs: result.compress_throughput_mbs,
            decompress_mbs: result.decompress_throughput_mbs,
        }];

        let violations = check_regressions(&baseline, &[result], 0.05, 0.10);
        assert!(
            violations.iter().any(|v| v.kind == RegressionKind::Ratio),
            "Should detect ratio regression"
        );
    }
}
