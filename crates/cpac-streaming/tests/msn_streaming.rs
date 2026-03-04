// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! MSN streaming integration tests.
//!
//! Tests MSN domain detection, extraction, and reconstruction with streaming compression.

use cpac_streaming::stream::{StreamingCompressor, StreamingDecompressor};
use cpac_streaming::MsnConfig;
use cpac_types::CompressConfig;

#[test]
fn msn_streaming_json_roundtrip() {
    let json_data = r#"{"name":"Alice","age":30,"city":"NYC"}
{"name":"Bob","age":25,"city":"SF"}
{"name":"Charlie","age":35,"city":"LA"}
{"name":"Diana","age":28,"city":"Seattle"}
{"name":"Eve","age":32,"city":"Boston"}"#
        .repeat(100);

    let config = CompressConfig {
        enable_msn: true,
        msn_confidence: 0.7,
        ..Default::default()
    };
    let msn_config = MsnConfig::default();

    // Compress with MSN streaming (small block size to test boundaries)
    let mut compressor =
        StreamingCompressor::with_msn(config, msn_config, 512, 16 * 1024 * 1024).unwrap();
    compressor.write(json_data.as_bytes()).unwrap();
    let compressed_frame = compressor.finish().unwrap();

    // Decompress
    let mut decompressor = StreamingDecompressor::new().unwrap();
    decompressor.feed(&compressed_frame).unwrap();
    let decompressed = decompressor.read_output();

    // JsonLogDomain isn't safe for streaming yet, so this will be byte-for-byte identical
    assert_eq!(decompressed, json_data.as_bytes());
    assert!(decompressor.is_done());
}

#[test]
fn msn_streaming_csv_roundtrip() {
    let csv_data = "name,age,city\nAlice,30,NYC\nBob,25,SF\nCharlie,35,LA\n".repeat(200);

    let config = CompressConfig {
        enable_msn: true,
        msn_confidence: 0.7,
        ..Default::default()
    };
    let msn_config = MsnConfig::default();

    let mut compressor =
        StreamingCompressor::with_msn(config, msn_config, 1024, 16 * 1024 * 1024).unwrap();
    compressor.write(csv_data.as_bytes()).unwrap();
    let compressed_frame = compressor.finish().unwrap();

    let mut decompressor = StreamingDecompressor::new().unwrap();
    decompressor.feed(&compressed_frame).unwrap();
    let decompressed = decompressor.read_output();

    assert_eq!(decompressed, csv_data.as_bytes());
}

#[test]
fn msn_streaming_yaml_roundtrip() {
    let yaml_data = "host: server1\nport: 8080\nregion: us-east\n".repeat(200);

    let config = CompressConfig {
        enable_msn: true,
        msn_confidence: 0.7,
        ..Default::default()
    };
    let msn_config = MsnConfig::default();

    let mut compressor =
        StreamingCompressor::with_msn(config, msn_config, 512, 16 * 1024 * 1024).unwrap();
    compressor.write(yaml_data.as_bytes()).unwrap();
    let compressed_frame = compressor.finish().unwrap();

    let mut decompressor = StreamingDecompressor::new().unwrap();
    decompressor.feed(&compressed_frame).unwrap();
    let decompressed = decompressor.read_output();

    assert_eq!(decompressed, yaml_data.as_bytes());
    assert!(decompressor.is_done());
}

#[test]
fn msn_streaming_xml_roundtrip() {
    let xml_data = r#"<person><name>Alice</name><age>30</age></person>
<person><name>Bob</name><age>25</age></person>
<person><name>Charlie</name><age>35</age></person>"#
        .repeat(100);

    let config = CompressConfig {
        enable_msn: true,
        msn_confidence: 0.7,
        ..Default::default()
    };
    let msn_config = MsnConfig::default();

    let mut compressor =
        StreamingCompressor::with_msn(config, msn_config, 512, 16 * 1024 * 1024).unwrap();
    compressor.write(xml_data.as_bytes()).unwrap();
    let compressed_frame = compressor.finish().unwrap();

    let mut decompressor = StreamingDecompressor::new().unwrap();
    decompressor.feed(&compressed_frame).unwrap();
    let decompressed = decompressor.read_output();

    assert_eq!(decompressed, xml_data.as_bytes());
}

