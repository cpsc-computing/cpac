// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! CPAC CLI — command-line interface for compression/decompression.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::fn_params_excessive_bools,
    clippy::needless_pass_by_value
)]

mod config;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use cpac_types::{AccelBackend, Backend, CompressConfig, CompressionLevel, Preset, ResourceConfig};
use indicatif::{ProgressBar, ProgressStyle};
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process;

/// CPAC file extension.
const CPAC_EXT: &str = ".cpac";

#[derive(Parser)]
#[command(
    name = "cpac",
    about = "CPAC — Constraint-Projected Adaptive Compression",
    version = cpac_engine::VERSION,
    long_about = "High-performance constraint-projected adaptive compression engine.\nSupports SSR-guided preprocessing, DAG-based transform pipelines, and multiple entropy backends.",
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Compress a file (or stdin with -).
    #[command(alias = "c")]
    Compress {
        /// Input file path (use - for stdin).
        input: PathBuf,
        /// Output file path (default: input + .cpac; use - for stdout).
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Entropy backend: raw, zstd, brotli, gzip, lzma.
        #[arg(short, long)]
        backend: Option<String>,
        /// Overwrite existing output file.
        #[arg(short, long)]
        force: bool,
        /// Keep original file after compression.
        #[arg(short, long, default_value_t = true)]
        keep: bool,
        /// Recursively compress all files in a directory.
        #[arg(short, long)]
        recursive: bool,
        /// Verbose output (-v = basic, -vv = detailed, -vvv = debug).
        #[arg(short, long, action = clap::ArgAction::Count)]
        verbose: u8,
        /// Worker threads (0 = auto: physical cores).
        #[arg(short = 'T', long, default_value_t = 0)]
        threads: usize,
        /// Max memory budget in MB (0 = auto: 25% of RAM, 256-8192 MB).
        #[arg(short = 'M', long, default_value_t = 0)]
        max_memory: usize,
        /// Use memory-mapped I/O (auto for files > 64 MB, force with flag).
        #[arg(long)]
        mmap: bool,
        /// Enable Multi-Scale Normalization (MSN) for domain-specific semantic extraction.
        /// Extracts repeated structure from JSON, CSV, XML, logs, etc. for higher ratios.
        /// Default: disabled.
        #[arg(long)]
        enable_msn: bool,
        /// MSN minimum confidence threshold (0.0-1.0). Higher = more selective.
        /// Default: 0.5.
        #[arg(long, default_value_t = 0.5, requires = "enable_msn")]
        msn_confidence: f64,
        /// Force specific MSN domain (overrides auto-detect).
        /// Format: category.type (e.g., text.json, log.apache).
        /// Use "cpac list-domains" to see available domains.
        #[arg(long, requires = "enable_msn")]
        msn_domain: Option<String>,
        /// Compression quality preset: fast, default, or best.
        /// fast    = brotli-6 / zstd-1   (throughput focus)
        /// default = brotli-11 / zstd-3  (matches industry baseline; fair comparison)
        /// best    = brotli-11 / zstd-9  (maximum ratio)
        #[arg(long, default_value = "default")]
        level: String,
        /// Enable data-driven smart transform selection.
        /// Uses the structure analyzer to recommend transforms based on SSR/MSN analysis
        /// and empirical corpus benchmarks. Automatically picks the best transform(s)
        /// via adaptive trials.
        #[arg(long)]
        smart: bool,
        /// Named preset: turbo, balanced, maximum, archive, max-ratio.
        /// Auto-configures level, transforms, MSN, block size, and threading.
        /// max-ratio forces brotli-11 with full MSN for absolute best ratio.
        /// Individual flags (--level, --smart, etc.) override the preset.
        #[arg(long)]
        preset: Option<String>,
        /// Hardware accelerator: auto, software, qat, iaa, gpu, fpga, sve2.
        #[arg(long, default_value = "auto")]
        accel: String,
        /// Pre-trained dictionary (.cpac-dict) for improved compression on
        /// homogeneous corpora.  Train with `cpac.py train-dict`.
        #[arg(long)]
        dict: Option<PathBuf>,
        /// Use incremental streaming compression (bounded memory, large files).
        /// Output uses the CPAC streaming wire format (.cpac-stream).
        #[arg(long)]
        streaming: bool,
        /// Block size in bytes for streaming compression (default: 1 MiB).
        #[arg(long, default_value_t = 1 << 20, requires = "streaming")]
        stream_block: usize,
        /// Disable automatic dictionary selection from .work/benchmarks/.
        #[arg(long)]
        no_auto_dict: bool,
        /// Transcode-compress lossless images (PNG, BMP, TIFF, WebP).
        /// Decodes to raw pixels, applies byte-plane split + delta + zstd.
        /// Falls back to normal compression for non-image files.
        #[arg(long)]
        transcode: bool,
        /// Encrypt output with a password (reads CPAC_PASSWORD env or prompts).
        /// Produces CPCE wire format (.cpac-enc) combining compression + encryption.
        #[arg(long)]
        encrypt: bool,
        /// PQC hybrid key file for encryption (.cpac-pub).
        /// When provided with --encrypt, uses X25519 + ML-KEM-768 instead of password.
        #[arg(long, requires = "encrypt")]
        encrypt_key: Option<PathBuf>,
        /// AEAD algorithm for encryption: chacha20 or aes256gcm.
        #[arg(long, default_value = "chacha20", requires = "encrypt")]
        encrypt_algo: String,
    },
    /// Decompress a file (or stdin with -).
    #[command(alias = "d", alias = "x")]
    Decompress {
        /// Input file path (.cpac; use - for stdin).
        input: PathBuf,
        /// Output file path (default: strip .cpac; use - for stdout).
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Overwrite existing output file.
        #[arg(short, long)]
        force: bool,
        /// Keep compressed file after decompression.
        #[arg(short, long, default_value_t = true)]
        keep: bool,
        /// Verbose output (-v = basic, -vv = detailed, -vvv = debug).
        #[arg(short, long, action = clap::ArgAction::Count)]
        verbose: u8,
        /// Worker threads (0 = auto: physical cores).
        #[arg(short = 'T', long, default_value_t = 0)]
        threads: usize,
        /// Use memory-mapped I/O (auto for files > 64 MB, force with flag).
        #[arg(long)]
        mmap: bool,
        /// Use streaming decompression (required for .cpac-stream files).
        #[arg(long)]
        streaming: bool,
        /// Secret key file for CPCE PQC decryption (.cpac-sec).
        /// Password decryption reads CPAC_PASSWORD env var or prompts.
        #[arg(long)]
        encrypt_key: Option<PathBuf>,
    },
    /// Show file info or host system details.
    #[command(alias = "i")]
    Info {
        /// Input file path (omit with --host).
        input: Option<PathBuf>,
        /// Print detected host system info (CPU, cores, RAM, SIMD).
        #[arg(long)]
        host: bool,
    },
    /// List available profiles.
    #[command(alias = "lp")]
    ListProfiles,
    /// List available backends.
    #[command(alias = "lb")]
    ListBackends,
    /// List available MSN domains.
    #[command(alias = "ld")]
    ListDomains,
    /// Benchmark compression on a file.
    #[command(alias = "bench")]
    Benchmark {
        /// Input file to benchmark.
        input: PathBuf,
        /// Number of iterations (overrides profile).
        #[arg(short = 'n', long)]
        iterations: Option<usize>,
        /// Quick mode: 3 iterations, 2 baselines, <10s.
        #[arg(long, conflicts_with_all = ["full", "iterations"])]
        quick: bool,
        /// Full mode: 50 iterations, 4 baselines, 20-60 min.
        #[arg(long, conflicts_with_all = ["quick", "iterations"])]
        full: bool,
        /// Skip baseline engines (gzip, zstd, brotli, lzma).
        #[arg(long)]
        skip_baselines: bool,
        /// Output results as JSON (machine-readable).
        #[arg(long)]
        json: bool,
        /// Also benchmark Track 1: SSR auto-routing (no MSN) and SSR+MSN.
        /// Shows what CPAC actually does in production vs. the individual backends above.
        #[arg(long)]
        track1: bool,
        /// Discovery mode: run with forced Track 1 (MSN on every block) and forced Track 2
        /// (MSN on no block) to compare MSN ceiling vs floor across the file.
        /// Use with --skip-baselines to reduce runtime.
        #[arg(long)]
        discovery: bool,
        /// CPAC entropy backends to benchmark (comma-separated).
        /// Default: zstd,brotli,gzip,lzma,raw.
        #[arg(long, value_delimiter = ',')]
        backends: Option<Vec<String>>,
        /// CPAC compression levels to benchmark (comma-separated).
        /// Each level runs CPAC backends at that level, then the matched
        /// baselines at the same effective effort.
        /// Available: fast, default, high, best.
        /// Default: just "default" for a single-level run.
        #[arg(long, value_delimiter = ',')]
        levels: Option<Vec<String>>,
        /// Override auto-matched baselines with an explicit list (comma-separated).
        /// Available: gzip-9, zstd-1, zstd-3, zstd-12, zstd-19, brotli-6, brotli-11, lzma-6.
        /// When provided, the same baselines run for every level (no auto-matching).
        #[arg(long, value_delimiter = ',')]
        baselines: Option<Vec<String>>,
    },
    /// Analyze file structure and recommend optimal compression strategy.
    #[command(alias = "a")]
    Analyze {
        /// Input file to analyze.
        input: PathBuf,
    },
    /// Profile a file: trial compression matrix, gap analysis, recommendations.
    #[command(alias = "p")]
    Profile {
        /// Input file to profile.
        input: PathBuf,
        /// Quick mode: fewer trials, faster results.
        #[arg(long)]
        quick: bool,
    },
    /// Analyze file with Auto-CAS constraint inference.
    #[command(alias = "cas")]
    AutoCas {
        /// Input file to analyze.
        input: PathBuf,
        /// Also compress with CAS and show results.
        #[arg(long)]
        compress: bool,
    },
    /// Encrypt a file with a password.
    Encrypt {
        /// Input file path.
        input: PathBuf,
        /// Output file path (default: input + .cpac-enc).
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// AEAD algorithm: chacha20 or aes256gcm.
        #[arg(short, long, default_value = "chacha20")]
        algorithm: String,
    },
    /// Decrypt a file with a password.
    Decrypt {
        /// Input file path (.cpac-enc).
        input: PathBuf,
        /// Output file path.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// AEAD algorithm used during encryption.
        #[arg(short, long, default_value = "chacha20")]
        algorithm: String,
    },
    /// Create a .cpar archive from a directory.
    #[command(alias = "ar")]
    ArchiveCreate {
        /// Directory to archive.
        input: PathBuf,
        /// Output archive file.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Solid mode: concatenate all files and compress as one stream.
        /// Better ratio for similar files (configs, logs) but requires
        /// full decompression to extract any single file.
        #[arg(long)]
        solid: bool,
    },
    /// Extract a .cpar archive.
    #[command(alias = "ax")]
    ArchiveExtract {
        /// Archive file.
        input: PathBuf,
        /// Output directory.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// List contents of a .cpar archive.
    #[command(alias = "al")]
    ArchiveList {
        /// Archive file.
        input: PathBuf,
    },
    /// Start a metrics/health HTTP server for monitoring.
    ///
    /// Exposes Prometheus-compatible metrics at /metrics and a health check at /health.
    /// Designed for sidecar or daemon deployments in data center environments.
    #[command(alias = "s")]
    Serve {
        /// Listen address (host:port).
        #[arg(long, default_value = "127.0.0.1:9100")]
        listen: String,
        /// Enable Prometheus metrics endpoint at /metrics.
        #[arg(long, default_value_t = true)]
        metrics: bool,
    },
    /// Batch-compress files from a manifest or directory with content-aware routing.
    ///
    /// Each file's compression config is auto-tuned based on SSR analysis and
    /// file extension. Shares a global thread pool and optional dictionary.
    #[command(alias = "cb")]
    CompressBatch {
        /// Manifest YAML file listing files+configs, OR a directory to scan.
        input: PathBuf,
        /// Output directory for compressed files (default: alongside originals).
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Overwrite existing output files.
        #[arg(short, long)]
        force: bool,
        /// Worker threads (0 = auto: physical cores).
        #[arg(short = 'T', long, default_value_t = 0)]
        threads: usize,
        /// Max concurrent files to compress in parallel.
        #[arg(long, default_value_t = 4)]
        concurrency: usize,
        /// Shared pre-trained dictionary for all files.
        #[arg(long)]
        dict: Option<PathBuf>,
        /// Content-aware routes file (YAML) mapping extensions to configs.
        #[arg(long)]
        routes: Option<PathBuf>,
        /// Verbose output.
        #[arg(short, long, action = clap::ArgAction::Count)]
        verbose: u8,
    },
    /// Post-quantum cryptography operations.
    #[command(alias = "pq")]
    Pqc {
        #[command(subcommand)]
        action: PqcAction,
    },
    /// Auto-analyze a directory and recommend optimal compression settings.
    #[command(alias = "aa")]
    AutoAnalyze {
        /// Directory (or single file) to analyze.
        input: PathBuf,
        /// Write Markdown report to a file.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Quick mode: fewer files, smaller size cap.
        #[arg(long)]
        quick: bool,
        /// Write recommended YAML config to .cpac-config.yml in the directory.
        #[arg(long)]
        write_config: bool,
    },
    /// Transform Laboratory tools.
    #[command(alias = "l")]
    Lab {
        #[command(subcommand)]
        action: LabAction,
    },
    /// Generate shell completions.
    Completions {
        /// Shell to generate completions for.
        shell: Shell,
    },
}

