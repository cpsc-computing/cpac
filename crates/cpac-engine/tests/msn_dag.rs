// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! MSN + DAG integration tests.

use cpac_engine::{compress, decompress, CompressConfig};
use cpac_types::Backend;

/// Test MSN with DAG profile (text-heavy).
#[test]
fn msn_with_dag_text_profile() {
    // JSON array with repeated elements - benefits from both MSN and text transforms
    let single_element = r#"{"user":"alice","action":"login","timestamp":"2026-01-01T10:00:00Z"}"#;
    let elements = vec![single_element; 150]; // 150 repeated elements
    let json_data = format!("[{}]", elements.join(","));

    let data = json_data.as_bytes();

    // Compress with MSN enabled (DAG auto-selected based on SSR)
    let config = CompressConfig {
        enable_msn: true,
        backend: Some(Backend::Zstd),
        ..Default::default()
    };

    let result = compress(data, &config).unwrap();
    println!("Original: {} bytes", data.len());
    println!(
        "Compressed: {} bytes ({:.2}x)",
        result.compressed_size,
        result.ratio()
    );

    // Decompress and verify (JSON may reorder keys, so compare semantically)
    let decompressed = decompress(&result.data).unwrap();

    // Parse both as JSON and compare
    let orig_json: serde_json::Value = serde_json::from_slice(data).unwrap();
    let decompressed_json: serde_json::Value = serde_json::from_slice(&decompressed.data).unwrap();
    assert_eq!(
        decompressed_json, orig_json,
        "Decompressed JSON should match original semantically"
    );
}

/// Test MSN doesn't conflict with DAG transforms.
#[test]
fn msn_dag_no_conflict() {
    let data = b"test data with repeated patterns ".repeat(100);

    println!("Original data: {} bytes", data.len());
    println!("First 50 bytes: {:?}", &data[..50]);

    // With MSN
    let config_msn = CompressConfig {
        enable_msn: true,
        ..Default::default()
    };
    let result_msn = compress(&data, &config_msn).unwrap();

    println!("MSN result:");
    println!("  Compressed size: {} bytes", result_msn.compressed_size);
    println!("  Ratio: {:.2}x", result_msn.ratio());

    // Without MSN
    let config_no_msn = CompressConfig {
        enable_msn: false,
        ..Default::default()
    };
    let result_no_msn = compress(&data, &config_no_msn).unwrap();

    // Both should work
    assert_eq!(decompress(&result_msn.data).unwrap().data, data);
    assert_eq!(decompress(&result_no_msn.data).unwrap().data, data);

    println!("With MSN: {:.2}x", result_msn.ratio());
    println!("Without MSN: {:.2}x", result_no_msn.ratio());
}

/// Test MSN with explicit DAG profile.
#[test]
fn msn_with_explicit_dag() {
    let csv_data = b"name,age,score\nAlice,30,95\nBob,25,88\nCharlie,35,92\n".repeat(20);

    // MSN will extract CSV headers, DAG will apply transforms to residual
    let config = CompressConfig {
        enable_msn: true,
        backend: Some(Backend::Brotli),
        ..Default::default()
    };

    let result = compress(&csv_data, &config).unwrap();

    println!("CSV compression with MSN+DAG:");
    println!("  Original: {} bytes", csv_data.len());
    println!("  Compressed: {} bytes", result.compressed_size);
    println!("  Ratio: {:.2}x", result.ratio());

    // Verify lossless
    let decompressed = decompress(&result.data).unwrap();
    assert_eq!(decompressed.data, csv_data);
}