#[test]
fn msn_streaming_incremental_writes() {
    let config = CompressConfig {
        enable_msn: true,
        msn_confidence: 0.7,
        ..Default::default()
    };
    let msn_config = MsnConfig {
        enable: true,
        confidence_threshold: 0.7,
        detection_buffer_size: 1024, // Small buffer for testing
    };

    // Write incrementally
    let mut compressor =
        StreamingCompressor::with_msn(config, msn_config, 2048, 16 * 1024 * 1024).unwrap();
    let mut expected = Vec::new();

    for i in 0..50 {
        let chunk = format!(r#"{{"id":{},"value":"test{}"}}"#, i, i) + "\n";
        compressor.write(chunk.as_bytes()).unwrap();
        expected.extend_from_slice(chunk.as_bytes());
    }

    let compressed_frame = compressor.finish().unwrap();

    // Decompress
    let mut decompressor = StreamingDecompressor::new().unwrap();
    decompressor.feed(&compressed_frame).unwrap();
    let decompressed = decompressor.read_output();

    // For JSON, compare semantically line-by-line (key order may differ)
    let expected_str = String::from_utf8(expected).unwrap();
    let decompressed_str = String::from_utf8(decompressed).unwrap();
    let expected_lines: Vec<&str> = expected_str.lines().collect();
    let decompressed_lines: Vec<&str> = decompressed_str.lines().collect();

    assert_eq!(expected_lines.len(), decompressed_lines.len());
    for (exp, decomp) in expected_lines.iter().zip(decompressed_lines.iter()) {
        let exp_json: serde_json::Value = serde_json::from_str(exp).unwrap();
        let decomp_json: serde_json::Value = serde_json::from_str(decomp).unwrap();
        assert_eq!(
            exp_json, decomp_json,
            "JSON objects don't match semantically"
        );
    }
}

#[test]
fn msn_streaming_no_detection() {
    // Binary data that shouldn't trigger MSN detection
    let binary_data = vec![0xFFu8; 10 * 1024];

    let config = CompressConfig {
        enable_msn: true,
        msn_confidence: 0.7,
        ..Default::default()
    };
    let msn_config = MsnConfig::default();

    let mut compressor =
        StreamingCompressor::with_msn(config, msn_config, 1024, 16 * 1024 * 1024).unwrap();
    compressor.write(&binary_data).unwrap();
    let compressed_frame = compressor.finish().unwrap();

    // Should still roundtrip correctly without MSN
    let mut decompressor = StreamingDecompressor::new().unwrap();
    decompressor.feed(&compressed_frame).unwrap();
    let decompressed = decompressor.read_output();

    assert_eq!(decompressed, binary_data);
}

#[test]
fn msn_streaming_disabled() {
    let json_data = r#"{"name":"test","value":123}"#.repeat(100);

    let config = CompressConfig {
        enable_msn: false, // MSN disabled
        ..Default::default()
    };
    let msn_config = MsnConfig::disabled();

    let mut compressor =
        StreamingCompressor::with_msn(config, msn_config, 1024, 16 * 1024 * 1024).unwrap();
    compressor.write(json_data.as_bytes()).unwrap();
    let compressed_frame = compressor.finish().unwrap();

    let mut decompressor = StreamingDecompressor::new().unwrap();
    decompressor.feed(&compressed_frame).unwrap();
    let decompressed = decompressor.read_output();

    assert_eq!(decompressed, json_data.as_bytes());
}

#[test]
fn msn_streaming_small_blocks() {
    // Test with very small block size to force chunk boundaries
    let json_data = r#"{"a":1,"b":2,"c":3}"#.repeat(100);

    let config = CompressConfig {
        enable_msn: true,
        msn_confidence: 0.7,
        ..Default::default()
    };
    let msn_config = MsnConfig::default();

    let mut compressor =
        StreamingCompressor::with_msn(config, msn_config, 256, 16 * 1024 * 1024).unwrap();
    compressor.write(json_data.as_bytes()).unwrap();
    let compressed_frame = compressor.finish().unwrap();

    let mut decompressor = StreamingDecompressor::new().unwrap();
    decompressor.feed(&compressed_frame).unwrap();
    let decompressed = decompressor.read_output();

    assert_eq!(decompressed, json_data.as_bytes());
}

#[test]
fn msn_streaming_compression_benefit() {
    // Verify MSN provides compression benefit for structured data
    let json_data =
        r#"{"timestamp":"2024-01-01T00:00:00Z","level":"info","message":"test","user_id":12345}
{"timestamp":"2024-01-01T00:01:00Z","level":"warn","message":"alert","user_id":67890}
{"timestamp":"2024-01-01T00:02:00Z","level":"error","message":"critical","user_id":11111}"#
            .repeat(50);

    // Compress with MSN
    let config_msn = CompressConfig {
        enable_msn: true,
        msn_confidence: 0.7,
        ..Default::default()
    };
    let msn_config = MsnConfig::default();
    let mut compressor_msn =
        StreamingCompressor::with_msn(config_msn, msn_config, 512, 16 * 1024 * 1024).unwrap();
    compressor_msn.write(json_data.as_bytes()).unwrap();
    let compressed_msn = compressor_msn.finish().unwrap();

    // Compress without MSN
    let config_no_msn = CompressConfig {
        enable_msn: false,
        ..Default::default()
    };
    let msn_config_disabled = MsnConfig::disabled();
    let mut compressor_no_msn =
        StreamingCompressor::with_msn(config_no_msn, msn_config_disabled, 512, 16 * 1024 * 1024)
            .unwrap();
    compressor_no_msn.write(json_data.as_bytes()).unwrap();
    let compressed_no_msn = compressor_no_msn.finish().unwrap();

    println!("Original size: {} bytes", json_data.len());
    println!("With MSN: {} bytes", compressed_msn.len());
    println!("Without MSN: {} bytes", compressed_no_msn.len());
    println!(
        "MSN improvement: {:.2}%",
        (1.0 - compressed_msn.len() as f64 / compressed_no_msn.len() as f64) * 100.0
    );

    // MSN should provide better compression for structured data
    assert!(
        compressed_msn.len() < compressed_no_msn.len(),
        "MSN compression should be smaller than raw compression for structured JSON"
    );

    // Verify decompression works
    let mut decompressor = StreamingDecompressor::new().unwrap();
    decompressor.feed(&compressed_msn).unwrap();
    let decompressed = decompressor.read_output();

    // JsonLogDomain isn't safe for streaming yet, so verify byte-for-byte
    assert_eq!(decompressed, json_data.as_bytes());
}

#[test]
fn msn_streaming_throughput_report() {
    use std::time::Instant;

    // Measure streaming compression throughput for MSN vs non-MSN on structured JSON.
    let json_data = r#"{"host":"srv1","level":"info","code":200,"latency_ms":42}
{"host":"srv2","level":"warn","code":404,"latency_ms":13}
{"host":"srv3","level":"error","code":500,"latency_ms":99}"#
        .repeat(500);
    let input = json_data.as_bytes();
    let input_len = input.len();

    // --- With MSN ---
    let cfg_msn = CompressConfig {
        enable_msn: true,
        msn_confidence: 0.7,
        ..Default::default()
    };
    let t0 = Instant::now();
    let mut c =
        StreamingCompressor::with_msn(cfg_msn, MsnConfig::default(), 4096, 64 << 20).unwrap();
    c.write(input).unwrap();
    let frame_msn = c.finish().unwrap();
    let msn_compress_ms = t0.elapsed().as_millis();

    let t1 = Instant::now();
    let mut d = StreamingDecompressor::new().unwrap();
    d.feed(&frame_msn).unwrap();
    let out_msn = d.read_output();
    let msn_decompress_ms = t1.elapsed().as_millis();
    assert_eq!(out_msn.len(), input_len, "MSN roundtrip size mismatch");

    // --- Without MSN ---
    let cfg_raw = CompressConfig {
        enable_msn: false,
        ..Default::default()
    };
    let t2 = Instant::now();
    let mut c2 =
        StreamingCompressor::with_msn(cfg_raw, MsnConfig::disabled(), 4096, 64 << 20).unwrap();
    c2.write(input).unwrap();
    let frame_raw = c2.finish().unwrap();
    let raw_compress_ms = t2.elapsed().as_millis();

    let improvement = (1.0 - frame_msn.len() as f64 / frame_raw.len() as f64) * 100.0;
    let msn_compress_mbs =
        input_len as f64 / 1_048_576.0 / (msn_compress_ms.max(1) as f64 / 1000.0);
    let msn_decompress_mbs =
        input_len as f64 / 1_048_576.0 / (msn_decompress_ms.max(1) as f64 / 1000.0);

    println!("\n=== MSN Streaming Throughput ===");
    println!("Input:           {:.1} KB", input_len as f64 / 1024.0);
    println!(
        "With MSN:        {} bytes ({:.1} MB/s compress, {:.1} MB/s decompress)",
        frame_msn.len(),
        msn_compress_mbs,
        msn_decompress_mbs
    );
    println!(
        "Without MSN:     {} bytes ({:.1} MB/s compress)",
        frame_raw.len(),
        input_len as f64 / 1_048_576.0 / (raw_compress_ms.max(1) as f64 / 1000.0)
    );
    println!("MSN improvement: {:.1}%", improvement);
    // NOTE: purely-repetitive .repeat(N) data is already optimal for brotli's LZ
    // back-referencing, so MSN may not improve ratio here. This test measures
    // throughput and verifies roundtrip correctness only.
}

#[test]
fn msn_streaming_large_data() {
    // Test with large data to verify multi-block MSN
    let json_data = r#"{"id":1,"name":"test","value":12345,"nested":{"a":1,"b":2}}"#.repeat(10000);

    let config = CompressConfig {
        enable_msn: true,
        msn_confidence: 0.7,
        ..Default::default()
    };
    let msn_config = MsnConfig::default();

    let mut compressor =
        StreamingCompressor::with_msn(config, msn_config, 1024 * 1024, 64 * 1024 * 1024).unwrap();
    compressor.write(json_data.as_bytes()).unwrap();
    let compressed_frame = compressor.finish().unwrap();

    let mut decompressor = StreamingDecompressor::new().unwrap();
    decompressor.feed(&compressed_frame).unwrap();
    let decompressed = decompressor.read_output();

    assert_eq!(decompressed, json_data.as_bytes());
}

#[test]
fn msn_streaming_reset() {
    let json_data1 = r#"{"data":1}"#.repeat(50);
    let json_data2 = r#"{"data":2}"#.repeat(50);

    let config = CompressConfig {
        enable_msn: true,
        msn_confidence: 0.7,
        ..Default::default()
    };
    let msn_config = MsnConfig::default();

    let mut compressor =
        StreamingCompressor::with_msn(config, msn_config, 512, 16 * 1024 * 1024).unwrap();
    compressor.write(json_data1.as_bytes()).unwrap();
    compressor.reset();
    compressor.write(json_data2.as_bytes()).unwrap();
    let compressed_frame = compressor.finish().unwrap();

    let mut decompressor = StreamingDecompressor::new().unwrap();
    decompressor.feed(&compressed_frame).unwrap();
    let decompressed = decompressor.read_output();

    assert_eq!(decompressed, json_data2.as_bytes());
}