/// Lab sub-commands.
#[derive(Subcommand)]
enum LabAction {
    /// Calibrate the analyzer from benchmark CSV results.
    ///
    /// Reads all .csv files from the benchmark directory, computes per-transform
    /// win-rates by file extension, and writes calibration.json.
    Calibrate {
        /// Directory containing benchmark CSV files.
        /// Default: .work/benchmarks/transform-study/
        #[arg(short, long)]
        dir: Option<PathBuf>,
        /// Output path for calibration.json.
        /// Default: .work/benchmarks/calibration.json
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Print results to stdout as JSON instead of writing a file.
        #[arg(long)]
        stdout: bool,
    },
}

/// PQC sub-commands.
#[derive(Subcommand)]
enum PqcAction {
    /// Generate a hybrid key pair (X25519 + ML-KEM-768).
    Keygen {
        /// Output directory for key files.
        #[arg(short, long, default_value = ".")]
        output: PathBuf,
    },
    /// Hybrid-encrypt a file using recipient's public keys.
    Encrypt {
        /// Input file.
        input: PathBuf,
        /// Recipient public key file (.cpac-pub).
        #[arg(short = 'k', long)]
        public_key: PathBuf,
        /// Output file (default: input + .cpac-pqe).
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Hybrid-decrypt a file using your secret keys.
    Decrypt {
        /// Input file (.cpac-pqe).
        input: PathBuf,
        /// Secret key file (.cpac-sec).
        #[arg(short = 'k', long)]
        secret_key: PathBuf,
        /// Output file.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Sign a file with ML-DSA-65.
    Sign {
        /// File to sign.
        input: PathBuf,
        /// Signing key file (.cpac-sec).
        #[arg(short = 'k', long)]
        secret_key: PathBuf,
    },
    /// Verify a signature.
    Verify {
        /// File to verify.
        input: PathBuf,
        /// Signature file (.cpac-sig).
        #[arg(short = 's', long)]
        signature: PathBuf,
        /// Public key file (.cpac-pub).
        #[arg(short = 'k', long)]
        public_key: PathBuf,
    },
}

fn parse_backend(s: &str) -> Result<Backend, String> {
    match s.to_lowercase().as_str() {
        "raw" => Ok(Backend::Raw),
        "zstd" => Ok(Backend::Zstd),
        "brotli" => Ok(Backend::Brotli),
        "gzip" | "gz" => Ok(Backend::Gzip),
        "lzma" => Ok(Backend::Lzma),
        "xz" => Ok(Backend::Xz),
        "lz4" => Ok(Backend::Lz4),
        "snappy" => Ok(Backend::Snappy),
        "lzham" => Ok(Backend::Lzham),
        "lizard" => Ok(Backend::Lizard),
        "zlib-ng" | "zlibng" => Ok(Backend::ZlibNg),
        "openzl" => Ok(Backend::OpenZl),
        other => Err(format!(
            "unknown backend: {other} (available: raw, zstd, brotli, gzip, lzma, xz, lz4, snappy, lzham, lizard, zlib-ng, openzl)"
        )),
    }
}

fn format_size(size: usize) -> String {
    if size < 1024 {
        format!("{size} B")
    } else if size < 1024 * 1024 {
        format!("{:.1} KB", size as f64 / 1024.0)
    } else if size < 1024 * 1024 * 1024 {
        format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", size as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

/// Read data from file or stdin.
fn read_input(path: &PathBuf) -> Vec<u8> {
    if path.to_str() == Some("-") {
        let mut buf = Vec::new();
        io::stdin().read_to_end(&mut buf).unwrap_or_else(|e| {
            eprintln!("Error reading from stdin: {e}");
            eprintln!("Hint: Check that stdin is properly piped or redirected.");
            process::exit(1);
        });
        buf
    } else {
        std::fs::read(path).unwrap_or_else(|e| {
            eprintln!("Error reading file '{}': {e}", path.display());
            if e.kind() == std::io::ErrorKind::NotFound {
                eprintln!("Hint: Verify the file path and ensure the file exists.");
            } else if e.kind() == std::io::ErrorKind::PermissionDenied {
                eprintln!("Hint: Check file permissions or run with appropriate privileges.");
            }
            process::exit(1);
        })
    }
}

/// Write data to file or stdout.
fn write_output(path: &PathBuf, data: &[u8], force: bool) {
    if path.to_str() == Some("-") {
        io::stdout().write_all(data).unwrap_or_else(|e| {
            eprintln!("Error writing to stdout: {e}");
            eprintln!("Hint: Check that stdout is not closed or redirected incorrectly.");
            process::exit(1);
        });
    } else {
        if !force && path.exists() {
            eprintln!("Error: Output file '{}' already exists", path.display());
            eprintln!("Hint: Use --force (-f) to overwrite, or specify a different output path with --output.");
            process::exit(1);
        }
        std::fs::write(path, data).unwrap_or_else(|e| {
            eprintln!("Error writing to file '{}': {e}", path.display());
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                eprintln!("Hint: Check directory permissions or file ownership.");
            } else if e.kind() == std::io::ErrorKind::NotFound {
                eprintln!("Hint: Ensure the parent directory exists.");
            }
            process::exit(1);
        });
    }
}

/// Collect files to compress (single file or recursive directory).
fn collect_files(path: &PathBuf, recursive: bool) -> Vec<PathBuf> {
    if path.to_str() == Some("-") {
        return vec![path.clone()];
    }
    if path.is_dir() {
        if !recursive {
            eprintln!("Error: {} is a directory (use --recursive)", path.display());
            process::exit(1);
        }
        let mut files = Vec::new();
        collect_files_recursive(path, &mut files);
        files
    } else {
        vec![path.clone()]
    }
}

fn collect_files_recursive(dir: &PathBuf, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                collect_files_recursive(&p, out);
            } else if !p.to_string_lossy().ends_with(CPAC_EXT) {
                out.push(p);
            }
        }
    }
}

/// Build a [`ResourceConfig`] merging user overrides on top of the
/// auto-detected safe defaults (physical cores, 25 % RAM).
fn build_resources(threads: usize, max_memory: usize) -> ResourceConfig {
    let mut rc = cpac_engine::auto_resource_config();
    if threads > 0 {
        rc.max_threads = threads;
    }
    if max_memory > 0 {
        rc.max_memory_mb = max_memory;
    }
    rc
}

