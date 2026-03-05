// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Benchmarking framework: `BenchmarkRunner`, `CorpusManager`, report generation.

use cpac_types::{Backend, CompressConfig, CpacError, CpacResult, Track};
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
    /// Label: CPAC backend name or baseline engine name.
    pub engine_label: String,
    pub original_size: usize,
    pub compressed_size: usize,
    pub ratio: f64,
    pub compress_time: Duration,
    pub decompress_time: Duration,
    pub compress_throughput_mbs: f64,
    pub decompress_throughput_mbs: f64,
    /// Estimated peak memory in bytes (input + output + overhead).
    pub peak_memory_bytes: usize,
    /// Whether a full compress→decompress lossless roundtrip was verified.
    pub lossless_verified: bool,
}

/// External baseline compression engines for comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaselineEngine {
    /// gzip level 9 (via flate2).
    Gzip9,
    /// Zstandard level 3 (native, not CPAC pipeline).
    Zstd3,
    /// Brotli level 11 (native, not CPAC pipeline).
    Brotli11,
    /// LZMA level 6 (via lzma-rs).
    Lzma6,
}

impl BaselineEngine {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            BaselineEngine::Gzip9 => "gzip-9",
            BaselineEngine::Zstd3 => "zstd-3",
            BaselineEngine::Brotli11 => "brotli-11",
            BaselineEngine::Lzma6 => "lzma-6",
        }
    }

    #[must_use]
    pub fn all() -> &'static [BaselineEngine] {
        &[
            BaselineEngine::Gzip9,
            BaselineEngine::Zstd3,
            BaselineEngine::Brotli11,
            BaselineEngine::Lzma6,
        ]
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
                Backend::Raw,
            ],
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
        let data = std::fs::read(path)
            .map_err(|e| cpac_types::CpacError::IoError(format!("{}: {e}", path.display())))?;
        let original_size = data.len();
        let iterations = self.profile.iterations();
        let config = CompressConfig {
            backend: Some(backend),
            ..Default::default()
        };

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
        // Estimated peak = input + compressed + decompressed buffers
        let peak_memory_bytes = original_size + compressed_size + original_size;

        Ok(BenchResult {
            file: path.to_path_buf(),
            engine_label: format!("{backend:?}"),
            original_size,
            compressed_size,
            ratio,
            compress_time: avg_compress,
            decompress_time: avg_decompress,
            compress_throughput_mbs: compress_mbs,
            decompress_throughput_mbs: decompress_mbs,
            peak_memory_bytes,
            lossless_verified,
        })
    }

    /// Benchmark a file using SSR auto-routing, optionally with MSN (Track 1).
    ///
    /// Unlike [`bench_file`](Self::bench_file) which forces a specific backend,
    /// this lets SSR select the backend automatically — matching the production
    /// `cpac compress` path. The result label reflects the actual track + backend
    /// chosen by SSR, e.g. `"T1(SSR/Brotli)"` or `"T1(MSN/Zstd)"`.
    pub fn bench_file_auto(&self, path: &Path, enable_msn: bool) -> CpacResult<BenchResult> {
        let data = std::fs::read(path)
            .map_err(|e| CpacError::IoError(format!("{}: {e}", path.display())))?;
        let original_size = data.len();
        let iterations = self.profile.iterations();
        let config = CompressConfig {
            backend: None, // SSR auto-selects
            enable_msn,
            msn_confidence: 0.5,
            // Pass filename so MSN can use extension-based domain detection
            // (e.g. .log → syslog confidence bump, .csv → CSV domain, .jsonl → JSONL).
            filename: Some(path.to_string_lossy().into_owned()),
            ..Default::default()
        };

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
        let engine_label = format!("{track_str}({mode_str}/{selected_backend:?})");

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
            peak_memory_bytes: original_size + compressed_size + original_size,
            lossless_verified,
        })
    }

    /// Benchmark a file against a baseline engine (gzip, lzma, etc.).
    pub fn bench_baseline(&self, path: &Path, engine: BaselineEngine) -> CpacResult<BenchResult> {
        let data = std::fs::read(path)
            .map_err(|e| cpac_types::CpacError::IoError(format!("{}: {e}", path.display())))?;
        let original_size = data.len();
        let iterations = self.profile.iterations();

        let mut total_compress = Duration::ZERO;
        let mut compressed_data = Vec::new();
        for _ in 0..iterations {
            let start = Instant::now();
            compressed_data = baseline_compress(&data, engine)?;
            total_compress += start.elapsed();
        }
        let avg_compress = total_compress / iterations as u32;
        let compressed_size = compressed_data.len();

        // Lossless verification
        let decompressed = baseline_decompress(&compressed_data, engine)?;
        let lossless_verified = decompressed == data;

        // Decompress timing
        let mut total_decompress = Duration::ZERO;
        for _ in 0..iterations {
            let start = Instant::now();
            let _ = baseline_decompress(&compressed_data, engine)?;
            total_decompress += start.elapsed();
        }
        let avg_decompress = total_decompress / iterations as u32;

        let ratio = calc_ratio(original_size, compressed_size);
        let peak_memory_bytes = original_size + compressed_size + original_size;

        Ok(BenchResult {
            file: path.to_path_buf(),
            engine_label: engine.label().to_string(),
            original_size,
            compressed_size,
            ratio,
            compress_time: avg_compress,
            decompress_time: avg_decompress,
            compress_throughput_mbs: calc_throughput(original_size, &avg_compress),
            decompress_throughput_mbs: calc_throughput(original_size, &avg_decompress),
            peak_memory_bytes,
            lossless_verified,
        })
    }

    /// Benchmark all files in a directory with CPAC backends + baselines.
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
                for &engine in BaselineEngine::all() {
                    if let Ok(r) = self.bench_baseline(file, engine) {
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

// ---------------------------------------------------------------------------
// Baseline compress / decompress
// ---------------------------------------------------------------------------

fn baseline_compress(data: &[u8], engine: BaselineEngine) -> CpacResult<Vec<u8>> {
    match engine {
        BaselineEngine::Gzip9 => {
            use flate2::write::GzEncoder;
            use flate2::Compression;
            use std::io::Write;
            let mut enc = GzEncoder::new(Vec::new(), Compression::best());
            enc.write_all(data)
                .map_err(|e| cpac_types::CpacError::CompressFailed(format!("gzip: {e}")))?;
            enc.finish()
                .map_err(|e| cpac_types::CpacError::CompressFailed(format!("gzip finish: {e}")))
        }
        BaselineEngine::Zstd3 => zstd::encode_all(std::io::Cursor::new(data), 3)
            .map_err(|e| cpac_types::CpacError::CompressFailed(format!("zstd-3: {e}"))),
        BaselineEngine::Brotli11 => {
            let mut out = Vec::new();
            let params = brotli::enc::BrotliEncoderParams {
                quality: 11,
                ..Default::default()
            };
            brotli::BrotliCompress(&mut std::io::Cursor::new(data), &mut out, &params)
                .map_err(|e| cpac_types::CpacError::CompressFailed(format!("brotli-11: {e}")))?;
            Ok(out)
        }
        BaselineEngine::Lzma6 => {
            let mut out = Vec::new();
            lzma_rs::lzma_compress(&mut std::io::Cursor::new(data), &mut out)
                .map_err(|e| cpac_types::CpacError::CompressFailed(format!("lzma: {e}")))?;
            Ok(out)
        }
    }
}

fn baseline_decompress(data: &[u8], engine: BaselineEngine) -> CpacResult<Vec<u8>> {
    match engine {
        BaselineEngine::Gzip9 => {
            use flate2::read::GzDecoder;
            use std::io::Read;
            let mut dec = GzDecoder::new(data);
            let mut out = Vec::new();
            dec.read_to_end(&mut out)
                .map_err(|e| cpac_types::CpacError::DecompressFailed(format!("gzip: {e}")))?;
            Ok(out)
        }
        BaselineEngine::Zstd3 => zstd::decode_all(std::io::Cursor::new(data))
            .map_err(|e| cpac_types::CpacError::DecompressFailed(format!("zstd-3: {e}"))),
        BaselineEngine::Brotli11 => {
            let mut out = Vec::new();
            brotli::BrotliDecompress(&mut std::io::Cursor::new(data), &mut out)
                .map_err(|e| cpac_types::CpacError::DecompressFailed(format!("brotli-11: {e}")))?;
            Ok(out)
        }
        BaselineEngine::Lzma6 => {
            let mut out = Vec::new();
            lzma_rs::lzma_decompress(&mut std::io::Cursor::new(data), &mut out)
                .map_err(|e| cpac_types::CpacError::DecompressFailed(format!("lzma: {e}")))?;
            Ok(out)
        }
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
        writeln!(
            md,
            "- `{}` [{}]: {:.2}x ratio, {:.1}/{:.1} MB/s (c/d), {:.1} MB mem, lossless={}",
            r.file.display(),
            r.engine_label,
            r.ratio,
            r.compress_throughput_mbs,
            r.decompress_throughput_mbs,
            r.peak_memory_bytes as f64 / 1_048_576.0,
            r.lossless_verified,
        )
        .ok();
    }
    md
}

/// Generate a CSV export from results.
#[must_use]
pub fn generate_csv_export(results: &[BenchResult]) -> String {
    let mut csv = String::from(
        "file,engine,original_bytes,compressed_bytes,ratio,compress_ms,decompress_ms,compress_mbs,decompress_mbs,peak_memory_bytes,lossless\n",
    );
    for r in results {
        writeln!(
            csv,
            "{},{},{},{},{:.4},{:.3},{:.3},{:.2},{:.2},{},{}",
            r.file.display(),
            r.engine_label,
            r.original_size,
            r.compressed_size,
            r.ratio,
            r.compress_time.as_secs_f64() * 1000.0,
            r.decompress_time.as_secs_f64() * 1000.0,
            r.compress_throughput_mbs,
            r.decompress_throughput_mbs,
            r.peak_memory_bytes,
            r.lossless_verified,
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
        // Create a few test files.
        for (name, content) in [
            ("text.txt", b"Hello World! ".repeat(100).as_slice().to_vec()),
            (
                "binary.bin",
                (0u8..=255).cycle().take(2048).collect::<Vec<_>>(),
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
    fn bench_baseline_gzip() {
        let dir = create_temp_corpus();
        let file = dir.path().join("text.txt");
        let runner = BenchmarkRunner::new(BenchProfile::Quick);
        let result = runner.bench_baseline(&file, BaselineEngine::Gzip9).unwrap();
        assert!(result.ratio > 1.0);
        assert!(result.lossless_verified);
        assert_eq!(result.engine_label, "gzip-9");
    }

    #[test]
    fn bench_baseline_lzma() {
        let dir = create_temp_corpus();
        let file = dir.path().join("text.txt");
        let runner = BenchmarkRunner::new(BenchProfile::Quick);
        let result = runner.bench_baseline(&file, BaselineEngine::Lzma6).unwrap();
        assert!(result.ratio > 1.0);
        assert!(result.lossless_verified);
    }

    #[test]
    fn bench_directory_with_baselines() {
        let dir = create_temp_corpus();
        let runner = BenchmarkRunner::new(BenchProfile::Quick);
        let results = runner.bench_directory(dir.path(), None);
        // 2 files × (5 CPAC backends + 4 baselines) = 18 results
        assert_eq!(results.len(), 18);
    }

    #[test]
    fn bench_directory_skip_baselines() {
        let dir = create_temp_corpus();
        let mut runner = BenchmarkRunner::new(BenchProfile::Quick);
        runner.skip_baselines = true;
        let results = runner.bench_directory(dir.path(), None);
        // 2 files × 5 CPAC backends = 10
        assert_eq!(results.len(), 10);
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
        assert_eq!(lines.len(), 11); // header + 10 data rows
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
