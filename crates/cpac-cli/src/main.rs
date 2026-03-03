// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! CPAC CLI — command-line interface for compression/decompression.

mod config;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use cpac_types::{Backend, CompressConfig, ResourceConfig};
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
    /// Post-quantum cryptography operations.
    #[command(alias = "pq")]
    Pqc {
        #[command(subcommand)]
        action: PqcAction,
    },
    /// Generate shell completions.
    Completions {
        /// Shell to generate completions for.
        shell: Shell,
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
        "lzma" | "xz" => Ok(Backend::Lzma),
        other => Err(format!(
            "unknown backend: {other} (available: raw, zstd, brotli, gzip, lzma)"
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
) {
    let backend = backend.map(|b| match parse_backend(&b) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error: {e}");
            process::exit(1);
        }
    });

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
        let config = CompressConfig {
            backend,
            resources: Some(resources.clone()),
            ..Default::default()
        };

        // Decide whether to use mmap (flag or auto for files > 64 MB)
        let use_mmap = mmap
            || (file_path.to_str() != Some("-")
                && cpac_streaming::mmap::should_use_mmap(file_path));

        let result = if use_mmap && file_path.to_str() != Some("-") {
            cpac_streaming::mmap::mmap_compress(file_path, &config)
        } else {
            let data = read_input(file_path);
            // Auto-dispatch: use block-parallel for data > 256 KiB when threads > 1
            let use_parallel =
                data.len() >= cpac_engine::PARALLEL_THRESHOLD && resources.max_threads > 1;
            if use_parallel {
                cpac_engine::compress_parallel(
                    &data,
                    &config,
                    cpac_engine::DEFAULT_BLOCK_SIZE,
                    resources.max_threads,
                )
            } else {
                cpac_engine::compress(&data, &config)
            }
        };
        let result = match result {
            Ok(r) => r,
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
        };

        let out_path = if files.len() == 1 {
            output.clone().unwrap_or_else(|| {
                let mut p = file_path.as_os_str().to_owned();
                p.push(CPAC_EXT);
                PathBuf::from(p)
            })
        } else {
            let mut p = file_path.as_os_str().to_owned();
            p.push(CPAC_EXT);
            PathBuf::from(p)
        };

        write_output(&out_path, &result.data, force);

        if let Some(ref pb) = progress_bar {
            pb.set_message(format!(
                "{}",
                file_path.file_name().unwrap_or_default().to_string_lossy()
            ));
            pb.inc(1);
        }

        if verbose >= 2 {
            // -vv = detailed output
            let ratio = if result.compressed_size > 0 {
                result.original_size as f64 / result.compressed_size as f64
            } else {
                0.0
            };
            let savings = if result.original_size > 0 {
                (1.0 - result.compressed_size as f64 / result.original_size as f64) * 100.0
            } else {
                0.0
            };
            println!("Input:      {}", file_path.display());
            println!("Output:     {}", out_path.display());
            println!("Original:   {}", format_size(result.original_size));
            println!("Compressed: {}", format_size(result.compressed_size));
            println!("Ratio:      {ratio:.2}x ({savings:.1}% saved)");
            println!("Track:      {:?}", result.track);
            println!("Backend:    {:?}", result.backend);
            if verbose >= 3 {
                // -vvv = debug info
                println!("Threads:    {}", resources.max_threads);
                println!("Memory:     {} MB", resources.max_memory_mb);
                println!("MMap:       {}", use_mmap);
            }
            if files.len() > 1 {
                println!();
            }
        } else if verbose == 1 || progress_bar.is_none() {
            // -v = basic output (or no progress bar)
            let ratio = if result.compressed_size > 0 {
                result.original_size as f64 / result.compressed_size as f64
            } else {
                0.0
            };
            println!(
                "{} -> {} [{ratio:.2}x]",
                file_path.display(),
                out_path.display()
            );
        }
    }

    if let Some(pb) = progress_bar {
        pb.finish_with_message("Done");
    }
}