/// Load raw zstd dictionary bytes from a .cpac-dict file.
fn load_dict_bytes(p: &std::path::Path, verbose: u8) -> Vec<u8> {
    let bytes = std::fs::read(p).unwrap_or_else(|e| {
        eprintln!("Error reading dictionary '{}': {e}", p.display());
        process::exit(1);
    });
    // If CPDI format, strip the 37-byte header to get raw zstd dict
    if bytes.len() > 37 && &bytes[0..4] == b"CPDI" {
        let size = u32::from_le_bytes([bytes[13], bytes[14], bytes[15], bytes[16]]) as usize;
        if bytes.len() >= 37 + size {
            if verbose >= 1 {
                eprintln!("Loaded CPAC dictionary: {} bytes ({})", size, p.display());
            }
            return bytes[37..37 + size].to_vec();
        }
    }
    if verbose >= 1 {
        eprintln!(
            "Loaded raw dictionary: {} bytes ({})",
            bytes.len(),
            p.display()
        );
    }
    bytes
}

/// Try to auto-select a dictionary from .work/benchmarks/ catalog.
fn auto_select_dict_for_input(input: &std::path::Path, verbose: u8) -> Option<Vec<u8>> {
    let catalog_dir = std::path::Path::new(".work/benchmarks");
    if !catalog_dir.is_dir() {
        return None;
    }
    let catalog = cpac_dict::scan_catalog(catalog_dir).ok()?;
    if catalog.is_empty() {
        return None;
    }
    let filename = input.file_name().and_then(|s| s.to_str());
    let entry = cpac_dict::auto_select_dictionary(filename, &catalog)?;
    if verbose >= 1 {
        eprintln!(
            "Auto-selected dictionary: {} (stem={})",
            entry.path.display(),
            entry.stem,
        );
    }
    Some(load_dict_bytes(&entry.path, verbose))
}

fn parse_level(s: &str) -> CompressionLevel {
    match s.to_ascii_lowercase().as_str() {
        "ultrafast" | "uf" | "0" => CompressionLevel::UltraFast,
        "fast" | "f" | "1" => CompressionLevel::Fast,
        "high" | "h" | "3" => CompressionLevel::High,
        "best" | "max" | "b" | "4" => CompressionLevel::Best,
        _ => CompressionLevel::Default,
    }
}

#[allow(clippy::too_many_arguments)]
fn cmd_compress(
    input: PathBuf,
    output: Option<PathBuf>,
    backend: Option<String>,
    force: bool,
    _keep: bool,
    recursive: bool,
    verbose: u8,
    threads: usize,
    max_memory: usize,
    mmap: bool,
    enable_msn: bool,
    msn_confidence: f64,
    msn_domain: Option<String>,
    level: CompressionLevel,
    smart: bool,
    transcode: bool,
    preset: Option<Preset>,
    accel: Option<AccelBackend>,
    dict: Option<PathBuf>,
    no_auto_dict: bool,
    streaming: bool,
    stream_block: usize,
    encrypt: bool,
    encrypt_key: Option<PathBuf>,
    encrypt_algo: String,
) {
    let backend = backend.map(|b| match parse_backend(&b) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error: {e}");
            process::exit(1);
        }
    });

    // Load dictionary: explicit --dict flag, or auto-select from catalog
    let dictionary: Option<Vec<u8>> = if let Some(p) = dict {
        Some(load_dict_bytes(&p, verbose))
    } else if !no_auto_dict {
        auto_select_dict_for_input(&input, verbose)
    } else {
        None
    };

    let resources = build_resources(threads, max_memory);
    let files = collect_files(&input, recursive);

    // Setup progress bar for multiple files
    let progress_bar = if files.len() > 1 && verbose == 0 {
        let pb = ProgressBar::new(files.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("[{bar:40.cyan/blue}] {pos}/{len} {msg}")
                .unwrap()
                .progress_chars("█▓░"),
        );
        Some(pb)
    } else {
        None
    };

    for file_path in &files {
        // Start from preset (if given) or default config
        let mut config = if let Some(p) = preset {
            CompressConfig::from_preset(p)
        } else {
            CompressConfig::default()
        };
        // CLI flags override preset values
        config.backend = backend;
        config.resources = Some(resources.clone());
        config.msn_domain = msn_domain.clone();
        config.msn_verbose = verbose >= 3;
        config.accelerator = accel;
        config.dictionary = dictionary.clone();
        // Only override preset defaults when user explicitly provided the flag
        if preset.is_none() || smart {
            config.enable_smart_transforms = smart || preset.is_none_or(|p| p.smart_transforms());
        }
        // MSN is only enabled by explicit --enable-msn flag.
        // Presets no longer auto-enable MSN (see Preset::msn_enabled docs).
        config.enable_msn = enable_msn;
        if preset.is_none() {
            config.level = level;
            config.msn_confidence = msn_confidence;
        }

        // Decide whether to use mmap (flag or auto for files > 64 MB)
        let use_mmap = mmap
            || (file_path.to_str() != Some("-")
                && cpac_streaming::mmap::should_use_mmap(file_path));

        let (compressed_data, original_size, compressed_size) = if transcode {
            // Transcode path: lossless image → pixel decode → byte-plane split → zstd
            let data = read_input(file_path);
            let orig = data.len();
            match cpac_transcode::transcode_compress(&data) {
                Ok(frame) => {
                    if verbose >= 2 {
                        eprintln!("Transcode: lossless image detected, using CPTC format");
                    }
                    let csz = frame.len();
                    (frame, orig, csz)
                }
                Err(_) => {
                    // Not a recognized lossless image — fall through to normal CPAC
                    if verbose >= 1 {
                        eprintln!("Transcode: not a lossless image, using standard CPAC");
                    }
                    let res = cpac_engine::compress(&data, &config);
                    match res {
                        Ok(r) => (r.data.clone(), r.original_size, r.data.len()),
                        Err(e) => {
                            eprintln!("Compression failed for '{}': {e}", file_path.display());
                            if files.len() > 1 {
                                continue;
                            }
                            process::exit(1);
                        }
                    }
                }
            }
        } else if streaming {
            // Streaming path: incremental, bounded-memory compression.
            let msn_cfg = cpac_streaming::MsnConfig {
                enable: enable_msn,
                confidence_threshold: msn_confidence,
                ..Default::default()
            };
            let data = read_input(file_path);
            let orig = data.len();
            let mut compressor = match cpac_streaming::stream::StreamingCompressor::with_msn(
                config.clone(),
                msn_cfg,
                stream_block,
                64 * 1024 * 1024,
            ) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Streaming compressor init failed: {e}");
                    process::exit(1);
                }
            };
            if let Err(e) = compressor.write(&data) {
                eprintln!("Streaming write failed: {e}");
                process::exit(1);
            }
            match compressor.finish() {
                Ok(frame) => {
                    let csz = frame.len();
                    (frame, orig, csz)
                }
                Err(e) => {
                    eprintln!("Streaming finish failed: {e}");
                    process::exit(1);
                }
            }
        } else if use_mmap && file_path.to_str() != Some("-") {
            match cpac_streaming::mmap::mmap_compress(file_path, &config) {
                Ok(r) => {
                    let csz = r.data.len();
                    (r.data, r.original_size, csz)
                }
                Err(e) => {
                    eprintln!("Compression failed for '{}': {e}", file_path.display());
                    eprintln!(
                        "Hint: Check input file format and try different backends with --backend."
                    );
                    if files.len() > 1 {
                        eprintln!("       Continuing with remaining files...\n");
                        continue;
                    }
                    process::exit(1);
                }
            }
        } else {
            let data = read_input(file_path);
            let use_parallel =
                data.len() >= cpac_engine::PARALLEL_THRESHOLD && resources.max_threads > 1;
            let res = if use_parallel {
                cpac_engine::compress_parallel(
                    &data,
                    &config,
                    cpac_engine::DEFAULT_BLOCK_SIZE,
                    resources.max_threads,
                )
            } else {
                cpac_engine::compress(&data, &config)
            };
            match res {
                Ok(r) => {
                    let csz = r.data.len();
                    (r.data, r.original_size, csz)
                }
                Err(e) => {
                    eprintln!("Compression failed for '{}': {e}", file_path.display());
                    eprintln!(
                        "Hint: Check input file format and try different backends with --backend."
                    );
                    if files.len() > 1 {
                        eprintln!("       Continuing with remaining files...\n");
                        continue;
                    }
                    process::exit(1);
                }
            }
        };

        // --- Phase 7: optional post-compression encryption ---
        let (final_data, final_size, enc_label) = if encrypt {
            let aead_algo = match parse_aead_algo(&encrypt_algo) {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("Error: {e}");
                    process::exit(1);
                }
            };
            let encrypted = if let Some(ref key_path) = encrypt_key {
                // PQC hybrid mode
                let pub_data = std::fs::read(key_path).unwrap_or_else(|e| {
                    eprintln!("Error reading public key '{}': {e}", key_path.display());
                    process::exit(1);
                });
                cpac_streaming::cpce::cpce_encrypt_pqc(&compressed_data, &pub_data).unwrap_or_else(
                    |e| {
                        eprintln!("PQC encryption failed: {e}");
                        process::exit(1);
                    },
                )
            } else {
                // Password mode
                let password = std::env::var("CPAC_PASSWORD").unwrap_or_else(|_| {
                    eprint!("Password: ");
                    let mut p = String::new();
                    io::stdin().read_line(&mut p).unwrap();
                    p.trim().to_string()
                });
                cpac_streaming::cpce::cpce_encrypt_password(
                    &compressed_data,
                    password.as_bytes(),
                    aead_algo,
                )
                .unwrap_or_else(|e| {
                    eprintln!("Encryption failed: {e}");
                    process::exit(1);
                })
            };
            let esz = encrypted.len();
            let label = if encrypt_key.is_some() {
                "encrypted+pqc"
            } else {
                "encrypted"
            };
            (encrypted, esz, Some(label))
        } else {
            (compressed_data, compressed_size, None)
        };

        let ext = if encrypt {
            ".cpac-enc"
        } else if streaming {
            ".cpac-stream"
        } else {
            CPAC_EXT
        };
        let out_path = if files.len() == 1 {
            output.clone().unwrap_or_else(|| {
                let mut p = file_path.as_os_str().to_owned();
                p.push(ext);
                PathBuf::from(p)
            })
        } else {
            let mut p = file_path.as_os_str().to_owned();
            p.push(ext);
            PathBuf::from(p)
        };

        write_output(&out_path, &final_data, force);

        if let Some(ref pb) = progress_bar {
            pb.set_message(format!(
                "{}",
                file_path.file_name().unwrap_or_default().to_string_lossy()
            ));
            pb.inc(1);
        }

        if verbose >= 2 {
            let ratio = if compressed_size > 0 {
                original_size as f64 / compressed_size as f64
            } else {
                0.0
            };
            let savings = if original_size > 0 {
                (1.0 - compressed_size as f64 / original_size as f64) * 100.0
            } else {
                0.0
            };
            println!("Input:      {}", file_path.display());
            println!("Output:     {}", out_path.display());
            println!("Original:   {}", format_size(original_size));
            println!("Compressed: {}", format_size(compressed_size));
            println!("Ratio:      {ratio:.2}x ({savings:.1}% saved)");
            let mode_str = match (streaming, enc_label) {
                (true, Some(l)) => format!("streaming + {l}"),
                (false, Some(l)) => format!("standard + {l}"),
                (true, None) => "streaming".to_string(),
                (false, None) => "standard".to_string(),
            };
            println!("Mode:       {mode_str}");
            if encrypt {
                println!("Encrypted:  {}", format_size(final_size));
            }
            if verbose >= 3 {
                println!("Threads:    {}", resources.max_threads);
                println!("Memory:     {} MB", resources.max_memory_mb);
                println!("MMap:       {use_mmap}");
            }
            if files.len() > 1 {
                println!();
            }
        } else if verbose == 1 || progress_bar.is_none() {
            let ratio = if compressed_size > 0 {
                original_size as f64 / compressed_size as f64
            } else {
                0.0
            };
            if encrypt {
                println!(
                    "{} -> {} [{ratio:.2}x, encrypted]",
                    file_path.display(),
                    out_path.display()
                );
            } else {
                println!(
                    "{} -> {} [{ratio:.2}x]",
                    file_path.display(),
                    out_path.display()
                );
            }
        }
    }

    if let Some(pb) = progress_bar {
        pb.finish_with_message("Done");
    }
}

