// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Transform Study CLI — YAML-driven experiment runner for benchmarking
//! individual transforms and chains against a corpus.

use clap::Parser;
use cpac_dag::registry::TransformRegistry;
use cpac_lab::collector::{collect_files, CollectOptions};
use cpac_lab::experiment::{load_experiment, validate_experiment};
use cpac_lab::report;
use cpac_lab::runner::run_experiment;
use std::io::Write;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "transform_study",
    about = "YAML-driven transform benchmarking against a corpus"
)]
struct Args {
    /// Path to the YAML experiment file.
    #[arg(short = 'e', long)]
    experiment: PathBuf,

    /// Corpus directory (overrides the YAML `corpus` field).
    #[arg(short = 'c', long)]
    corpus: Option<PathBuf>,

    /// Output CSV instead of text tables.
    #[arg(long)]
    csv: bool,

    /// Suppress per-file output; show only summary.
    #[arg(long)]
    quiet: bool,

    /// Process all file types (not just text extensions).
    #[arg(long)]
    all_types: bool,
}

fn main() {
    let args = Args::parse();

    // Load and validate experiment
    let mut experiment = load_experiment(&args.experiment).unwrap_or_else(|e| {
        eprintln!("Error loading experiment: {e}");
        std::process::exit(1);
    });

    if let Err(e) = validate_experiment(&experiment) {
        eprintln!("Invalid experiment: {e}");
        std::process::exit(1);
    }

    // CLI overrides
    let corpus_dir = args
        .corpus
        .or(experiment.corpus.take())
        .unwrap_or_else(|| {
            eprintln!("No corpus directory specified (use --corpus or set in YAML)");
            std::process::exit(1);
        });

    if !corpus_dir.is_dir() {
        eprintln!("Corpus directory not found: {}", corpus_dir.display());
        std::process::exit(1);
    }

    // Collect files
    let opts = CollectOptions {
        extensions: experiment.extensions.clone(),
        exclude_extensions: experiment.exclude_extensions.clone(),
        max_file_size: experiment.max_file_size,
        recursive: true,
        all_types: args.all_types,
    };

    let files = collect_files(&corpus_dir, &opts);
    if files.is_empty() {
        eprintln!("No eligible files found in {}", corpus_dir.display());
        std::process::exit(1);
    }

    // Header
    if !args.csv {
        println!("TRANSFORM STUDY: {}", experiment.name);
        if !experiment.description.is_empty() {
            println!("  {}", experiment.description);
        }
        println!("Corpus:    {}", corpus_dir.display());
        println!("zstd:      level {}", experiment.zstd_level);
        println!("Files:     {}", files.len());
        println!(
            "Trials:    {}",
            experiment
                .trials
                .iter()
                .map(|t| t.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        println!();
    } else {
        report::print_csv_header();
    }

    // Build transform registry
    let registry = TransformRegistry::with_builtins();

    // Run experiment
    let results = run_experiment(&experiment, &files, &registry, |i, total, name| {
        if !args.csv {
            let disp_end = name
                .char_indices()
                .nth(55)
                .map(|(idx, _)| idx)
                .unwrap_or(name.len());
            eprint!("\r[{:>5}/{:<5}] {:<55}", i + 1, total, &name[..disp_end]);
            let _ = std::io::stderr().flush();
        }
    });

    // Clear progress
    if !args.csv {
        eprint!("\r{:<80}\r", "");
        let _ = std::io::stderr().flush();
    }

    // Output
    if args.csv {
        for r in &results {
            report::print_csv_row(r);
        }
    } else if !args.quiet {
        // Per-file detail (abbreviated)
        println!(
            "{:<34}  {:<16}  {:>8}  {:>8}  {:>+9}",
            "File", "Trial", "z_base", "z_after", "net_gain"
        );
        println!("{}", "─".repeat(80));
        for r in &results {
            let disp = if r.file_name.chars().count() > 33 {
                let start = r
                    .file_name
                    .char_indices()
                    .rev()
                    .nth(31)
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                format!("\u{2026}{}", &r.file_name[start..])
            } else {
                r.file_name.clone()
            };
            println!(
                "{:<34}  {:<16}  {:>8}  {:>8}  {:>+9}",
                disp, r.trial_name, r.z_baseline, r.z_after, r.net_gain
            );
        }
    }

    // Summary
    if !args.csv {
        report::print_summary(&results, &experiment.group_by);
    }
}
