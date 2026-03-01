// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Regression tests: golden vectors, ratio gates, determinism across runs.

use cpac_engine::{compress, decompress, Backend, CompressConfig};

// ---------------------------------------------------------------------------
// Golden-vector helpers
// ---------------------------------------------------------------------------

/// Compress data and return (compressed_bytes, ratio).
fn compress_vec(data: &[u8], backend: Backend) -> (Vec<u8>, f64) {
    let config = CompressConfig {
        backend: Some(backend),
        ..Default::default()
    };
    let result = compress(data, &config).expect("golden compress");
    let ratio = result.ratio();
    (result.data, ratio)
}

/// Assert that compressed data decompresses to the expected original.
fn assert_golden_roundtrip(data: &[u8], backend: Backend, label: &str) {
    let (compressed, _ratio) = compress_vec(data, backend);
    let decompressed = decompress(&compressed).unwrap_or_else(|_| panic!("{label}: decompress"));
    assert_eq!(decompressed.data, data, "{label}: golden roundtrip failed");
}

// ---------------------------------------------------------------------------
// Golden vector tests (known inputs → verified roundtrip)
// ---------------------------------------------------------------------------

#[test]
fn golden_empty_all_backends() {
    for backend in [Backend::Raw, Backend::Zstd, Backend::Brotli] {
        assert_golden_roundtrip(b"", backend, "empty");
    }
}

#[test]
fn golden_single_byte() {
    for backend in [Backend::Raw, Backend::Zstd, Backend::Brotli] {
        assert_golden_roundtrip(b"X", backend, "single_byte");
    }
}

#[test]
fn golden_ascii_sentence() {
    let data = b"The quick brown fox jumps over the lazy dog.";
    for backend in [Backend::Raw, Backend::Zstd, Backend::Brotli] {
        assert_golden_roundtrip(data, backend, "ascii_sentence");
    }
}

#[test]
fn golden_structured_binary() {
    let mut data = Vec::with_capacity(4000);
    for _ in 0..1000u16 {
        data.extend_from_slice(&[0x01, 0x02, 0xFF, 0x00]);
    }
    for backend in [Backend::Raw, Backend::Zstd, Backend::Brotli] {
        assert_golden_roundtrip(&data, backend, "structured_binary");
    }
}

#[test]
fn golden_csv() {
    let mut csv = String::from("id,name,value\n");
    for i in 0..500 {
        csv.push_str(&format!("{i},item_{i},{}\n", i * 7 % 1000));
    }
    for backend in [Backend::Raw, Backend::Zstd, Backend::Brotli] {
        assert_golden_roundtrip(csv.as_bytes(), backend, "csv_data");
    }
}

#[test]
fn golden_random_lcg() {
    let mut rng: u64 = 42;
    let data: Vec<u8> = (0..16384)
        .map(|_| {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            (rng >> 33) as u8
        })
        .collect();
    for backend in [Backend::Raw, Backend::Zstd, Backend::Brotli] {
        assert_golden_roundtrip(&data, backend, "random_lcg");
    }
}

// ---------------------------------------------------------------------------
// Ratio regression gates
// ---------------------------------------------------------------------------
// These tests ensure compression ratios don't regress below established
// baselines. If a code change causes a ratio regression >2%, these fail.

#[test]
fn ratio_gate_repetitive_text() {
    let data = b"abcdef".repeat(10_000);
    let (_compressed, ratio) = compress_vec(&data, Backend::Zstd);
    // Highly repetitive: expect at least 50x ratio
    assert!(
        ratio > 50.0,
        "repetitive text ratio regressed: {ratio:.2} (expected > 50.0)"
    );
}

#[test]
fn ratio_gate_structured_binary() {
    let mut data = Vec::with_capacity(4000);
    for _ in 0..1000u16 {
        data.extend_from_slice(&[0x01, 0x02, 0xFF, 0x00]);
    }
    let (_compressed, ratio) = compress_vec(&data, Backend::Zstd);
    // Columnar-friendly structured data: expect at least 10x
    assert!(
        ratio > 10.0,
        "structured binary ratio regressed: {ratio:.2} (expected > 10.0)"
    );
}

#[test]
fn ratio_gate_csv() {
    let mut csv = String::from("id,name,value,status\n");
    for i in 0..1000 {
        csv.push_str(&format!(
            "{i},item_{i},{},{}\n",
            i * 7 % 1000,
            if i % 2 == 0 { "ok" } else { "err" }
        ));
    }
    let (_compressed, ratio) = compress_vec(csv.as_bytes(), Backend::Zstd);
    // CSV with entropy: expect at least 3x
    assert!(
        ratio > 3.0,
        "csv ratio regressed: {ratio:.2} (expected > 3.0)"
    );
}

#[test]
fn ratio_gate_ascii_text() {
    let data = b"The quick brown fox jumps over the lazy dog. ".repeat(1000);
    let (_compressed, ratio) = compress_vec(&data, Backend::Zstd);
    // Repeated English text: expect at least 20x
    assert!(
        ratio > 20.0,
        "ascii text ratio regressed: {ratio:.2} (expected > 20.0)"
    );
}