#[allow(clippy::too_many_arguments)]
fn cmd_decompress(
    input: PathBuf,
    output: Option<PathBuf>,
    force: bool,
    _keep: bool,
    verbose: u8,
    threads: usize,
    mmap: bool,
    streaming: bool,
    decrypt_key: Option<PathBuf>,
) {
    let use_mmap =
        mmap || (input.to_str() != Some("-") && cpac_streaming::mmap::should_use_mmap(&input));

    // Read raw data first to check for CPCE magic.
    let raw_data = read_input(&input);
    let is_cpce = cpac_streaming::cpce::is_cpce(&raw_data);

    // Phase 7: if CPCE-encrypted, decrypt first to get the inner compressed frame.
    let data = if is_cpce {
        if verbose >= 1 {
            eprintln!("Detected CPCE encrypted frame, decrypting...");
        }
        let password = std::env::var("CPAC_PASSWORD").ok();
        let sec_key_data = decrypt_key.as_ref().map(|p| {
            std::fs::read(p).unwrap_or_else(|e| {
                eprintln!("Error reading secret key '{}': {e}", p.display());
                process::exit(1);
            })
        });
        let pw_bytes = password.as_deref().map(|s| s.as_bytes());
        let sk_bytes = sec_key_data.as_deref();

        match cpac_streaming::cpce::cpce_auto_decrypt(&raw_data, pw_bytes, sk_bytes) {
            Ok(inner) => inner,
            Err(e) => {
                eprintln!("CPCE decryption failed for '{}': {e}", input.display());
                eprintln!(
                    "Hint: Set CPAC_PASSWORD env var or provide --encrypt-key with secret key."
                );
                process::exit(1);
            }
        }
    } else {
        raw_data
    };

    // Detect streaming format by filename or explicit flag.
    let is_stream = streaming || input.extension().is_some_and(|e| e == "cpac-stream");

    let decompressed_data = if is_stream {
        let mut decomp = match cpac_streaming::stream::StreamingDecompressor::new() {
            Ok(d) => d,
            Err(e) => {
                eprintln!("Streaming decompressor init failed: {e}");
                process::exit(1);
            }
        };
        if let Err(e) = decomp.feed(&data) {
            eprintln!(
                "Streaming decompression failed for '{}': {e}",
                input.display()
            );
            eprintln!("Hint: Ensure the file was compressed with --streaming.");
            process::exit(1);
        }
        decomp.read_output()
    } else if !is_cpce && use_mmap && input.to_str() != Some("-") {
        // mmap path only when not CPCE (we already read into memory for CPCE)
        match cpac_streaming::mmap::mmap_decompress(&input) {
            Ok(r) => r.data,
            Err(e) => {
                eprintln!("Decompression failed for '{}': {e}", input.display());
                process::exit(1);
            }
        }
    } else {
        let resources = build_resources(threads, 0);
        let res = if cpac_engine::is_cpbl(&data) {
            cpac_engine::decompress_parallel(&data, resources.max_threads)
        } else {
            cpac_engine::decompress(&data)
        };
        match res {
            Ok(r) => r.data,
            Err(e) => {
                eprintln!("Decompression failed for '{}': {e}", input.display());
                eprintln!("Hint: Ensure the file is a valid CPAC archive and not corrupted.");
                eprintln!(
                    "      Use 'cpac info {}' to inspect the file.",
                    input.display()
                );
                process::exit(1);
            }
        }
    };

    let out_path = output.unwrap_or_else(|| {
        let s = input.to_string_lossy();
        if let Some(stripped) = s
            .strip_suffix(".cpac-enc")
            .or_else(|| s.strip_suffix(".cpac-stream"))
            .or_else(|| s.strip_suffix(CPAC_EXT))
        {
            PathBuf::from(stripped)
        } else {
            let mut p = input.as_os_str().to_owned();
            p.push(".out");
            PathBuf::from(p)
        }
    });

    write_output(&out_path, &decompressed_data, force);

    if verbose >= 2 {
        let input_size = if input.to_str() == Some("-") {
            0
        } else {
            input.metadata().map(|m| m.len() as usize).unwrap_or(0)
        };
        println!("Input:       {}", input.display());
        println!("Output:      {}", out_path.display());
        println!("Compressed:  {}", format_size(input_size));
        println!("Original:    {}", format_size(decompressed_data.len()));
        let mode_str = match (is_stream, is_cpce) {
            (true, true) => "streaming + decrypted",
            (false, true) => "standard + decrypted",
            (true, false) => "streaming",
            (false, false) => "standard",
        };
        println!("Mode:        {mode_str}");
        if verbose >= 3 {
            let resources = build_resources(threads, 0);
            println!("Threads:     {}", resources.max_threads);
            println!("MMap:        {use_mmap}");
        }
    } else {
        let dec_label = if is_cpce { ", decrypted" } else { "" };
        println!(
            "{} -> {} [{}{}]",
            input.display(),
            out_path.display(),
            format_size(decompressed_data.len()),
            dec_label,
        );
    }
}

fn cmd_info(input: Option<PathBuf>, host: bool) {
    if host {
        let info = cpac_engine::cached_host_info();
        print!("{info}");
        let rc = cpac_engine::auto_resource_config();
        println!("  Threads:   {} (physical cores)", rc.max_threads);
        println!("  Mem cap:   {} MB (25% of RAM)", rc.max_memory_mb);

        // Accelerator summary
        let accels = cpac_engine::accel::detect_accelerators();
        let selected = cpac_engine::accel::select_accelerator(None, &accels);
        println!();
        println!("Accelerators:");
        for a in &accels {
            let marker = if *a == selected { " (active)" } else { "" };
            println!("  {:?}{marker}", a);
        }
        println!();
        println!("Env vars to enable hardware accel:");
        println!("  CPAC_QAT_ENABLED=1     Intel QAT");
        println!("  CPAC_IAA_ENABLED=1     Intel IAA (Sapphire Rapids+)");
        println!("  CPAC_GPU_ENABLED=1     GPU Compute (CUDA/Vulkan)");
        println!("  CPAC_XILINX_ENABLED=1  AMD Xilinx Alveo FPGA");
        println!("  CPAC_SVE2_ENABLED=1    ARM SVE2 (AArch64 only)");

        if input.is_none() {
            return;
        }
        println!();
    }

    let Some(path) = input else {
        eprintln!("Error: provide an input file or use --host");
        process::exit(1);
    };

    let data = read_input(&path);
    let ssr = cpac_ssr::analyze(&data);

    println!("File:        {}", path.display());
    println!("Size:        {}", format_size(data.len()));
    println!("Track:       {:?}", ssr.track);
    println!("Viability:   {:.3}", ssr.viability_score);
    println!("Entropy:     {:.3} bits/byte", ssr.entropy_estimate);
    println!("ASCII ratio: {:.3}", ssr.ascii_ratio);
    if let Some(ref hint) = ssr.domain_hint {
        println!("Domain:      {hint:?}");
    }
}

fn cmd_list_profiles() {
    let cache = cpac_engine::ProfileCache::with_builtins();
    println!("Available profiles:");
    let mut names: Vec<&str> = cache.profile_names();
    names.sort_unstable();
    for name in names {
        if let Some(profile) = cache.get_profile(name) {
            println!("  {:<16} {}", name, profile.description);
        }
    }
}

fn cmd_list_backends() {
    println!("Available backends:");
    println!("  raw       No compression (passthrough)");
    println!("  zstd      Zstandard compression (default for most data)");
    println!("  brotli    Brotli compression (better for text)");
    println!("  gzip      Gzip/Deflate (RFC 1952, wide compatibility)");
    println!("  lzma      LZMA compression (raw LZMA stream)");
    println!("  xz        XZ container format (LZMA2)");
    println!("  lz4       LZ4 compression (fast + HC modes)");
    println!("  snappy    Snappy compression (no levels)");
    println!("  lzham     LZHAM compression");
    println!("  lizard    Lizard compression");
    println!("  zlib-ng   zlib-ng compression");
}

