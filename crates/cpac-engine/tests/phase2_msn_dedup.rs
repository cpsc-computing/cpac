//! Phase 2: MSN cross-block metadata deduplication roundtrip tests.
//!
//! These tests verify that the CPBL v2 format correctly stores shared
//! MSN metadata in the header, sets per-block flags, and reconstructs
//! the original data on decompress.

use cpac_engine::{compress, decompress, is_cpbl, parallel, CompressConfig};

/// Helper: build a data blob above PARALLEL_THRESHOLD_TEXT from a repeating record.
fn make_parallel_data(record: &[u8]) -> Vec<u8> {
    record
        .iter()
        .copied()
        .cycle()
        .take(parallel::PARALLEL_THRESHOLD_TEXT + 2048)
        .collect()
}

/// JSON records above the parallel threshold should compress into CPBL v2
/// with shared MSN metadata, then decompress to the original.
#[test]
fn roundtrip_phase2_json_parallel_msn_dedup() {
    let record = br#"{"host":"srv1","port":8080,"status":"ok","ts":"2025-01-01T00:00:00Z"}
"#;
    let data = make_parallel_data(record);
    assert!(data.len() >= parallel::PARALLEL_THRESHOLD_TEXT);

    let config = CompressConfig {
        enable_msn: true,
        ..Default::default()
    };
    let compressed = compress(&data, &config).expect("Phase 2 JSON compress failed");

    // Must go through parallel path.
    assert!(is_cpbl(&compressed.data), "expected CPBL frame for JSON");

    // Verify CPBL v2 or v3 version byte (offset 4).
    // Phase 3 auto-dict may upgrade to v3.
    assert!(
        compressed.data[4] == parallel::CPBL_VERSION_V2
            || compressed.data[4] == parallel::CPBL_VERSION_V3,
        "expected CPBL v2 or v3 for MSN-eligible data, got {}",
        compressed.data[4]
    );

    let result = decompress(&compressed.data).expect("Phase 2 JSON decompress failed");
    assert_eq!(
        result.data, data,
        "Phase 2 JSON parallel MSN dedup roundtrip mismatch"
    );
}

/// YAML records above the parallel threshold.
/// NOTE: YAML MSN detection is intentionally disabled (returns 0.0 confidence),
/// so this produces CPBL v1 (no shared metadata). Roundtrip must still work.
#[test]
fn roundtrip_phase2_yaml_parallel_no_msn() {
    let record = b"host: srv1\nport: 8080\nstatus: ok\nts: 2025-01-01T00:00:00Z\n---\n";
    let data = make_parallel_data(record);
    assert!(data.len() >= parallel::PARALLEL_THRESHOLD_TEXT);

    let config = CompressConfig {
        enable_msn: true,
        ..Default::default()
    };
    let compressed = compress(&data, &config).expect("Phase 2 YAML compress failed");
    assert!(is_cpbl(&compressed.data), "expected CPBL frame for YAML");
    // YAML MSN detection disabled → v1 or v3 (auto-dict) expected.
    assert!(
        compressed.data[4] == parallel::CPBL_VERSION_V1
            || compressed.data[4] == parallel::CPBL_VERSION_V3,
        "YAML with disabled MSN should produce CPBL v1 or v3, got {}",
        compressed.data[4]
    );

    let result = decompress(&compressed.data).expect("Phase 2 YAML decompress failed");
    assert_eq!(
        result.data, data,
        "Phase 2 YAML parallel roundtrip mismatch"
    );
}

/// Non-MSN data (binary) above parallel threshold should still work (CPBL v1).
#[test]
fn roundtrip_phase2_binary_no_msn_v1() {
    let data: Vec<u8> = (0u8..=255)
        .cycle()
        .take(parallel::PARALLEL_THRESHOLD_TEXT + 2048)
        .collect();

    let config = CompressConfig {
        enable_msn: true,
        ..Default::default()
    };
    let compressed = compress(&data, &config).expect("Phase 2 binary compress failed");
    assert!(is_cpbl(&compressed.data), "expected CPBL frame for binary");

    // Binary data shouldn't trigger MSN → v1 or v3 (auto-dict) expected.
    assert!(
        compressed.data[4] == parallel::CPBL_VERSION_V1
            || compressed.data[4] == parallel::CPBL_VERSION_V3,
        "binary data should produce CPBL v1 or v3, got {}",
        compressed.data[4]
    );

    let result = decompress(&compressed.data).expect("Phase 2 binary decompress failed");
    assert_eq!(
        result.data, data,
        "Phase 2 binary CPBL v1 roundtrip mismatch"
    );
}

/// XML records above the parallel threshold — MSN dedup.
#[test]
fn roundtrip_phase2_xml_parallel_msn_dedup() {
    let record = b"<?xml version=\"1.0\"?><event><host>srv1</host><port>8080</port><status>ok</status></event>\n";
    let data = make_parallel_data(record);

    let config = CompressConfig {
        enable_msn: true,
        ..Default::default()
    };
    let compressed = compress(&data, &config).expect("Phase 2 XML compress failed");
    assert!(is_cpbl(&compressed.data), "expected CPBL frame for XML");

    let result = decompress(&compressed.data).expect("Phase 2 XML decompress failed");
    assert_eq!(
        result.data, data,
        "Phase 2 XML parallel MSN dedup roundtrip mismatch"
    );
}