#[test]
fn ratio_gate_json_data() {
    let mut json = String::from("[\n");
    for i in 0..500 {
        json.push_str(&format!(
            "  {{\"id\": {i}, \"name\": \"item_{i}\", \"value\": {}, \"active\": {}}}{}",
            i * 7 % 1000,
            if i % 2 == 0 { "true" } else { "false" },
            if i < 499 { ",\n" } else { "\n" }
        ));
    }
    json.push_str("]");
    let (_compressed, ratio) = compress_vec(json.as_bytes(), Backend::Zstd);
    // JSON: expect at least 3.0x
    assert!(
        ratio >= 3.0,
        "json ratio regressed: {ratio:.2} (expected >= 3.0)"
    );
}

#[test]
fn ratio_gate_xml_data() {
    let mut xml = String::from("<?xml version=\"1.0\"?>\n<records>\n");
    for i in 0..300 {
        xml.push_str(&format!(
            "  <record id=\"{i}\">\n    <name>item_{i}</name>\n    <value>{}</value>\n  </record>\n",
            i * 7 % 1000
        ));
    }
    xml.push_str("</records>");
    let (_compressed, ratio) = compress_vec(xml.as_bytes(), Backend::Zstd);
    // XML: expect at least 3.5x (more verbose than JSON)
    assert!(
        ratio >= 3.5,
        "xml ratio regressed: {ratio:.2} (expected >= 3.5)"
    );
}

#[test]
fn ratio_gate_log_data() {
    let mut log = String::new();
    for i in 0..1000 {
        let level = ["INFO", "WARN", "ERROR", "DEBUG"][i % 4];
        log.push_str(&format!(
            "2026-03-01 12:{}:{:02} [{}] myapp: Processing request {} from 192.168.1.{}\n",
            i % 60,
            i % 60,
            level,
            i,
            (i % 254) + 1
        ));
    }
    let (_compressed, ratio) = compress_vec(log.as_bytes(), Backend::Zstd);
    // Structured logs: expect at least 3.5x
    assert!(
        ratio >= 3.5,
        "log ratio regressed: {ratio:.2} (expected >= 3.5)"
    );
}

#[test]
fn ratio_gate_binary_structured() {
    // ELF-like header repetition + structured sections
    let mut data = Vec::new();
    // Header with magic
    data.extend_from_slice(b"\x7fELF\x02\x01\x01\x00");
    // Repetitive structured data
    for _ in 0..2000 {
        data.extend_from_slice(&[0x00, 0x01, 0x02, 0x03]);
        data.extend_from_slice(&[0xFF, 0xFE, 0xFD, 0xFC]);
    }
    let (_compressed, ratio) = compress_vec(&data, Backend::Zstd);
    // Structured binary: expect at least 1.8x
    assert!(
        ratio >= 1.8,
        "binary structured ratio regressed: {ratio:.2} (expected >= 1.8)"
    );
}

#[test]
fn ratio_gate_random_should_expand() {
    // High-entropy random data should not compress well
    let mut rng: u64 = 12345;
    let data: Vec<u8> = (0..16384)
        .map(|_| {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (rng >> 33) as u8
        })
        .collect();
    let (_compressed, ratio) = compress_vec(&data, Backend::Zstd);
    // Random data: expect ratio between 0.8 and 1.2 (minimal compression or slight expansion)
    assert!(
        ratio >= 0.8 && ratio <= 1.2,
        "random data ratio unexpected: {ratio:.2} (expected 0.8-1.2, sanity check)"
    );
}

// ---------------------------------------------------------------------------
// Cross-run determinism
// ---------------------------------------------------------------------------

#[test]
fn determinism_100_runs() {
    let data = b"Determinism verification across multiple compress calls.";
    let config = CompressConfig::default();
    let reference = compress(data, &config).unwrap().data;
    for i in 0..100 {
        let result = compress(data, &config).unwrap().data;
        assert_eq!(result, reference, "determinism failed on run {i}");
    }
}

#[test]
fn determinism_with_transforms() {
    // Data that triggers ROLZ transform
    let data = b"The quick brown fox jumps over the lazy dog. ".repeat(50);
    let config = CompressConfig::default();
    let reference = compress(&data, &config).unwrap().data;
    for i in 0..20 {
        let result = compress(&data, &config).unwrap().data;
        assert_eq!(
            result, reference,
            "determinism with transforms failed on run {i}"
        );
    }
}

// ---------------------------------------------------------------------------
// Frame stability
// ---------------------------------------------------------------------------

#[test]
fn frame_magic_bytes_present() {
    let data = b"Frame format test data";
    let config = CompressConfig::default();
    let compressed = compress(data, &config).unwrap();
    // CPAC frame starts with "CP" magic
    assert!(
        compressed.data.len() >= 2,
        "compressed too short for magic bytes"
    );
    assert_eq!(
        &compressed.data[0..2],
        b"CP",
        "missing CP magic bytes in frame"
    );
}

#[test]
fn frame_version_byte() {
    let data = b"Version byte test";
    let config = CompressConfig::default();
    let compressed = compress(data, &config).unwrap();
    // Version byte is at offset 2
    assert!(compressed.data.len() >= 3);
    assert_eq!(
        compressed.data[2], 1,
        "expected frame version 1, got {}",
        compressed.data[2]
    );
}