fn cmd_list_domains() {
    use cpac_msn::global_registry;

    println!("Available MSN domains:");
    println!("  Domain ID           Description");
    println!("  ------------------  ------------");

    let registry = global_registry();
    let mut domain_ids = registry.list_domains();
    domain_ids.sort();

    for domain_id in domain_ids {
        if let Some(domain) = registry.get(&domain_id) {
            let info = domain.info();
            println!("  {:<18}  {}", info.id, info.name);
        }
    }

    println!();
    println!("Use --msn-domain=<id> to force a specific domain.");
    println!("Default: auto-detect based on content.");
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::fn_params_excessive_bools)]
fn cmd_benchmark(
    input: PathBuf,
    iterations: Option<usize>,
    quick: bool,
    full: bool,
    skip_baselines: bool,
    json: bool,
    _track1: bool,
    discovery: bool,
    cli_backends: Option<Vec<String>>,
    cli_levels: Option<Vec<String>>,
    cli_baselines: Option<Vec<String>>,
) {
    use cpac_engine::{
        matched_baselines, parse_compression_level, BenchProfile, BenchmarkRunner, StandaloneCodec,
    };

    // Determine bench profile (iteration count)
    let profile = if quick {
        BenchProfile::Quick
    } else if full {
        BenchProfile::Full
    } else {
        BenchProfile::Balanced
    };

    let mut runner = BenchmarkRunner::new(profile);
    if skip_baselines {
        runner.skip_baselines = true;
    }

    // Override backends from --backends flag
    if let Some(ref names) = cli_backends {
        let mut backends = Vec::new();
        for name in names {
            match parse_backend(name) {
                Ok(b) => backends.push(b),
                Err(e) => eprintln!("Warning: {e}"),
            }
        }
        if !backends.is_empty() {
            runner.backends = backends;
        }
    }

    // Parse --levels (default: [Default])
    let levels: Vec<CompressionLevel> = if let Some(ref names) = cli_levels {
        names
            .iter()
            .filter_map(|s| {
                let lvl = parse_compression_level(s);
                if lvl.is_none() {
                    eprintln!("Warning: unknown level '{s}', skipping");
                }
                lvl
            })
            .collect()
    } else {
        vec![CompressionLevel::Default]
    };
    let levels = if levels.is_empty() {
        vec![CompressionLevel::Default]
    } else {
        levels
    };

    // Parse --baselines override (if provided, same for all levels)
    let baselines_override: Option<Vec<StandaloneCodec>> = cli_baselines.as_ref().map(|labels| {
        labels
            .iter()
            .filter_map(|l| {
                let codec = StandaloneCodec::from_label(l);
                if codec.is_none() {
                    eprintln!("Warning: unknown baseline '{l}', skipping");
                }
                codec
            })
            .collect()
    });

    // Override iterations if -n was provided
    let actual_iterations = iterations.unwrap_or_else(|| profile.iterations());
    runner.profile = match actual_iterations {
        1 => BenchProfile::Quick,
        n if n <= 5 => BenchProfile::Balanced,
        _ => BenchProfile::Full,
    };

    let mode_label = if quick {
        "Quick"
    } else if full {
        "Full"
    } else if iterations.is_some() {
        "Custom"
    } else {
        "Balanced"
    };

    println!("CPAC Benchmark ({mode_label} mode, {actual_iterations} iterations)");
    println!("File: {}\n", input.display());

    let mut all_results = Vec::new();

    // --- Per-level benchmark: CPAC backends + matched baselines ---
    for &level in &levels {
        let level_tag = format!("{level:?}");
        if levels.len() > 1 {
            println!("--- CPAC level: {level_tag} ---");
        }

        // CPAC backends at this level
        for &backend in &runner.backends {
            match runner.bench_file_with_level(&input, backend, Some(level)) {
                Ok(result) => {
                    let label = result.engine_label.clone();
                    let ram_mb = result.peak_memory_bytes as f64 / 1_048_576.0;
                    let ssr_tag = result.ssr_time.map_or(String::new(), |d| {
                        format!("  SSR:{:.1}ms", d.as_secs_f64() * 1000.0)
                    });
                    println!(
                        "  {:16}  ratio: {:5.2}x  compress: {:6.1} MB/s  decompress: {:6.1} MB/s  RAM: {:5.1} MB  verified: {}{}",
                        label,
                        result.ratio,
                        result.compress_throughput_mbs,
                        result.decompress_throughput_mbs,
                        ram_mb,
                        if result.lossless_verified { "YES" } else { "NO" },
                        ssr_tag,
                    );
                    all_results.push(result);
                }
                Err(e) => eprintln!("  {:16}  ERROR: {}", format!("{backend:?}"), e),
            }
        }

        // Matched standalone baselines (or override)
        if !runner.skip_baselines {
            println!();
            let baselines = if let Some(ref over) = baselines_override {
                over.clone()
            } else {
                matched_baselines(level)
            };
            for codec in &baselines {
                match runner.bench_standalone(&input, codec) {
                    Ok(result) => {
                        let ram_mb = result.peak_memory_bytes as f64 / 1_048_576.0;
                        println!(
                            "  {:16}  ratio: {:5.2}x  compress: {:6.1} MB/s  decompress: {:6.1} MB/s  RAM: {:5.1} MB  verified: {}",
                            result.engine_label,
                            result.ratio,
                            result.compress_throughput_mbs,
                            result.decompress_throughput_mbs,
                            ram_mb,
                            if result.lossless_verified { "YES" } else { "NO" }
                        );
                        all_results.push(result);
                    }
                    Err(e) => eprintln!("  {:16}  ERROR: {}", codec.label(), e),
                }
            }
        }

        if levels.len() > 1 {
            println!();
        }
    }

    // Always run Track 1 (SSR auto) and Track 2 (SSR+MSN)
    // Use the first requested level so auto-routing is compared fairly.
    let auto_level = levels[0];
    println!();
    println!("  --- Track 1+2: SSR auto-routing (level: {auto_level:?}) ---");
    for enable_msn in [false, true] {
        match runner.bench_file_auto(&input, enable_msn, auto_level) {
            Ok(result) => {
                let ram_mb = result.peak_memory_bytes as f64 / 1_048_576.0;
                let ssr_tag = result.ssr_time.map_or(String::new(), |d| {
                    format!("  SSR:{:.1}ms", d.as_secs_f64() * 1000.0)
                });
                println!(
                    "  {:20}  ratio: {:5.2}x  compress: {:6.1} MB/s  decompress: {:6.1} MB/s  RAM: {:5.1} MB  verified: {}{}",
                    result.engine_label,
                    result.ratio,
                    result.compress_throughput_mbs,
                    result.decompress_throughput_mbs,
                    ram_mb,
                    if result.lossless_verified { "YES" } else { "NO" },
                    ssr_tag,
                );
                all_results.push(result);
            }
            Err(e) => eprintln!(
                "  Track({})  ERROR: {}",
                if enable_msn { "MSN" } else { "SSR" },
                e
            ),
        }
    }

    // MSN per-backend: test each backend with MSN preprocessing enabled.
    // Skip Raw backend (MSN irrelevant) and keep only compressing backends.
    {
        println!();
        println!("  --- MSN per-backend (level: {auto_level:?}) ---");
        for &backend in &runner.backends {
            if backend == cpac_engine::Backend::Raw {
                continue;
            }
            match runner.bench_file_msn(&input, backend, auto_level) {
                Ok(result) => {
                    let ram_mb = result.peak_memory_bytes as f64 / 1_048_576.0;
                    println!(
                        "  {:24}  ratio: {:5.2}x  compress: {:6.1} MB/s  decompress: {:6.1} MB/s  RAM: {:5.1} MB  verified: {}",
                        result.engine_label,
                        result.ratio,
                        result.compress_throughput_mbs,
                        result.decompress_throughput_mbs,
                        ram_mb,
                        if result.lossless_verified { "YES" } else { "NO" }
                    );
                    all_results.push(result);
                }
                Err(e) => eprintln!("  MSN/{:?}  ERROR: {}", backend, e),
            }
        }
    }

    // Discovery: forced-T1 (MSN on every block) vs forced-T2 (MSN on no block).
    if discovery {
        use cpac_engine::Track;
        println!();
        println!("  --- Discovery: forced track override ---");
        for force_track in [Some(Track::Track2), Some(Track::Track1)] {
            match runner.bench_file_forced_track(&input, force_track, auto_level) {
                Ok(result) => {
                    println!(
                        "  {:26}  ratio: {:5.2}x  compress: {:6.1} MB/s  decompress: {:6.1} MB/s  verified: {}",
                        result.engine_label,
                        result.ratio,
                        result.compress_throughput_mbs,
                        result.decompress_throughput_mbs,
                        if result.lossless_verified { "YES" } else { "NO" }
                    );
                    all_results.push(result);
                }
                Err(e) => eprintln!("  ForceTrack  ERROR: {e}"),
            }
        }
    }

    // Summary / JSON output
    if json {
        println!("[");
        for (i, r) in all_results.iter().enumerate() {
            let comma = if i + 1 < all_results.len() { "," } else { "" };
            println!(
                "  {{\"engine\":\"{}\",\"ratio\":{:.4},\"compress_mbs\":{:.2},\"decompress_mbs\":{:.2},\"verified\":{}}}{}",
                r.engine_label, r.ratio, r.compress_throughput_mbs, r.decompress_throughput_mbs,
                r.lossless_verified,
                comma
            );
        }
        println!("]");
    } else if !all_results.is_empty() {
        println!();
        let best_ratio = all_results
            .iter()
            .max_by(|a, b| a.ratio.partial_cmp(&b.ratio).unwrap())
            .unwrap();
        let best_speed = all_results
            .iter()
            .max_by(|a, b| {
                a.compress_throughput_mbs
                    .partial_cmp(&b.compress_throughput_mbs)
                    .unwrap()
            })
            .unwrap();
        println!(
            "Best ratio:        {} ({:.2}x)",
            best_ratio.engine_label, best_ratio.ratio
        );
        println!(
            "Fastest compress:  {} ({:.1} MB/s)",
            best_speed.engine_label, best_speed.compress_throughput_mbs
        );
    }
}

