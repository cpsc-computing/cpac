// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Real-world MSN tests using corpus data.

use cpac_engine::{compress, decompress, CompressConfig};
use std::path::PathBuf;
use std::fs;

fn corpus_path(filename: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(".work/benchmarks/bench-corpus")
        .join(filename)
}

/// Test MSN on real JSON data from corpus.
#[test]
fn corpus_json() {
    let path = corpus_path("data.json");
    if !path.exists() {
        eprintln!("Skipping test: corpus file not found: {:?}", path);
        return;
    }
    
    let data = fs::read(&path).unwrap();
    
    // Without MSN
    let config_no_msn = CompressConfig {
        enable_msn: false,
        ..Default::default()
    };
    let result_no_msn = compress(&data, &config_no_msn).unwrap();
    
    // With MSN
    let config_msn = CompressConfig {
        enable_msn: true,
        ..Default::default()
    };
    let result_msn = compress(&data, &config_msn).unwrap();
    
    println!("Corpus data.json ({} bytes):", data.len());
    println!("  Without MSN: {} bytes ({:.2}x)", result_no_msn.compressed_size, result_no_msn.ratio());
    println!("  With MSN: {} bytes ({:.2}x)", result_msn.compressed_size, result_msn.ratio());
    println!("  MSN delta: {:.1}%", 
        (result_no_msn.compressed_size as f64 / result_msn.compressed_size as f64 - 1.0) * 100.0);
    
    // Verify lossless roundtrip
    let decompressed = decompress(&result_msn.data).unwrap();
    assert_eq!(decompressed.data, data);
}

/// Test MSN on large JSON data from corpus.
#[test]
fn corpus_large_json() {
    let path = corpus_path("large-data.json");
    if !path.exists() {
        eprintln!("Skipping test: corpus file not found: {:?}", path);
        return;
    }
    
    let data = fs::read(&path).unwrap();
    
    // Without MSN
    let config_no_msn = CompressConfig {
        enable_msn: false,
        ..Default::default()
    };
    let result_no_msn = compress(&data, &config_no_msn).unwrap();
    
    // With MSN
    let config_msn = CompressConfig {
        enable_msn: true,
        ..Default::default()
    };
    let result_msn = compress(&data, &config_msn).unwrap();
    
    println!("Corpus large-data.json ({} bytes):", data.len());
    println!("  Without MSN: {} bytes ({:.2}x)", result_no_msn.compressed_size, result_no_msn.ratio());
    println!("  With MSN: {} bytes ({:.2}x)", result_msn.compressed_size, result_msn.ratio());
    println!("  MSN delta: {:.1}%", 
        (result_no_msn.compressed_size as f64 / result_msn.compressed_size as f64 - 1.0) * 100.0);
    
    // Verify lossless roundtrip
    let decompressed = decompress(&result_msn.data).unwrap();
    assert_eq!(decompressed.data, data);
}

/// Test MSN on CSV data from corpus.
#[test]
fn corpus_csv() {
    let path = corpus_path("metrics.csv");
    if !path.exists() {
        eprintln!("Skipping test: corpus file not found: {:?}", path);
        return;
    }
    
    let data = fs::read(&path).unwrap();
    
    // Without MSN
    let config_no_msn = CompressConfig {
        enable_msn: false,
        ..Default::default()
    };
    let result_no_msn = compress(&data, &config_no_msn).unwrap();
    
    // With MSN
    let config_msn = CompressConfig {
        enable_msn: true,
        ..Default::default()
    };
    let result_msn = compress(&data, &config_msn).unwrap();
    
    println!("Corpus metrics.csv ({} bytes):", data.len());
    println!("  Without MSN: {} bytes ({:.2}x)", result_no_msn.compressed_size, result_no_msn.ratio());
    println!("  With MSN: {} bytes ({:.2}x)", result_msn.compressed_size, result_msn.ratio());
    println!("  MSN delta: {:.1}%", 
        (result_no_msn.compressed_size as f64 / result_msn.compressed_size as f64 - 1.0) * 100.0);
    
    // Verify lossless roundtrip
    let decompressed = decompress(&result_msn.data).unwrap();
    assert_eq!(decompressed.data, data);
}

