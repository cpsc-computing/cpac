// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Real-world data tests for MSN compression.

use cpac_engine::{compress, decompress, CompressConfig};

/// Test MSN on realistic Apache access logs.
#[test]
fn apache_access_logs() {
    let log_data = r#"192.168.1.100 - - [03/Mar/2026:10:15:23 +0000] "GET /index.html HTTP/1.1" 200 4523 "-" "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36"
192.168.1.101 - - [03/Mar/2026:10:15:24 +0000] "GET /style.css HTTP/1.1" 200 2341 "http://example.com/index.html" "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36"
192.168.1.102 - - [03/Mar/2026:10:15:25 +0000] "POST /api/login HTTP/1.1" 200 156 "-" "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36"
192.168.1.103 - - [03/Mar/2026:10:15:26 +0000] "GET /images/logo.png HTTP/1.1" 200 8932 "http://example.com/index.html" "Mozilla/5.0 (iPhone; CPU iPhone OS 14_0 like Mac OS X)"
192.168.1.104 - - [03/Mar/2026:10:15:27 +0000] "GET /api/users HTTP/1.1" 200 1234 "-" "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36"
"#.repeat(100);

    // Without MSN
    let config_no_msn = CompressConfig {
        enable_msn: false,
        ..Default::default()
    };
    let result_no_msn = compress(log_data.as_bytes(), &config_no_msn).unwrap();
    
    // With MSN
    let config_msn = CompressConfig {
        enable_msn: true,
        ..Default::default()
    };
    let result_msn = compress(log_data.as_bytes(), &config_msn).unwrap();
    
    println!("Apache logs ({} bytes):", log_data.len());
    println!("  Without MSN: {} bytes ({:.2}x)", result_no_msn.compressed_size, result_no_msn.ratio());
    println!("  With MSN: {} bytes ({:.2}x)", result_msn.compressed_size, result_msn.ratio());
    
    // Verify decompression
    let decompressed = decompress(&result_msn.data).unwrap();
    assert_eq!(decompressed.data, log_data.as_bytes());
}

/// Test MSN on JSON API responses.
#[test]
fn json_api_responses() {
    let json_data = r#"{"users":[{"id":1,"name":"Alice","email":"alice@example.com","role":"admin","created":"2026-01-01T00:00:00Z"},{"id":2,"name":"Bob","email":"bob@example.com","role":"user","created":"2026-01-02T00:00:00Z"},{"id":3,"name":"Charlie","email":"charlie@example.com","role":"user","created":"2026-01-03T00:00:00Z"}]}"#;
    
    // Without MSN
    let config_no_msn = CompressConfig {
        enable_msn: false,
        ..Default::default()
    };
    let result_no_msn = compress(json_data.as_bytes(), &config_no_msn).unwrap();
    
    // With MSN
    let config_msn = CompressConfig {
        enable_msn: true,
        ..Default::default()
    };
    let result_msn = compress(json_data.as_bytes(), &config_msn).unwrap();
    
    println!("JSON API response ({} bytes):", json_data.len());
    println!("  Without MSN: {} bytes ({:.2}x)", result_no_msn.compressed_size, result_no_msn.ratio());
    println!("  With MSN: {} bytes ({:.2}x)", result_msn.compressed_size, result_msn.ratio());
    
    // Verify decompression
    let decompressed = decompress(&result_msn.data).unwrap();
    assert_eq!(decompressed.data, json_data.as_bytes());
}

/// Test MSN on CSV data export.
#[test]
fn csv_export() {
    let csv_data = b"timestamp,user_id,action,resource,duration_ms,status\n\
2026-03-03T10:00:01Z,user123,read,/api/data,45,200\n\
2026-03-03T10:00:02Z,user456,write,/api/data,120,201\n\
2026-03-03T10:00:03Z,user789,read,/api/users,32,200\n\
2026-03-03T10:00:04Z,user123,delete,/api/data,67,204\n\
2026-03-03T10:00:05Z,user456,read,/api/stats,89,200\n".repeat(200);
    
    // Without MSN
    let config_no_msn = CompressConfig {
        enable_msn: false,
        ..Default::default()
    };
    let result_no_msn = compress(&csv_data, &config_no_msn).unwrap();
    
    // With MSN
    let config_msn = CompressConfig {
        enable_msn: true,
        ..Default::default()
    };
    let result_msn = compress(&csv_data, &config_msn).unwrap();
    
    println!("CSV export ({} bytes):", csv_data.len());
    println!("  Without MSN: {} bytes ({:.2}x)", result_no_msn.compressed_size, result_no_msn.ratio());
    println!("  With MSN: {} bytes ({:.2}x)", result_msn.compressed_size, result_msn.ratio());
    
    // Verify decompression
    let decompressed = decompress(&result_msn.data).unwrap();
    assert_eq!(decompressed.data, csv_data);
}