fn cmd_auto_cas(input: PathBuf, compress: bool) {
    let data = read_input(&input);
    let values: Vec<i64> = data.iter().map(|&b| i64::from(b)).collect();
    let analysis = cpac_cas::analyze_columns(&[("data".into(), values)]);

    println!("Auto-CAS analysis for {}", input.display());
    println!("  Total DoF:       {:.1}", analysis.total_dof);
    println!("  Constrained DoF: {:.1}", analysis.constrained_dof);
    println!(
        "  Estimated benefit: {:.1}%",
        analysis.estimated_benefit * 100.0
    );
    for (col, constraints) in &analysis.constraints {
        println!("  Column '{col}':");
        for c in constraints {
            println!("    - {c:?}");
        }
    }

    if compress {
        let cas_data = cpac_cas::cas_compress(&data);
        println!(
            "  CAS frame size: {} (original {})",
            format_size(cas_data.len()),
            format_size(data.len())
        );
    }
}

fn parse_aead_algo(s: &str) -> Result<cpac_crypto::AeadAlgorithm, String> {
    match s.to_lowercase().as_str() {
        "chacha20" | "chacha20poly1305" => Ok(cpac_crypto::AeadAlgorithm::ChaCha20Poly1305),
        "aes256gcm" | "aes-256-gcm" | "aes" => Ok(cpac_crypto::AeadAlgorithm::Aes256Gcm),
        other => Err(format!(
            "unknown algorithm: {other} (available: chacha20, aes256gcm)"
        )),
    }
}

fn cmd_encrypt(input: PathBuf, output: Option<PathBuf>, algorithm: String) {
    let algo = match parse_aead_algo(&algorithm) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Error: {e}");
            process::exit(1);
        }
    };

    let data = read_input(&input);

    // Read password from env or prompt
    let password = std::env::var("CPAC_PASSWORD").unwrap_or_else(|_| {
        eprint!("Password: ");
        let mut p = String::new();
        io::stdin().read_line(&mut p).unwrap();
        p.trim().to_string()
    });

    let (salt, nonce, ciphertext) =
        match cpac_crypto::encrypt_with_password(&data, password.as_bytes(), algo) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Encryption error: {e}");
                process::exit(1);
            }
        };

    // Format: [salt_len:u8][salt][nonce_len:u8][nonce][ciphertext]
    let mut out_data = Vec::new();
    out_data.push(salt.len() as u8);
    out_data.extend_from_slice(&salt);
    out_data.push(nonce.len() as u8);
    out_data.extend_from_slice(&nonce);
    out_data.extend_from_slice(&ciphertext);

    let out_path = output.unwrap_or_else(|| {
        let mut p = input.as_os_str().to_owned();
        p.push(".cpac-enc");
        PathBuf::from(p)
    });

    write_output(&out_path, &out_data, true);
    println!(
        "{} -> {} (encrypted, {})",
        input.display(),
        out_path.display(),
        format_size(out_data.len())
    );
}

fn cmd_decrypt(input: PathBuf, output: Option<PathBuf>, algorithm: String) {
    let algo = match parse_aead_algo(&algorithm) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Error: {e}");
            process::exit(1);
        }
    };

    let data = read_input(&input);
    if data.len() < 4 {
        eprintln!("Error: file too small to be encrypted");
        process::exit(1);
    }

    let salt_len = data[0] as usize;
    let salt = &data[1..=salt_len];
    let nonce_len = data[1 + salt_len] as usize;
    let nonce = &data[2 + salt_len..2 + salt_len + nonce_len];
    let ciphertext = &data[2 + salt_len + nonce_len..];

    let password = std::env::var("CPAC_PASSWORD").unwrap_or_else(|_| {
        eprint!("Password: ");
        let mut p = String::new();
        io::stdin().read_line(&mut p).unwrap();
        p.trim().to_string()
    });

    let plaintext = match cpac_crypto::decrypt_with_password(
        ciphertext,
        password.as_bytes(),
        salt,
        nonce,
        algo,
    ) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Decryption error: {e}");
            process::exit(1);
        }
    };

    let out_path = output.unwrap_or_else(|| {
        let s = input.to_string_lossy();
        if let Some(stripped) = s.strip_suffix(".cpac-enc") {
            PathBuf::from(stripped)
        } else {
            let mut p = input.as_os_str().to_owned();
            p.push(".dec");
            PathBuf::from(p)
        }
    });

    write_output(&out_path, &plaintext, true);
    println!(
        "{} -> {} (decrypted, {})",
        input.display(),
        out_path.display(),
        format_size(plaintext.len())
    );
}

fn cmd_archive_create(input: PathBuf, output: Option<PathBuf>, solid: bool) {
    if !input.is_dir() {
        eprintln!("Error: {} is not a directory", input.display());
        process::exit(1);
    }
    let config = CompressConfig::default();
    let archive_data = if solid {
        match cpac_archive::create_archive_solid(&input, &config) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("Archive creation error (solid): {e}");
                process::exit(1);
            }
        }
    } else {
        match cpac_archive::create_archive(&input, &config) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("Archive creation error: {e}");
                process::exit(1);
            }
        }
    };

    let mode_str = if solid { "solid" } else { "regular" };
    let out_path = output.unwrap_or_else(|| {
        let mut p = input.as_os_str().to_owned();
        p.push(".cpar");
        PathBuf::from(p)
    });

    write_output(&out_path, &archive_data, true);
    println!(
        "{} -> {} ({}, {})",
        input.display(),
        out_path.display(),
        format_size(archive_data.len()),
        mode_str,
    );
}

fn cmd_archive_extract(input: PathBuf, output: Option<PathBuf>) {
    let data = read_input(&input);
    let out_dir = output.unwrap_or_else(|| {
        let s = input.to_string_lossy();
        if let Some(stripped) = s.strip_suffix(".cpar") {
            PathBuf::from(stripped)
        } else {
            PathBuf::from(".")
        }
    });

    std::fs::create_dir_all(&out_dir).unwrap_or_else(|e| {
        eprintln!("Error creating output directory: {e}");
        process::exit(1);
    });

    let entries = match cpac_archive::extract_archive(&data, &out_dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Archive extraction error: {e}");
            process::exit(1);
        }
    };

    println!("Extracted {} files to {}", entries.len(), out_dir.display());
    for entry in &entries {
        println!(
            "  {} ({})",
            entry.path,
            format_size(entry.original_size as usize)
        );
    }
}

fn cmd_archive_list(input: PathBuf) {
    let data = read_input(&input);
    let entries = match cpac_archive::list_archive(&data) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Archive list error: {e}");
            process::exit(1);
        }
    };

    println!("{} entries in {}:", entries.len(), input.display());
    for entry in &entries {
        println!(
            "  {:<40} {:>10} -> {:>10}",
            entry.path,
            format_size(entry.original_size as usize),
            format_size(entry.compressed_size as usize)
        );
    }
}

fn cmd_pqc(action: PqcAction) {
    match action {
        PqcAction::Keygen { output } => {
            let kp = cpac_crypto::hybrid::hybrid_keygen().unwrap_or_else(|e| {
                eprintln!("Key generation error: {e}");
                process::exit(1);
            });

            // Serialize: pub = x25519_pub(32) + mlkem_pub
            let mut pub_data = Vec::new();
            pub_data.extend_from_slice(&kp.x25519_public);
            pub_data.extend_from_slice(&kp.mlkem_public);
            let pub_path = output.join("cpac-hybrid.pub");
            std::fs::write(&pub_path, &pub_data).unwrap_or_else(|e| {
                eprintln!("Error writing {}: {e}", pub_path.display());
                process::exit(1);
            });

            // Serialize: sec = x25519_sec(32) + mlkem_sec
            let mut sec_data = Vec::new();
            sec_data.extend_from_slice(&kp.x25519_secret);
            sec_data.extend_from_slice(&kp.mlkem_secret);
            let sec_path = output.join("cpac-hybrid.sec");
            std::fs::write(&sec_path, &sec_data).unwrap_or_else(|e| {
                eprintln!("Error writing {}: {e}", sec_path.display());
                process::exit(1);
            });

            println!(
                "Generated hybrid keypair:\n  Public:  {}\n  Secret:  {}",
                pub_path.display(),
                sec_path.display()
            );
        }
        PqcAction::Encrypt {
            input,
            public_key,
            output,
        } => {
            let data = read_input(&input);
            let pub_data = std::fs::read(&public_key).unwrap_or_else(|e| {
                eprintln!("Error reading public key: {e}");
                process::exit(1);
            });
            if pub_data.len() < 32 {
                eprintln!("Invalid public key file");
                process::exit(1);
            }
            let x_pub = &pub_data[..32];
            let mlkem_pub = &pub_data[32..];

            let encrypted = cpac_crypto::hybrid::hybrid_encrypt(&data, x_pub, mlkem_pub)
                .unwrap_or_else(|e| {
                    eprintln!("Hybrid encryption error: {e}");
                    process::exit(1);
                });

            let out_path = output.unwrap_or_else(|| {
                let mut p = input.as_os_str().to_owned();
                p.push(".cpac-pqe");
                PathBuf::from(p)
            });
            write_output(&out_path, &encrypted, true);
            println!(
                "{} -> {} (hybrid-encrypted, {})",
                input.display(),
                out_path.display(),
                format_size(encrypted.len())
            );
        }
        PqcAction::Decrypt {
            input,
            secret_key,
            output,
        } => {
            let data = read_input(&input);
            let sec_data = std::fs::read(&secret_key).unwrap_or_else(|e| {
                eprintln!("Error reading secret key: {e}");
                process::exit(1);
            });
            if sec_data.len() < 32 {
                eprintln!("Invalid secret key file");
                process::exit(1);
            }
            let x_sec = &sec_data[..32];
            let mlkem_sec = &sec_data[32..];

            let decrypted = cpac_crypto::hybrid::hybrid_decrypt(&data, x_sec, mlkem_sec)
                .unwrap_or_else(|e| {
                    eprintln!("Hybrid decryption error: {e}");
                    process::exit(1);
                });

            let out_path = output.unwrap_or_else(|| {
                let s = input.to_string_lossy();
                if let Some(stripped) = s.strip_suffix(".cpac-pqe") {
                    PathBuf::from(stripped)
                } else {
                    let mut p = input.as_os_str().to_owned();
                    p.push(".dec");
                    PathBuf::from(p)
                }
            });
            write_output(&out_path, &decrypted, true);
            println!(
                "{} -> {} (hybrid-decrypted, {})",
                input.display(),
                out_path.display(),
                format_size(decrypted.len())
            );
        }
        PqcAction::Sign { input, secret_key } => {
            let data = read_input(&input);
            let sec_data = std::fs::read(&secret_key).unwrap_or_else(|e| {
                eprintln!("Error reading secret key: {e}");
                process::exit(1);
            });
            // ML-DSA-65 signing key follows after x25519 secret (32 bytes)
            let dsa_sk = if sec_data.len() > 32 {
                &sec_data[32..]
            } else {
                &sec_data
            };

            let sig =
                cpac_crypto::pqc::pqc_sign(&data, dsa_sk, cpac_crypto::pqc::PqcAlgorithm::MlDsa65)
                    .unwrap_or_else(|e| {
                        eprintln!("Signing error: {e}");
                        process::exit(1);
                    });

            let sig_path = {
                let mut p = input.as_os_str().to_owned();
                p.push(".cpac-sig");
                PathBuf::from(p)
            };
            std::fs::write(&sig_path, &sig).unwrap_or_else(|e| {
                eprintln!("Error writing signature: {e}");
                process::exit(1);
            });
            println!("Signature: {} ({} bytes)", sig_path.display(), sig.len());
        }
        PqcAction::Verify {
            input,
            signature,
            public_key,
        } => {
            let data = read_input(&input);
            let sig_data = std::fs::read(&signature).unwrap_or_else(|e| {
                eprintln!("Error reading signature: {e}");
                process::exit(1);
            });
            let pub_data = std::fs::read(&public_key).unwrap_or_else(|e| {
                eprintln!("Error reading public key: {e}");
                process::exit(1);
            });
            // ML-DSA-65 verifying key follows after x25519 public (32 bytes)
            let dsa_vk = if pub_data.len() > 32 {
                &pub_data[32..]
            } else {
                &pub_data
            };

            match cpac_crypto::pqc::pqc_verify(
                &data,
                &sig_data,
                dsa_vk,
                cpac_crypto::pqc::PqcAlgorithm::MlDsa65,
            ) {
                Ok(true) => println!("Signature VALID"),
                Ok(false) => {
                    eprintln!("Signature INVALID");
                    process::exit(1);
                }
                Err(e) => {
                    eprintln!("Verification error: {e}");
                    process::exit(1);
                }
            }
        }
    }
}