fn cmd_decompress(
    input: PathBuf,
    output: Option<PathBuf>,
    force: bool,
    _keep: bool,
    verbose: u8,
    threads: usize,
    mmap: bool,
) {
    let use_mmap =
        mmap || (input.to_str() != Some("-") && cpac_streaming::mmap::should_use_mmap(&input));

    let result = if use_mmap && input.to_str() != Some("-") {
        cpac_streaming::mmap::mmap_decompress(&input)
    } else {
        let resources = build_resources(threads, 0);
        let data = read_input(&input);
        if cpac_engine::is_cpbl(&data) {
            cpac_engine::decompress_parallel(&data, resources.max_threads)
        } else {
            cpac_engine::decompress(&data)
        }
    };
    let result = match result {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Decompression failed for '{}': {e}", input.display());
            eprintln!("Hint: Ensure the file is a valid CPAC archive and not corrupted.");
            eprintln!(
                "      Use 'cpac info {}' to inspect the file.",
                input.display()
            );
            process::exit(1);
        }
    };

    let out_path = output.unwrap_or_else(|| {
        let s = input.to_string_lossy();
        if let Some(stripped) = s.strip_suffix(CPAC_EXT) {
            PathBuf::from(stripped)
        } else {
            let mut p = input.as_os_str().to_owned();
            p.push(".out");
            PathBuf::from(p)
        }
    });

    write_output(&out_path, &result.data, force);

    if verbose >= 2 {
        // -vv = detailed output
        let input_size = if input.to_str() != Some("-") {
            input.metadata().map(|m| m.len() as usize).unwrap_or(0)
        } else {
            0
        };
        println!("Input:       {}", input.display());
        println!("Output:      {}", out_path.display());
        println!("Compressed:  {}", format_size(input_size));
        println!("Original:    {}", format_size(result.data.len()));
        if verbose >= 3 {
            // -vvv = debug info
            let resources = build_resources(threads, 0);
            println!("Threads:     {}", resources.max_threads);
            println!("MMap:        {}", use_mmap);
            println!("Block-level: {}", cpac_engine::is_cpbl(&read_input(&input)));
        }
    } else {
        // Default or -v: basic output
        println!(
            "{} -> {} [{}]",
            input.display(),
            out_path.display(),
            format_size(result.data.len())
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
        if input.is_none() {
            return;
        }
        println!();
    }

    let path = match input {
        Some(p) => p,
        None => {
            eprintln!("Error: provide an input file or use --host");
            process::exit(1);
        }
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
    names.sort();
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
    println!("  lzma      LZMA/xz compression (maximum ratio, slow)");
}

fn cmd_benchmark(
    input: PathBuf,
    iterations: Option<usize>,
    quick: bool,
    full: bool,
    skip_baselines: bool,
) {
    use cpac_engine::{BaselineEngine, BenchProfile, BenchmarkRunner};

    // Determine profile
    let profile = if quick {
        BenchProfile::Quick
    } else if full {
        BenchProfile::Full
    } else if iterations.is_some() {
        // Custom iterations via -n flag: use balanced as base
        BenchProfile::Balanced
    } else {
        // Default: balanced
        BenchProfile::Balanced
    };

    let mut runner = BenchmarkRunner::new(profile);
    if skip_baselines {
        runner.skip_baselines = true;
    }

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

    println!(
        "CPAC Benchmark ({} mode, {} iterations)",
        mode_label, actual_iterations
    );
    println!("File: {}\n", input.display());

    // Benchmark CPAC backends
    let mut all_results = Vec::new();
    for &backend in &runner.backends {
        match runner.bench_file(&input, backend) {
            Ok(result) => {
                println!(
                    "  {:12}  ratio: {:5.2}x  compress: {:6.1} MB/s  decompress: {:6.1} MB/s  verified: {}",
                    result.engine_label,
                    result.ratio,
                    result.compress_throughput_mbs,
                    result.decompress_throughput_mbs,
                    if result.lossless_verified { "YES" } else { "NO" }
                );
                all_results.push(result);
            }
            Err(e) => eprintln!("  {:12}  ERROR: {}", format!("{:?}", backend), e),
        }
    }

    // Benchmark baselines (if not skipped)
    if !runner.skip_baselines {
        println!();
        let baselines = if quick {
            &[BaselineEngine::Gzip9, BaselineEngine::Zstd3][..]
        } else {
            BaselineEngine::all()
        };
        for &engine in baselines {
            match runner.bench_baseline(&input, engine) {
                Ok(result) => {
                    println!(
                        "  {:12}  ratio: {:5.2}x  compress: {:6.1} MB/s  decompress: {:6.1} MB/s  verified: {}",
                        result.engine_label,
                        result.ratio,
                        result.compress_throughput_mbs,
                        result.decompress_throughput_mbs,
                        if result.lossless_verified { "YES" } else { "NO" }
                    );
                    all_results.push(result);
                }
                Err(e) => eprintln!("  {:12}  ERROR: {}", engine.label(), e),
            }
        }
    }

    // Summary
    if !all_results.is_empty() {
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
    let values: Vec<i64> = data.iter().map(|&b| b as i64).collect();
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
    let salt = &data[1..1 + salt_len];
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

fn cmd_archive_create(input: PathBuf, output: Option<PathBuf>) {
    if !input.is_dir() {
        eprintln!("Error: {} is not a directory", input.display());
        process::exit(1);
    }
    let config = CompressConfig::default();
    let archive_data = match cpac_archive::create_archive(&input, &config) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Archive creation error: {e}");
            process::exit(1);
        }
    };

    let out_path = output.unwrap_or_else(|| {
        let mut p = input.as_os_str().to_owned();
        p.push(".cpar");
        PathBuf::from(p)
    });

    write_output(&out_path, &archive_data, true);
    println!(
        "{} -> {} ({})",
        input.display(),
        out_path.display(),
        format_size(archive_data.len())
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
        } => cmd_compress(
            input, output, backend, force, keep, recursive, verbose, threads, max_memory, mmap,
        ),
        Commands::Decompress {
            input,
            output,
            force,
            keep,
            verbose,
            threads,
            mmap,
        } => cmd_decompress(input, output, force, keep, verbose, threads, mmap),
        Commands::Info { input, host } => cmd_info(input, host),
        Commands::ListProfiles => cmd_list_profiles(),
        Commands::ListBackends => cmd_list_backends(),
        Commands::Benchmark {
            input,
            iterations,
            quick,
            full,
            skip_baselines,
        } => cmd_benchmark(input, iterations, quick, full, skip_baselines),
        Commands::AutoCas { input, compress } => cmd_auto_cas(input, compress),
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
        Commands::ArchiveCreate { input, output } => cmd_archive_create(input, output),
        Commands::ArchiveExtract { input, output } => cmd_archive_extract(input, output),
        Commands::ArchiveList { input } => cmd_archive_list(input),
        Commands::Pqc { action } => cmd_pqc(action),
        Commands::Completions { shell } => cmd_completions(shell),
    }
}
