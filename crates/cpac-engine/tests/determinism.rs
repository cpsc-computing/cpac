// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Cross-backend determinism and edge case tests.

use cpac_engine::{compress, decompress};
use cpac_types::{Backend, CompressConfig};

#[test]
fn test_determinism_same_input_same_output() {
    let data = b"Hello, CPAC! This is a determinism test.";
    let config = CompressConfig::default();

    let result1 = compress(data, &config).unwrap();
    let result2 = compress(data, &config).unwrap();

    assert_eq!(
        result1.data, result2.data,
        "Same input must produce identical output"
    );
}

#[test]
fn test_determinism_across_thread_counts() {
    use cpac_engine::{compress_parallel, decompress_parallel};

    let data = vec![0xAA; 512 * 1024]; // 512 KB
    let config = CompressConfig::default();

    let result_1thread = compress_parallel(&data, &config, 256 * 1024, 1).unwrap();
    let result_4thread = compress_parallel(&data, &config, 256 * 1024, 4).unwrap();

    // Decompress both using parallel decompressor (CPBL format)
    let dec1 = decompress_parallel(&result_1thread.data, 1).unwrap();
    let dec4 = decompress_parallel(&result_4thread.data, 4).unwrap();

    assert_eq!(dec1.data, data);
    assert_eq!(dec4.data, data);
}

#[test]
fn test_edge_case_empty_file() {
    let empty: &[u8] = b"";
    let config = CompressConfig::default();

    let result = compress(empty, &config).unwrap();
    assert_eq!(result.original_size, 0);

    let decompressed = decompress(&result.data).unwrap();
    assert_eq!(decompressed.data.len(), 0);
}

#[test]
fn test_edge_case_one_byte() {
    let data: &[u8] = b"X";
    let config = CompressConfig::default();

    let result = compress(data, &config).unwrap();
    assert_eq!(result.original_size, 1);

    let decompressed = decompress(&result.data).unwrap();
    assert_eq!(decompressed.data, data);
}

#[test]
fn test_edge_case_all_zeros() {
    let data = vec![0u8; 1024];
    let config = CompressConfig {
        backend: Some(Backend::Zstd),
        ..Default::default()
    };

    let result = compress(&data, &config).unwrap();
    assert!(
        result.ratio() > 10.0,
        "All-zeros should compress extremely well"
    );

    let decompressed = decompress(&result.data).unwrap();
    assert_eq!(decompressed.data, data);
}

#[test]
fn test_edge_case_random_data() {
    // Pseudo-random (incompressible)
    let data: Vec<u8> = (0..1024)
        .map(|i| ((i * 214013 + 2531011) >> 16) as u8)
        .collect();
    let config = CompressConfig {
        backend: Some(Backend::Raw),
        ..Default::default()
    };

    let result = compress(&data, &config).unwrap();
    // Raw backend should not compress
    assert!(
        result.ratio() < 1.2,
        "Random data with Raw should not expand much"
    );

    let decompressed = decompress(&result.data).unwrap();
    assert_eq!(decompressed.data, data);
}