fn cmd_auto_analyze(input: PathBuf, output: Option<PathBuf>, quick: bool, write_config: bool) {
    if !input.is_dir() {
        eprintln!("Error: {} is not a directory", input.display());
        eprintln!("Hint: auto-analyze requires a directory. Use 'cpac analyze' for single files.");
        process::exit(1);
    }

    eprintln!("Analyzing {}...", input.display());
    let report = cpac_lab::auto_analyze::auto_analyze(&input, quick);
    let md = cpac_lab::auto_analyze::format_report(&report);

    if let Some(ref out_path) = output {
        std::fs::write(out_path, &md).unwrap_or_else(|e| {
            eprintln!("Error writing report: {e}");
            process::exit(1);
        });
        println!("Report written to {}", out_path.display());
    } else {
        print!("{md}");
    }

    if write_config {
        let config_path = input.join(".cpac-config.yml");
        std::fs::write(&config_path, &report.recommended_config).unwrap_or_else(|e| {
            eprintln!("Error writing config: {e}");
            process::exit(1);
        });
        println!("Config written to {}", config_path.display());
    }
}

fn cmd_analyze(input: PathBuf) {
    let data = read_input(&input);
    let filename = input.to_str();
    let profile = cpac_engine::analyze_structure(&data, filename);
    print!("{}", cpac_engine::format_profile(&profile));
}

fn cmd_profile(input: PathBuf, quick: bool) {
    let data = read_input(&input);
    let filename = input.to_str();
    match cpac_engine::profile_file(&data, filename, quick) {
        Ok(result) => {
            print!("{}", cpac_engine::format_profile_result(&result));
        }
        Err(e) => {
            eprintln!("Profiling error: {e}");
            process::exit(1);
        }
    }
}