/// Test MSN on syslog messages.
#[test]
fn syslog_messages() {
    let syslog_data = b"<34>1 2026-03-03T10:15:23.123Z web01 nginx 1234 - - user logged in successfully\n\
<34>1 2026-03-03T10:15:24.456Z web01 nginx 1235 - - GET /api/users returned 200\n\
<34>1 2026-03-03T10:15:25.789Z web02 nginx 1236 - - POST /api/login returned 401\n\
<34>1 2026-03-03T10:15:26.012Z web01 nginx 1237 - - cache hit for /static/image.png\n\
<34>1 2026-03-03T10:15:27.345Z web02 nginx 1238 - - database query took 45ms\n".repeat(100);
    
    // Without MSN
    let config_no_msn = CompressConfig {
        enable_msn: false,
        ..Default::default()
    };
    let result_no_msn = compress(&syslog_data, &config_no_msn).unwrap();
    
    // With MSN
    let config_msn = CompressConfig {
        enable_msn: true,
        ..Default::default()
    };
    let result_msn = compress(&syslog_data, &config_msn).unwrap();
    
    println!("Syslog messages ({} bytes):", syslog_data.len());
    println!("  Without MSN: {} bytes ({:.2}x)", result_no_msn.compressed_size, result_no_msn.ratio());
    println!("  With MSN: {} bytes ({:.2}x)", result_msn.compressed_size, result_msn.ratio());
    
    // Verify decompression
    let decompressed = decompress(&result_msn.data).unwrap();
    assert_eq!(decompressed.data, syslog_data);
}

/// Test MSN on JSONL (newline-delimited JSON) application logs.
#[test]
fn jsonl_application_logs() {
    let jsonl_data = r#"{"timestamp":"2026-03-03T10:00:01Z","level":"INFO","service":"api","message":"Request received","request_id":"req-001"}
{"timestamp":"2026-03-03T10:00:02Z","level":"DEBUG","service":"api","message":"Database query started","request_id":"req-001"}
{"timestamp":"2026-03-03T10:00:03Z","level":"DEBUG","service":"api","message":"Database query completed","request_id":"req-001"}
{"timestamp":"2026-03-03T10:00:04Z","level":"INFO","service":"api","message":"Response sent","request_id":"req-001"}
{"timestamp":"2026-03-03T10:00:05Z","level":"ERROR","service":"api","message":"Connection timeout","request_id":"req-002"}
"#.repeat(200);
    
    // Without MSN
    let config_no_msn = CompressConfig {
        enable_msn: false,
        ..Default::default()
    };
    let result_no_msn = compress(jsonl_data.as_bytes(), &config_no_msn).unwrap();
    
    // With MSN
    let config_msn = CompressConfig {
        enable_msn: true,
        ..Default::default()
    };
    let result_msn = compress(jsonl_data.as_bytes(), &config_msn).unwrap();
    
    println!("JSONL logs ({} bytes):", jsonl_data.len());
    println!("  Without MSN: {} bytes ({:.2}x)", result_no_msn.compressed_size, result_no_msn.ratio());
    println!("  With MSN: {} bytes ({:.2}x)", result_msn.compressed_size, result_msn.ratio());
    
    // Verify decompression
    let decompressed = decompress(&result_msn.data).unwrap();
    assert_eq!(decompressed.data, jsonl_data.as_bytes());
}

/// Test MSN on XML configuration files.
#[test]
fn xml_config() {
    let xml_data = br#"<?xml version="1.0"?>
<config>
  <server><host>localhost</host><port>8080</port><ssl>true</ssl></server>
  <database><host>db.example.com</host><port>5432</port><name>production</name></database>
  <cache><enabled>true</enabled><ttl>3600</ttl><max_size>1000</max_size></cache>
  <logging><level>INFO</level><format>json</format><output>/var/log/app.log</output></logging>
</config>"#.repeat(50);
    
    // Without MSN
    let config_no_msn = CompressConfig {
        enable_msn: false,
        ..Default::default()
    };
    let result_no_msn = compress(&xml_data, &config_no_msn).unwrap();
    
    // With MSN
    let config_msn = CompressConfig {
        enable_msn: true,
        ..Default::default()
    };
    let result_msn = compress(&xml_data, &config_msn).unwrap();
    
    println!("XML config ({} bytes):", xml_data.len());
    println!("  Without MSN: {} bytes ({:.2}x)", result_no_msn.compressed_size, result_no_msn.ratio());
    println!("  With MSN: {} bytes ({:.2}x)", result_msn.compressed_size, result_msn.ratio());
    
    // Verify decompression
    let decompressed = decompress(&result_msn.data).unwrap();
    assert_eq!(decompressed.data, xml_data);
}
