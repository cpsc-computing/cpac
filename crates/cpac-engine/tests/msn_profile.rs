// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! MSN profiling tests - measure overhead on Track 2 data.

use cpac_engine::{compress, CompressConfig};
use std::time::Instant;

/// Test that MSN has <5% overhead on Track 2 (generic/binary) data.
/// MSN should be skipped entirely for Track 2, so overhead should be minimal.
#[test]
fn msn_overhead_track2() {
    // Binary data - should be Track 2
    let binary_data: Vec<u8> = (0u8..=255).cycle().take(1024 * 1024).collect(); // 1MB binary

    // Warm up
    for _ in 0..3 {
        let _ = compress(&binary_data[..1024], &CompressConfig::default());
    }

    // Baseline (MSN disabled)
    let start = Instant::now();
    for _ in 0..10 {
        let config = CompressConfig {
            enable_msn: false,
            ..Default::default()
        };
        let _ = compress(&binary_data, &config).unwrap();
    }
    let baseline_duration = start.elapsed();

    // With MSN (should be skipped for Track 2)
    let start = Instant::now();
    for _ in 0..10 {
        let config = CompressConfig {
            enable_msn: true,
            ..Default::default()
        };
        let _ = compress(&binary_data, &config).unwrap();
    }
    let msn_duration = start.elapsed();

    let overhead_pct =
        ((msn_duration.as_secs_f64() / baseline_duration.as_secs_f64()) - 1.0) * 100.0;

    println!("Baseline (MSN off): {:?}", baseline_duration);
    println!("With MSN (Track 2): {:?}", msn_duration);
    println!("Overhead: {:.2}%", overhead_pct);

    // MSN should add <5% overhead on Track 2 data (should be near zero)
    assert!(
        overhead_pct < 5.0,
        "MSN overhead on Track 2 data: {:.2}%",
        overhead_pct
    );
}

/// Test MSN benefit on structured JSON data (Track 1).
#[test]
fn msn_benefit_track1() {
    // Large repetitive JSON with many fields - should benefit from MSN
    let json_record = r#"{"timestamp":1234567890,"user_id":"user123","action":"click","page":"/home","session":"abc","ip":"192.168.1.1","user_agent":"Mozilla/5.0","referrer":"https://example.com","status":200,"duration_ms":150}"#;

    // Create 1000 records to simulate realistic log volume
    let data = format!(
        "{}
",
        json_record
    )
    .repeat(1000);

    // Without MSN
    let config_no_msn = CompressConfig {
        enable_msn: false,
        ..Default::default()
    };
    let result_no_msn = compress(data.as_bytes(), &config_no_msn).unwrap();

    // With MSN
    let config_msn = CompressConfig {
        enable_msn: true,
        ..Default::default()
    };
    let result_msn = compress(data.as_bytes(), &config_msn).unwrap();

    println!("Original size: {} bytes", data.len());
    println!(
        "Without MSN: {} bytes ({:.2}x)",
        result_no_msn.compressed_size,
        result_no_msn.ratio()
    );
    println!(
        "With MSN: {} bytes ({:.2}x)",
        result_msn.compressed_size,
        result_msn.ratio()
    );

    // MSN should achieve better compression on structured data
    // (exact ratio depends on SSR track selection and entropy backend)
    println!(
        "MSN improvement: {:.1}%",
        ((result_no_msn.compressed_size as f64 / result_msn.compressed_size as f64) - 1.0) * 100.0
    );
}

/// Measure MSN extraction time vs compression time.
#[test]
fn msn_time_breakdown() {
    let json_data = r#"{"field1":"value","field2":123,"field3":"data"}
{"field1":"other","field2":456,"field3":"more"}
{"field1":"third","field2":789,"field3":"test"}"#
        .repeat(100);

    let data = json_data.as_bytes();

    // Measure just MSN extraction
    let start = Instant::now();
    let _ = cpac_msn::extract(data, None, 0.5).unwrap();
    let msn_time = start.elapsed();

    // Measure full compression with MSN
    let start = Instant::now();
    let config = CompressConfig {
        enable_msn: true,
        ..Default::default()
    };
    let _ = compress(data, &config).unwrap();
    let total_time = start.elapsed();

    println!("MSN extraction: {:?}", msn_time);
    println!("Total compression: {:?}", total_time);
    println!(
        "MSN percentage: {:.1}%",
        (msn_time.as_secs_f64() / total_time.as_secs_f64()) * 100.0
    );
}