fn cmd_lab(action: LabAction) {
    match action {
        LabAction::Calibrate {
            dir,
            output,
            stdout,
        } => {
            let bench_dir =
                dir.unwrap_or_else(|| PathBuf::from(".work/benchmarks/transform-study"));
            if !bench_dir.is_dir() {
                eprintln!(
                    "Error: benchmark directory not found: {}",
                    bench_dir.display()
                );
                eprintln!("Hint: Run transform_study experiments first, or specify --dir.");
                process::exit(1);
            }

            let csvs = cpac_lab::calibrate::discover_csvs(&bench_dir);
            if csvs.is_empty() {
                eprintln!("No CSV files found in {}", bench_dir.display());
                process::exit(1);
            }
            eprintln!(
                "Reading {} CSV files from {}",
                csvs.len(),
                bench_dir.display()
            );

            let cal = cpac_lab::calibrate::calibrate(&bench_dir);
            let json = serde_json::to_string_pretty(&cal).unwrap();

            if stdout {
                println!("{json}");
            } else {
                let out_path =
                    output.unwrap_or_else(|| PathBuf::from(".work/benchmarks/calibration.json"));
                if let Some(parent) = out_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                std::fs::write(&out_path, &json).unwrap_or_else(|e| {
                    eprintln!("Error writing calibration: {e}");
                    process::exit(1);
                });
                println!("Calibration written to {}", out_path.display());
                println!(
                    "  {} transforms, {} data rows from {} CSV files",
                    cal.transforms.len(),
                    cal.total_rows,
                    cal.csv_files.len(),
                );

                // Print a quick summary
                println!();
                println!(
                    "{:<24}  {:>5}  {:>7}  {:>8}  {:>12}",
                    "Transform", "Files", "WinRate", "Wins", "TotalGain"
                );
                println!("{}", "─".repeat(62));
                for (name, tc) in &cal.transforms {
                    let o = &tc.overall;
                    println!(
                        "{:<24}  {:>5}  {:>6.1}%  {:>8}  {:>+12}",
                        name,
                        o.files,
                        o.win_rate * 100.0,
                        o.win_count,
                        o.total_gain_bytes,
                    );
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Phase 8A: Prometheus metrics endpoint
// ---------------------------------------------------------------------------

fn cmd_serve(listen: String, _metrics: bool) {
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;

    let listener = TcpListener::bind(&listen).unwrap_or_else(|e| {
        eprintln!("Error binding to {listen}: {e}");
        process::exit(1);
    });
    println!("CPAC metrics server listening on http://{listen}");
    println!("  GET /metrics  — Prometheus metrics");
    println!("  GET /health   — health check");
    println!("Press Ctrl+C to stop.\n");

    // Global counters (atomic for thread-safety if we ever go async)
    use std::sync::atomic::{AtomicU64, Ordering};
    static REQ_COUNT: AtomicU64 = AtomicU64::new(0);
    let start = std::time::Instant::now();

    for stream in listener.incoming() {
        let mut stream = match stream {
            Ok(s) => s,
            Err(_) => continue,
        };
        REQ_COUNT.fetch_add(1, Ordering::Relaxed);

        // Read first request line
        let mut reader = BufReader::new(&stream);
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() {
            continue;
        }

        let (status, content_type, body) = if line.starts_with("GET /metrics") {
            let host_info = cpac_engine::cached_host_info();
            let rc = cpac_engine::auto_resource_config();
            let uptime = start.elapsed().as_secs();
            let reqs = REQ_COUNT.load(Ordering::Relaxed);

            let body = format!(
                "# HELP cpac_info CPAC build information.\n\
                 # TYPE cpac_info gauge\n\
                 cpac_info{{version=\"{}\"}} 1\n\
                 # HELP cpac_uptime_seconds Server uptime in seconds.\n\
                 # TYPE cpac_uptime_seconds counter\n\
                 cpac_uptime_seconds {}\n\
                 # HELP cpac_requests_total Total HTTP requests served.\n\
                 # TYPE cpac_requests_total counter\n\
                 cpac_requests_total {}\n\
                 # HELP cpac_threads Available worker threads.\n\
                 # TYPE cpac_threads gauge\n\
                 cpac_threads {}\n\
                 # HELP cpac_memory_cap_mb Memory budget in MB.\n\
                 # TYPE cpac_memory_cap_mb gauge\n\
                 cpac_memory_cap_mb {}\n\
                 # HELP cpac_host_cores Physical CPU cores.\n\
                 # TYPE cpac_host_cores gauge\n\
                 cpac_host_cores {}\n",
                cpac_engine::VERSION,
                uptime,
                reqs,
                rc.max_threads,
                rc.max_memory_mb,
                host_info.physical_cores,
            );
            ("200 OK", "text/plain; version=0.0.4; charset=utf-8", body)
        } else if line.starts_with("GET /health") {
            let body = "{\"status\":\"ok\"}\n".to_string();
            ("200 OK", "application/json", body)
        } else {
            ("404 Not Found", "text/plain", "Not Found\n".to_string())
        };

        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
    }
}

// ---------------------------------------------------------------------------
// Phase 7A+7B: Batch compression with content-aware routing
// ---------------------------------------------------------------------------

/// Default routing table: maps file extension patterns to compression configs.
fn default_route(ext: &str) -> CompressConfig {
    let mut cfg = CompressConfig::default();
    match ext {
        // Text/structured → enable MSN + smart transforms
        "json" | "jsonl" | "ndjson" | "geojson" => {
            cfg.enable_msn = true;
            cfg.enable_smart_transforms = true;
        }
        "csv" | "tsv" | "parquet" => {
            cfg.enable_msn = true;
            cfg.enable_smart_transforms = true;
        }
        "xml" | "html" | "htm" | "svg" | "xhtml" => {
            cfg.enable_msn = true;
            cfg.enable_smart_transforms = true;
        }
        "yaml" | "yml" | "toml" | "ini" | "cfg" | "conf" | "tf" | "hcl" => {
            cfg.enable_msn = true;
            cfg.enable_smart_transforms = true;
        }
        // Logs → MSN + smart
        "log" | "syslog" => {
            cfg.enable_msn = true;
            cfg.enable_smart_transforms = true;
        }
        // Source code → smart transforms, brotli for ratio
        "py" | "rs" | "go" | "js" | "ts" | "java" | "c" | "cpp" | "h" | "rb" | "sh" => {
            cfg.enable_smart_transforms = true;
            cfg.backend = Some(Backend::Brotli);
        }
        // Already compressed → skip (raw passthrough)
        "gz" | "bz2" | "xz" | "zst" | "zip" | "rar" | "7z" | "lz4" => {
            cfg.backend = Some(Backend::Raw);
        }
        // Binary/media → fast zstd, no transforms
        "exe" | "dll" | "so" | "dylib" | "bin" | "o" => {
            cfg.backend = Some(Backend::Zstd);
            cfg.level = cpac_types::CompressionLevel::Fast;
        }
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "mp4" | "mp3" | "ogg" | "flac" => {
            cfg.backend = Some(Backend::Raw);
        }
        // Default: auto-detect via SSR
        _ => {}
    }
    cfg
}

/// Load a YAML routes file mapping extensions to overrides.
fn load_routes_file(
    path: &std::path::Path,
) -> Result<std::collections::HashMap<String, String>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read routes file '{}': {e}", path.display()))?;
    // Simple YAML: ext: preset_name (one per line)
    let mut map = std::collections::HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((ext, preset)) = line.split_once(':') {
            map.insert(ext.trim().to_string(), preset.trim().to_string());
        }
    }
    Ok(map)
}

#[allow(clippy::too_many_arguments)]
fn cmd_compress_batch(
    input: PathBuf,
    output_dir: Option<PathBuf>,
    force: bool,
    threads: usize,
    concurrency: usize,
    dict: Option<PathBuf>,
    routes: Option<PathBuf>,
    verbose: u8,
) {
    let resources = build_resources(threads, 0);

    // Load shared dictionary
    let dictionary: Option<Vec<u8>> = dict.map(|p| load_dict_bytes(&p, verbose));

    // Load custom routes (if provided)
    let custom_routes = routes.map(|p| {
        load_routes_file(&p).unwrap_or_else(|e| {
            eprintln!("Error: {e}");
            process::exit(1);
        })
    });

    // Collect files: if input is directory, scan recursively; else treat as manifest YAML.
    let files: Vec<PathBuf> = if input.is_dir() {
        let mut out = Vec::new();
        collect_files_recursive(&input, &mut out);
        out
    } else if input.extension().is_some_and(|e| e == "yaml" || e == "yml") {
        // Parse manifest: list of file paths (one per line for simplicity)
        let text = std::fs::read_to_string(&input).unwrap_or_else(|e| {
            eprintln!("Error reading manifest '{}': {e}", input.display());
            process::exit(1);
        });
        text.lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(PathBuf::from)
            .filter(|p| p.is_file())
            .collect()
    } else {
        eprintln!("Error: batch input must be a directory or YAML manifest");
        process::exit(1);
    };

    if files.is_empty() {
        eprintln!("No files found to compress.");
        return;
    }

    println!(
        "CPAC Batch Compress: {} files, {} concurrent",
        files.len(),
        concurrency
    );

    // Create output dir if needed
    if let Some(ref dir) = output_dir {
        std::fs::create_dir_all(dir).unwrap_or_else(|e| {
            eprintln!("Error creating output directory '{}': {e}", dir.display());
            process::exit(1);
        });
    }

    let pb = ProgressBar::new(files.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("█▓░"),
    );

    let mut total_orig: u64 = 0;
    let mut total_comp: u64 = 0;
    let mut errors: usize = 0;

    // Process files (sequential for now; concurrency reserved for future rayon file-level parallelism)
    let _ = concurrency; // Reserved for future use
    for file_path in &files {
        let ext = file_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        // Content-aware routing: custom routes > default route table
        let mut config = if let Some(ref routes_map) = custom_routes {
            if let Some(preset_name) = routes_map.get(&ext) {
                if let Some(preset) = Preset::from_str_loose(preset_name) {
                    CompressConfig::from_preset(preset)
                } else {
                    default_route(&ext)
                }
            } else {
                default_route(&ext)
            }
        } else {
            default_route(&ext)
        };

        config.resources = Some(resources.clone());
        config.dictionary = dictionary.clone();

        let data = std::fs::read(file_path).unwrap_or_else(|e| {
            eprintln!("Error reading '{}': {e}", file_path.display());
            errors += 1;
            Vec::new()
        });
        if data.is_empty() && errors > 0 {
            pb.inc(1);
            continue;
        }

        let orig_size = data.len();
        let result = cpac_engine::compress(&data, &config);

        match result {
            Ok(r) => {
                let out_path = if let Some(ref dir) = output_dir {
                    let name = file_path.file_name().unwrap_or_default();
                    let mut p = dir.join(name);
                    let mut s = p.as_os_str().to_owned();
                    s.push(CPAC_EXT);
                    p = PathBuf::from(s);
                    p
                } else {
                    let mut s = file_path.as_os_str().to_owned();
                    s.push(CPAC_EXT);
                    PathBuf::from(s)
                };

                write_output(&out_path, &r.data, force);
                total_orig += orig_size as u64;
                total_comp += r.data.len() as u64;

                if verbose >= 1 {
                    let ratio = orig_size as f64 / r.data.len().max(1) as f64;
                    println!(
                        "  {} -> {} [{ratio:.2}x] route={ext}",
                        file_path.display(),
                        out_path.display()
                    );
                }
            }
            Err(e) => {
                eprintln!("  Error compressing '{}': {e}", file_path.display());
                errors += 1;
            }
        }

        pb.set_message(format!(
            "{}",
            file_path.file_name().unwrap_or_default().to_string_lossy()
        ));
        pb.inc(1);
    }

    pb.finish_with_message("Done");

    let overall_ratio = if total_comp > 0 {
        total_orig as f64 / total_comp as f64
    } else {
        0.0
    };
    println!(
        "\nBatch complete: {} files, {} -> {} ({overall_ratio:.2}x), {errors} errors",
        files.len(),
        format_size(total_orig as usize),
        format_size(total_comp as usize),
    );
}

fn cmd_completions(shell: Shell) {
    let mut cmd = Cli::command();
    generate(shell, &mut cmd, "cpac", &mut io::stdout());
}

fn main() {
    // Load config (used for defaults; CLI flags override)
    let _config = config::CpacConfig::load();

    let cli = Cli::parse();

    match cli.command {
        Commands::Compress {
            input,
            output,
            backend,
            force,
            keep,
            recursive,
            verbose,
            threads,
            max_memory,
            mmap,
            enable_msn,
            msn_confidence,
            msn_domain,
            level,
            smart,
            preset,
            accel,
            dict,
            no_auto_dict,
            streaming,
            stream_block,
            encrypt,
            encrypt_key,
            encrypt_algo,
            transcode,
        } => cmd_compress(
            input,
            output,
            backend,
            force,
            keep,
            recursive,
            verbose,
            threads,
            max_memory,
            mmap,
            enable_msn,
            msn_confidence,
            msn_domain,
            parse_level(&level),
            smart,
            transcode,
            preset.and_then(|s| Preset::from_str_loose(&s)),
            AccelBackend::from_str_loose(&accel),
            dict,
            no_auto_dict,
            streaming,
            stream_block,
            encrypt,
            encrypt_key,
            encrypt_algo,
        ),
        Commands::Decompress {
            input,
            output,
            force,
            keep,
            verbose,
            threads,
            mmap,
            streaming,
            encrypt_key,
        } => cmd_decompress(
            input,
            output,
            force,
            keep,
            verbose,
            threads,
            mmap,
            streaming,
            encrypt_key,
        ),
        Commands::Info { input, host } => cmd_info(input, host),
        Commands::ListProfiles => cmd_list_profiles(),
        Commands::ListBackends => cmd_list_backends(),
        Commands::ListDomains => cmd_list_domains(),
        Commands::Benchmark {
            input,
            iterations,
            quick,
            full,
            skip_baselines,
            json,
            track1,
            discovery,
            backends,
            levels,
            baselines,
        } => cmd_benchmark(
            input,
            iterations,
            quick,
            full,
            skip_baselines,
            json,
            track1,
            discovery,
            backends,
            levels,
            baselines,
        ),
        Commands::Analyze { input } => cmd_analyze(input),
        Commands::Profile { input, quick } => cmd_profile(input, quick),
        Commands::AutoCas { input, compress } => cmd_auto_cas(input, compress),
        Commands::AutoAnalyze {
            input,
            output,
            quick,
            write_config,
        } => cmd_auto_analyze(input, output, quick, write_config),
        Commands::Encrypt {
            input,
            output,
            algorithm,
        } => cmd_encrypt(input, output, algorithm),
        Commands::Decrypt {
            input,
            output,
            algorithm,
        } => cmd_decrypt(input, output, algorithm),
        Commands::ArchiveCreate {
            input,
            output,
            solid,
        } => cmd_archive_create(input, output, solid),
        Commands::ArchiveExtract { input, output } => cmd_archive_extract(input, output),
        Commands::ArchiveList { input } => cmd_archive_list(input),
        Commands::Serve { listen, metrics } => cmd_serve(listen, metrics),
        Commands::CompressBatch {
            input,
            output,
            force,
            threads,
            concurrency,
            dict,
            routes,
            verbose,
        } => cmd_compress_batch(
            input,
            output,
            force,
            threads,
            concurrency,
            dict,
            routes,
            verbose,
        ),
        Commands::Pqc { action } => cmd_pqc(action),
        Commands::Lab { action } => cmd_lab(action),
        Commands::Completions { shell } => cmd_completions(shell),
    }
}
