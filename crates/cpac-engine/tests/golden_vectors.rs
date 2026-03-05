// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Golden vector tests - validate that stored .cpac files decompress correctly.

use cpac_engine::decompress;
use std::fs;
use std::path::Path;

const GOLDEN_DIR: &str = "tests/fixtures/golden";

/// Helper to load and decompress a golden vector file.
fn test_golden_decompress(filename: &str) {
    let path = Path::new(GOLDEN_DIR).join(filename);
    let compressed = fs::read(&path).unwrap_or_else(|_| panic!("Missing: {}", path.display()));
    let result = decompress(&compressed)
        .unwrap_or_else(|e| panic!("Decompress failed for {}: {}", filename, e));
    assert!(result.success, "Decompress marked failed for {}", filename);
    // Basic sanity: decompressed data exists
    assert!(
        !result.data.is_empty() || filename.contains("empty"),
        "Empty result for {}",
        filename
    );
}

// ---------------------------------------------------------------------------
// Backend variants
// ---------------------------------------------------------------------------

#[test]
fn golden_backend_zstd() {
    test_golden_decompress("backend_zstd.cpac");
}

#[test]
fn golden_backend_brotli() {
    test_golden_decompress("backend_brotli.cpac");
}

#[test]
fn golden_backend_raw() {
    test_golden_decompress("backend_raw.cpac");
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[test]
fn golden_edge_empty() {
    let path = Path::new(GOLDEN_DIR).join("edge_empty.cpac");
    let compressed = fs::read(&path).unwrap();
    let result = decompress(&compressed).unwrap();
    assert_eq!(
        result.data.len(),
        0,
        "Empty golden should decompress to zero bytes"
    );
}

#[test]
fn golden_edge_single_byte() {
    let path = Path::new(GOLDEN_DIR).join("edge_single_byte.cpac");
    let compressed = fs::read(&path).unwrap();
    let result = decompress(&compressed).unwrap();
    assert_eq!(
        result.data.len(),
        1,
        "Single byte golden should decompress to 1 byte"
    );
    assert_eq!(result.data[0], b'X', "Single byte should be 'X'");
}

#[test]
fn golden_edge_small() {
    test_golden_decompress("edge_small.cpac");
}

#[test]
fn golden_edge_medium() {
    let path = Path::new(GOLDEN_DIR).join("edge_medium.cpac");
    let compressed = fs::read(&path).unwrap();
    let result = decompress(&compressed).unwrap();
    assert_eq!(
        result.data.len(),
        4096,
        "Medium golden should be 4096 bytes"
    );
    // All bytes should be 0xAB
    assert!(
        result.data.iter().all(|&b| b == 0xAB),
        "Medium data should be all 0xAB"
    );
}

#[test]
fn golden_edge_large_repetitive() {
    let path = Path::new(GOLDEN_DIR).join("edge_large_repetitive.cpac");
    let compressed = fs::read(&path).unwrap();
    let result = decompress(&compressed).unwrap();
    assert_eq!(
        result.data.len(),
        80000,
        "Large repetitive should be 80000 bytes"
    );
    // Verify pattern
    for chunk in result.data.chunks(8) {
        if chunk.len() == 8 {
            assert_eq!(chunk, b"ABCDEFGH", "Large repetitive pattern mismatch");
        }
    }
}

// ---------------------------------------------------------------------------
// Data patterns
// ---------------------------------------------------------------------------

#[test]
fn golden_pattern_text_repetitive() {
    let path = Path::new(GOLDEN_DIR).join("pattern_text_repetitive.cpac");
    let compressed = fs::read(&path).unwrap();
    let result = decompress(&compressed).unwrap();
    // Should be 500 repetitions of 45-char line + newline = 22500 bytes
    assert_eq!(result.data.len(), 22500, "Text repetitive size mismatch");
    // First line check
    assert!(
        result.data.starts_with(b"The quick brown fox"),
        "Text pattern start mismatch"
    );
}

#[test]
fn golden_pattern_csv() {
    let path = Path::new(GOLDEN_DIR).join("pattern_csv.cpac");
    let compressed = fs::read(&path).unwrap();
    let result = decompress(&compressed).unwrap();
    let text = String::from_utf8_lossy(&result.data);
    // Should start with header
    assert!(
        text.starts_with("id,name,value,status\n"),
        "CSV header mismatch"
    );
    // Should have ~1000 rows
    let line_count = text.lines().count();
    assert!(
        line_count >= 1000,
        "CSV should have >= 1000 lines, got {}",
        line_count
    );
}

#[test]
fn golden_pattern_json() {
    let path = Path::new(GOLDEN_DIR).join("pattern_json.cpac");
    let compressed = fs::read(&path).unwrap();
    let result = decompress(&compressed).unwrap();
    let text = String::from_utf8_lossy(&result.data);
    // Should be valid JSON array
    assert!(text.starts_with('['), "JSON should start with '['");
    assert!(text.ends_with(']'), "JSON should end with ']'");
    assert!(
        text.contains(r#""id":0"#),
        "JSON should contain first object"
    );
}

#[test]
fn golden_pattern_binary_structured() {
    let path = Path::new(GOLDEN_DIR).join("pattern_binary_structured.cpac");
    let compressed = fs::read(&path).unwrap();
    let result = decompress(&compressed).unwrap();
    assert_eq!(
        result.data.len(),
        8000,
        "Binary structured should be 8000 bytes"
    );
    // Verify pattern: [01 02 03 04 FF FE FD FC] repeated
    for (i, chunk) in result.data.chunks(8).enumerate() {
        if chunk.len() == 8 {
            assert_eq!(
                chunk,
                &[0x01, 0x02, 0x03, 0x04, 0xFF, 0xFE, 0xFD, 0xFC],
                "Binary pattern mismatch at chunk {}",
                i
            );
        }
    }
}

#[test]
fn golden_pattern_random() {
    test_golden_decompress("pattern_random.cpac");
    let path = Path::new(GOLDEN_DIR).join("pattern_random.cpac");
    let compressed = fs::read(&path).unwrap();
    let result = decompress(&compressed).unwrap();
    assert_eq!(
        result.data.len(),
        8192,
        "Random pattern should be 8192 bytes"
    );
}

// ---------------------------------------------------------------------------
// Batch test: verify all golden files exist and decompress
// ---------------------------------------------------------------------------

#[test]
fn all_golden_vectors_present() {
    let expected_files = [
        "backend_zstd.cpac",
        "backend_brotli.cpac",
        "backend_raw.cpac",
        "edge_empty.cpac",
        "edge_single_byte.cpac",
        "edge_small.cpac",
        "edge_medium.cpac",
        "edge_large_repetitive.cpac",
        "pattern_text_repetitive.cpac",
        "pattern_csv.cpac",
        "pattern_json.cpac",
        "pattern_binary_structured.cpac",
        "pattern_random.cpac",
        "README.md",
    ];

    for filename in &expected_files {
        let path = Path::new(GOLDEN_DIR).join(filename);
        assert!(path.exists(), "Missing golden file: {}", filename);
    }
}

#[test]
fn all_golden_vectors_decompress() {
    let cpac_files = [
        "backend_zstd.cpac",
        "backend_brotli.cpac",
        "backend_raw.cpac",
        "edge_empty.cpac",
        "edge_single_byte.cpac",
        "edge_small.cpac",
        "edge_medium.cpac",
        "edge_large_repetitive.cpac",
        "pattern_text_repetitive.cpac",
        "pattern_csv.cpac",
        "pattern_json.cpac",
        "pattern_binary_structured.cpac",
        "pattern_random.cpac",
    ];

    for filename in &cpac_files {
        test_golden_decompress(filename);
    }
}