/// Test MSN on large CSV data from corpus.
#[test]
fn corpus_large_csv() {
    let path = corpus_path("large-metrics.csv");
    if !path.exists() {
        eprintln!("Skipping test: corpus file not found: {:?}", path);
        return;
    }
    
    let data = fs::read(&path).unwrap();
    
    // Without MSN
    let config_no_msn = CompressConfig {
        enable_msn: false,
        ..Default::default()
    };
    let result_no_msn = compress(&data, &config_no_msn).unwrap();
    
    // With MSN
    let config_msn = CompressConfig {
        enable_msn: true,
        ..Default::default()
    };
    let result_msn = compress(&data, &config_msn).unwrap();
    
    println!("Corpus large-metrics.csv ({} bytes):", data.len());
    println!("  Without MSN: {} bytes ({:.2}x)", result_no_msn.compressed_size, result_no_msn.ratio());
    println!("  With MSN: {} bytes ({:.2}x)", result_msn.compressed_size, result_msn.ratio());
    println!("  MSN delta: {:.1}%", 
        (result_no_msn.compressed_size as f64 / result_msn.compressed_size as f64 - 1.0) * 100.0);
    
    // Verify lossless roundtrip
    let decompressed = decompress(&result_msn.data).unwrap();
    assert_eq!(decompressed.data, data);
}

/// Test MSN on server log data from corpus.
#[test]
fn corpus_server_log() {
    let path = corpus_path("server.log");
    if !path.exists() {
        eprintln!("Skipping test: corpus file not found: {:?}", path);
        return;
    }
    
    let data = fs::read(&path).unwrap();
    
    // Without MSN
    let config_no_msn = CompressConfig {
        enable_msn: false,
        ..Default::default()
    };
    let result_no_msn = compress(&data, &config_no_msn).unwrap();
    
    // With MSN
    let config_msn = CompressConfig {
        enable_msn: true,
        ..Default::default()
    };
    let result_msn = compress(&data, &config_msn).unwrap();
    
    println!("Corpus server.log ({} bytes):", data.len());
    println!("  Without MSN: {} bytes ({:.2}x)", result_no_msn.compressed_size, result_no_msn.ratio());
    println!("  With MSN: {} bytes ({:.2}x)", result_msn.compressed_size, result_msn.ratio());
    println!("  MSN delta: {:.1}%", 
        (result_no_msn.compressed_size as f64 / result_msn.compressed_size as f64 - 1.0) * 100.0);
    
    // Verify lossless roundtrip
    let decompressed = decompress(&result_msn.data).unwrap();
    assert_eq!(decompressed.data, data);
}

/// Test MSN on large server log data from corpus.
#[test]
fn corpus_large_server_log() {
    let path = corpus_path("large-server.log");
    if !path.exists() {
        eprintln!("Skipping test: corpus file not found: {:?}", path);
        return;
    }
    
    let data = fs::read(&path).unwrap();
    
    // Without MSN
    let config_no_msn = CompressConfig {
        enable_msn: false,
        ..Default::default()
    };
    let result_no_msn = compress(&data, &config_no_msn).unwrap();
    
    // With MSN
    let config_msn = CompressConfig {
        enable_msn: true,
        ..Default::default()
    };
    let result_msn = compress(&data, &config_msn).unwrap();
    
    println!("Corpus large-server.log ({} bytes):", data.len());
    println!("  Without MSN: {} bytes ({:.2}x)", result_no_msn.compressed_size, result_no_msn.ratio());
    println!("  With MSN: {} bytes ({:.2}x)", result_msn.compressed_size, result_msn.ratio());
    println!("  MSN delta: {:.1}%", 
        (result_no_msn.compressed_size as f64 / result_msn.compressed_size as f64 - 1.0) * 100.0);
    
    // Verify lossless roundtrip
    let decompressed = decompress(&result_msn.data).unwrap();
    assert_eq!(decompressed.data, data);
}
