// CPAC Benchmark Runner - Quick and Full profiles
use cpac_engine::BenchmarkRunner;
use cpac_engine::bench::{BenchProfile, generate_markdown_report, generate_csv_export};
use std::path::Path;
use std::env;

fn run_benchmark(profile: BenchProfile, profile_name: &str) {
    println!("=== CPAC {} Benchmark ===\n", profile_name);
    println!("Iterations: {}\n", profile.iterations());
    
    let corpus_dir = Path::new("bench-corpus");
    let runner = BenchmarkRunner::new(profile);
    
    let start = std::time::Instant::now();
    let results = runner.bench_directory(corpus_dir, None);
    let elapsed = start.elapsed();
    
    let summary = BenchmarkRunner::summarize(&format!("{}-bench", profile_name.to_lowercase()), &results);
    
    // Display results
    println!("Total Original: {} bytes ({:.2} MB)", 
        summary.total_original, summary.total_original as f64 / 1_048_576.0);
    println!("Total Compressed: {} bytes ({:.2} MB)", 
        summary.total_compressed, summary.total_compressed as f64 / 1_048_576.0);
    println!("Overall Ratio: {:.2}x", summary.overall_ratio);
    println!("Mean Compress Throughput: {:.1} MB/s", summary.mean_compress_mbs);
    println!("Mean Decompress Throughput: {:.1} MB/s", summary.mean_decompress_mbs);
    println!("Peak Memory: {:.1} MB", summary.total_peak_memory_bytes as f64 / 1_048_576.0);
    println!("All Lossless: {}", summary.all_lossless);
    println!("Benchmark Duration: {:.2}s\n", elapsed.as_secs_f64());
    
    // Generate report
    let filename_base = format!("bench-results-{}", profile_name.to_lowercase());
    let md = generate_markdown_report(&summary);
    std::fs::write(format!("{}.md", filename_base), &md).unwrap();
    println!("Report saved to: {}.md", filename_base);
    
    // Also save CSV
    let csv = generate_csv_export(&results);
    std::fs::write(format!("{}.csv", filename_base), &csv).unwrap();
    println!("CSV exported to: {}.csv\n", filename_base);
}

fn main() {
    let args: Vec<String> = env::args().collect();
    
    if args.len() > 1 && args[1] == "full" {
        // Run only Full benchmark
        run_benchmark(BenchProfile::Full, "Full");
    } else if args.len() > 1 && args[1] == "quick" {
        // Run only Quick benchmark
        run_benchmark(BenchProfile::Quick, "Quick");
    } else {
        // Run both by default
        run_benchmark(BenchProfile::Quick, "Quick");
        println!("\n{}\n", "=".repeat(60));
        run_benchmark(BenchProfile::Full, "Full");
    }
}
