// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! End-to-end integration tests for CPAC.
//!
//! These tests exercise complete pipelines:
//! - Engine compress → decompress for all MSN-detectable domain types
//! - Streaming compress → decompress with MSN enabled
//! - Regression baseline save/load/check roundtrip
//! - Metadata compactness verification (MessagePack < JSON)
//! - Archive create → extract

use cpac_engine::{compress, decompress, CompressConfig};
use cpac_types::{Backend, CpacError};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn compress_decompress_roundtrip(data: &[u8], enable_msn: bool) {
    let config = CompressConfig {
        enable_msn,
        msn_confidence: 0.7,
        backend: Some(Backend::Zstd),
        ..Default::default()
    };
    let compressed = compress(data, &config).unwrap_or_else(|e| panic!("compress failed: {e}"));
    let decompressed =
        decompress(&compressed.data).unwrap_or_else(|e| panic!("decompress failed: {e}"));
    assert_eq!(
        decompressed.data, data,
        "roundtrip mismatch (msn={})",
        enable_msn
    );
}

// ---------------------------------------------------------------------------
// Engine roundtrips for all major data types
// ---------------------------------------------------------------------------

#[test]
fn integration_json_roundtrip_with_msn() {
    let json_data = r#"{"name":"Alice","age":30,"city":"NYC"}
{"name":"Bob","age":25,"city":"SF"}
{"name":"Charlie","age":35,"city":"LA"}
{"name":"Diana","age":28,"city":"NYC"}
{"name":"Eve","age":32,"city":"Boston"}"#
        .repeat(50);
    compress_decompress_roundtrip(json_data.as_bytes(), true);
}

#[test]
fn integration_csv_roundtrip_with_msn() {
    let csv_data = "name,age,city,status\nAlice,30,NYC,active\nBob,25,SF,inactive\n".repeat(200);
    compress_decompress_roundtrip(csv_data.as_bytes(), true);
}

#[test]
fn integration_yaml_roundtrip_with_msn() {
    let yaml_data = "host: server1\nport: 8080\nregion: us-east\n".repeat(200);
    compress_decompress_roundtrip(yaml_data.as_bytes(), true);
}

#[test]
fn integration_xml_roundtrip_with_msn() {
    let xml_data = "<person><name>Alice</name><age>30</age></person>\n".repeat(200);
    compress_decompress_roundtrip(xml_data.as_bytes(), true);
}

#[test]
fn integration_binary_roundtrip_no_msn() {
    let binary: Vec<u8> = (0u8..=255).cycle().take(32 * 1024).collect();
    compress_decompress_roundtrip(&binary, false);
}

#[test]
fn integration_empty_roundtrip() {
    compress_decompress_roundtrip(b"", false);
    compress_decompress_roundtrip(b"", true);
}

#[test]
fn integration_single_byte_roundtrip() {
    compress_decompress_roundtrip(b"x", false);
}

// ---------------------------------------------------------------------------
// Streaming + MSN integration
// ---------------------------------------------------------------------------

#[test]
fn integration_streaming_msn_json_roundtrip() {
    use cpac_streaming::stream::{StreamingCompressor, StreamingDecompressor};
    use cpac_streaming::MsnConfig;

    let json_data = r#"{"id":1,"host":"srv1","level":"info","code":200}"#.repeat(300);
    let input = json_data.as_bytes();

    let cfg = CompressConfig {
        enable_msn: true,
        msn_confidence: 0.7,
        ..Default::default()
    };
    let mut compressor =
        StreamingCompressor::with_msn(cfg, MsnConfig::default(), 2048, 64 << 20).unwrap();
    compressor.write(input).unwrap();
    let frame = compressor.finish().unwrap();

    let mut decompressor = StreamingDecompressor::new().unwrap();
    decompressor.feed(&frame).unwrap();
    let output = decompressor.read_output();

    assert_eq!(output, input, "streaming JSON MSN roundtrip mismatch");
    assert!(decompressor.is_done());
}

