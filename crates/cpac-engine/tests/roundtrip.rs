// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Integration tests: roundtrip compress → decompress for various inputs.

use cpac_engine::{compress, decompress, Backend, CompressConfig};

fn roundtrip(data: &[u8], label: &str) {
    let config = CompressConfig::default();
    let compressed =
        compress(data, &config).unwrap_or_else(|e| panic!("{label}: compress failed: {e}"));
    let decompressed =
        decompress(&compressed.data).unwrap_or_else(|e| panic!("{label}: decompress failed: {e}"));
    assert!(decompressed.success, "{label}: success flag is false");
    assert_eq!(
        decompressed.data,
        data,
        "{label}: data mismatch (len {} vs {})",
        decompressed.data.len(),
        data.len()
    );
}

fn roundtrip_backend(data: &[u8], backend: Backend, label: &str) {
    let config = CompressConfig {
        backend: Some(backend),
        ..Default::default()
    };
    let compressed = compress(data, &config)
        .unwrap_or_else(|e| panic!("{label}/{backend:?}: compress failed: {e}"));
    assert_eq!(compressed.backend, backend);
    let decompressed = decompress(&compressed.data)
        .unwrap_or_else(|e| panic!("{label}/{backend:?}: decompress failed: {e}"));
    assert_eq!(
        decompressed.data, data,
        "{label}/{backend:?}: data mismatch"
    );
}

// --- Test vectors ---

#[test]
fn empty() {
    roundtrip(b"", "empty");
}

#[test]
fn single_byte() {
    roundtrip(b"X", "single_byte");
}

#[test]
fn ascii_text() {
    roundtrip(
        b"The quick brown fox jumps over the lazy dog.",
        "ascii_text",
    );
}

#[test]
fn repetitive_short() {
    let data = b"aaaa".repeat(100);
    roundtrip(&data, "repetitive_short");
}

#[test]
fn repetitive_large() {
    let data = b"abcdefghij\n".repeat(10_000);
    roundtrip(&data, "repetitive_large");
}

#[test]
fn binary_all_bytes() {
    let data: Vec<u8> = (0u8..=255).collect();
    roundtrip(&data, "binary_all_bytes");
}

#[test]
fn binary_cycling() {
    let data: Vec<u8> = (0u8..=255).cycle().take(4096).collect();
    roundtrip(&data, "binary_cycling");
}

#[test]
fn pseudo_random() {
    // LCG pseudo-random (deterministic)
    let mut rng: u64 = 12345;
    let data: Vec<u8> = (0..8192)
        .map(|_| {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            (rng >> 33) as u8
        })
        .collect();
    roundtrip(&data, "pseudo_random");
}

#[test]
fn one_megabyte() {
    let data: Vec<u8> = b"CPAC compression test data block. ".repeat(32768); // ~1 MB
    roundtrip(&data, "one_megabyte");
}

#[test]
fn json_like() {
    let data = br#"{"users":[{"name":"Alice","age":30},{"name":"Bob","age":25}]}"#;
    roundtrip(data, "json_like");
}

#[test]
fn csv_like() {
    let data = b"name,age,city\nAlice,30,NYC\nBob,25,LA\nCharlie,35,CHI\n";
    roundtrip(data, "csv_like");
}

// --- Backend-specific tests ---

#[test]
fn all_backends_ascii() {
    let data = b"Testing all backends with ASCII data for CPAC.";
    for backend in [Backend::Raw, Backend::Zstd, Backend::Brotli] {
        roundtrip_backend(data, backend, "all_backends_ascii");
    }
}

#[test]
fn all_backends_binary() {
    let data: Vec<u8> = (0u8..=255).cycle().take(2048).collect();
    for backend in [Backend::Raw, Backend::Zstd, Backend::Brotli] {
        roundtrip_backend(&data, backend, "all_backends_binary");
    }
}

// --- Phase 2: Transform integration tests ---

#[test]
fn structured_binary_transpose() {
    // Fixed-width 4-byte records with columnar patterns.
    // Should trigger transpose transform in preprocess.
    let mut data = Vec::with_capacity(800);
    for _ in 0..200u16 {
        data.push(0x01); // constant col 0
        data.push(0x02); // constant col 1
        data.push(0xFF); // constant col 2
        data.push(0x00); // constant col 3
    }
    roundtrip(&data, "structured_binary_transpose");
}

#[test]
fn structured_binary_with_counter() {
    // Fixed-width records with one varying column.
    let mut data = Vec::with_capacity(800);
    for i in 0..200u16 {
        data.push(0x01);
        data.push((i & 0xFF) as u8);
        data.push(0xFF);
        data.push(0x00);
    }
    roundtrip(&data, "structured_binary_counter");
}

#[test]
fn repetitive_text_rolz() {
    // Medium-entropy repetitive text — should trigger ROLZ transform.
    let data = b"The quick brown fox jumps over the lazy dog. ".repeat(50);
    roundtrip(&data, "repetitive_text_rolz");
}

#[test]
fn log_like_text_rolz() {
    // Log-style data with repeating patterns.
    let mut data = Vec::new();
    for i in 0..100 {
        data.extend_from_slice(
            format!("2026-03-01T12:00:{i:02} INFO  [worker-{i}] Processing request id={i}\n")
                .as_bytes(),
        );
    }
    roundtrip(&data, "log_like_text_rolz");
}

#[test]
fn transform_ratio_improvement() {
    // Structured binary: compression with transforms should achieve better
    // ratio than the data size alone.
    let mut data = Vec::with_capacity(4000);
    for _ in 0..1000u16 {
        data.push(0x01);
        data.push(0x02);
        data.push(0xFF);
        data.push(0x00);
    }
    let config = CompressConfig::default();
    let compressed = compress(&data, &config).unwrap();
    // Highly structured data should compress extremely well
    assert!(
        compressed.ratio() > 10.0,
        "expected ratio > 10 for structured data, got {}",
        compressed.ratio()
    );
    let decompressed = decompress(&compressed.data).unwrap();
    assert_eq!(decompressed.data, data);
}

// --- Determinism test ---

#[test]
fn deterministic_output() {
    let data = b"Determinism test: same input should produce same output.";
    let config = CompressConfig::default();
    let r1 = compress(data, &config).unwrap();
    let r2 = compress(data, &config).unwrap();
    assert_eq!(r1.data, r2.data, "non-deterministic compression output");
}

#[test]
fn deterministic_with_transforms() {
    // Structured data that triggers transforms should still be deterministic.
    let mut data = Vec::with_capacity(800);
    for _ in 0..200u16 {
        data.push(0x01);
        data.push(0x02);
        data.push(0xFF);
        data.push(0x00);
    }
    let config = CompressConfig::default();
    let r1 = compress(&data, &config).unwrap();
    let r2 = compress(&data, &config).unwrap();
    assert_eq!(r1.data, r2.data, "non-deterministic with transforms");
}
