// Quick benchmark runner
use cpac_engine::BenchmarkRunner;
use cpac_engine::bench::{BenchProfile, generate_markdown_report, generate_csv_export};
use std::path::Path;

fn main() {
    println!("=== CPAC Quick Benchmark ===\n");
    
    let corpus_dir = Path::new("bench-corpus");
    let runner = BenchmarkRunner::new(BenchProfile::Quick);
    
    let results = runner.bench_directory(corpus_dir, None);
    let summary = BenchmarkRunner::summarize("quick-bench", &results);
    
    // Display results
    println!("Total Original: {} bytes ({:.2} KB)", 
        summary.total_original, summary.total_original as f64 / 1024.0);
    println!("Total Compressed: {} bytes ({:.2} KB)", 
        summary.total_compressed, summary.total_compressed as f64 / 1024.0);
    println!("Overall Ratio: {:.2}x", summary.overall_ratio);
    println!("Mean Compress Throughput: {:.1} MB/s", summary.mean_compress_mbs);
    println!("Mean Decompress Throughput: {:.1} MB/s", summary.mean_decompress_mbs);
    println!("All Lossless: {}\n", summary.all_lossless);
    
    // Generate report
    let md = generate_markdown_report(&summary);
    std::fs::write("bench-results-quick.md", &md).unwrap();
    println!("Report saved to: bench-results-quick.md");
    
    // Also save CSV
    let csv = generate_csv_export(&results);
    std::fs::write("bench-results-quick.csv", &csv).unwrap();
    println!("CSV exported to: bench-results-quick.csv\n");
    
    println!("=== Balanced Benchmark ===\n");
    let runner_balanced = BenchmarkRunner::new(BenchProfile::Balanced);
    let results_balanced = runner_balanced.bench_directory(corpus_dir, None);
    let summary_balanced = BenchmarkRunner::summarize("balanced-bench", &results_balanced);
    
    println!("Total Original: {} bytes ({:.2} KB)", 
        summary_balanced.total_original, summary_balanced.total_original as f64 / 1024.0);
    println!("Total Compressed: {} bytes ({:.2} KB)", 
        summary_balanced.total_compressed, summary_balanced.total_compressed as f64 / 1024.0);
    println!("Overall Ratio: {:.2}x", summary_balanced.overall_ratio);
    println!("Mean Compress Throughput: {:.1} MB/s", summary_balanced.mean_compress_mbs);
    println!("Mean Decompress Throughput: {:.1} MB/s", summary_balanced.mean_decompress_mbs);
    println!("All Lossless: {}\n", summary_balanced.all_lossless);
    
    let md_balanced = generate_markdown_report(&summary_balanced);
    std::fs::write("bench-results-balanced.md", &md_balanced).unwrap();
    println!("Report saved to: bench-results-balanced.md");
    
    let csv_balanced = generate_csv_export(&results_balanced);
    std::fs::write("bench-results-balanced.csv", &csv_balanced).unwrap();
    println!("CSV exported to: bench-results-balanced.csv");
}