#[test]
fn integration_streaming_msn_csv_roundtrip() {
    use cpac_streaming::stream::{StreamingCompressor, StreamingDecompressor};
    use cpac_streaming::MsnConfig;

    let csv_data = "id,value,label\n".to_string()
        + &(0..500)
            .map(|i| format!("{i},{},{}\n", i * 7, if i % 2 == 0 { "ok" } else { "err" }))
            .collect::<String>();
    let input = csv_data.as_bytes();

    let cfg = CompressConfig {
        enable_msn: true,
        msn_confidence: 0.7,
        ..Default::default()
    };
    let mut compressor =
        StreamingCompressor::with_msn(cfg, MsnConfig::default(), 1024, 32 << 20).unwrap();
    compressor.write(input).unwrap();
    let frame = compressor.finish().unwrap();

    let mut decompressor = StreamingDecompressor::new().unwrap();
    decompressor.feed(&frame).unwrap();
    let output = decompressor.read_output();

    assert_eq!(output, input, "streaming CSV MSN roundtrip mismatch");
}

// ---------------------------------------------------------------------------
// Metadata compactness (MessagePack < JSON)
// ---------------------------------------------------------------------------

#[test]
fn integration_metadata_compact_smaller_than_json() {
    use cpac_msn::{decode_metadata_compact, encode_metadata_compact, MsnMetadata};
    use std::collections::HashMap;

    let mut fields = HashMap::new();
    fields.insert(
        "field_names".to_string(),
        serde_json::json!(["name", "age", "city", "status", "timestamp", "level"]),
    );

    let meta = MsnMetadata {
        version: 1,
        fields,
        applied: true,
        domain_id: Some("text.jsonlog".to_string()),
        confidence: 0.95,
    };

    let compact = encode_metadata_compact(&meta).unwrap();
    let json = serde_json::to_vec(&meta).unwrap();

    // MessagePack (compact) should be smaller than JSON.
    assert!(
        compact.len() < json.len(),
        "compact={} should be < json={}",
        compact.len(),
        json.len()
    );

    // Roundtrip: decode compact and verify fields survive.
    let decoded = decode_metadata_compact(&compact).unwrap();
    assert_eq!(decoded.applied, meta.applied);
    assert_eq!(decoded.domain_id, meta.domain_id);

    // Legacy JSON path also works.
    let decoded_json = decode_metadata_compact(&json).unwrap();
    assert_eq!(decoded_json.applied, meta.applied);
}

// ---------------------------------------------------------------------------
// Regression baseline roundtrip
// ---------------------------------------------------------------------------

#[test]
fn integration_regression_baseline_self_check() {
    use cpac_engine::bench::{
        check_regressions, load_baseline, save_baseline, BenchProfile, BenchmarkRunner,
    };
    use std::io::Write;
    use tempfile::tempdir;

    let dir = tempdir().unwrap();

    // Write a small compressible file.
    let file = dir.path().join("sample.txt");
    let mut f = std::fs::File::create(&file).unwrap();
    f.write_all(b"Hello CPAC integration test! ".repeat(200).as_slice())
        .unwrap();
    drop(f);

    // Benchmark it.
    let runner = BenchmarkRunner::new(BenchProfile::Quick);
    let result = runner.bench_file(&file, Backend::Zstd).unwrap();
    let results = vec![result];

    // Save and reload baseline.
    let baseline_path = dir.path().join("baseline.json");
    save_baseline(&baseline_path, &results).unwrap();
    let baseline = load_baseline(&baseline_path).unwrap();
    assert_eq!(baseline.len(), 1);

    // Self-check: no regressions against itself.
    let violations = check_regressions(&baseline, &results, 0.05, 0.10);
    assert!(
        violations.is_empty(),
        "self-check produced regressions: {violations:?}"
    );
}

// ---------------------------------------------------------------------------
// Error handling
// ---------------------------------------------------------------------------

#[test]
fn integration_decompress_invalid_frame() {
    let result = decompress(b"notaframe");
    assert!(
        matches!(result, Err(CpacError::InvalidFrame(_))),
        "expected InvalidFrame for garbage input"
    );
}

#[test]
fn integration_decompress_truncated_frame() {
    // Compress something, then truncate it.
    let config = CompressConfig::default();
    let compressed = compress(b"hello world", &config).unwrap();
    let truncated = &compressed.data[..compressed.data.len() / 2];
    let result = decompress(truncated);
    assert!(result.is_err(), "expected error for truncated frame");
}
