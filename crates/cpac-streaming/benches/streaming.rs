// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Criterion benchmarks for streaming compression.
//!
//! Measures throughput of `StreamingCompressor` and `StreamingDecompressor`
//! with and without MSN, on structured (JSON) and unstructured (binary) data.

use cpac_streaming::{
    stream::{StreamingCompressor, StreamingDecompressor},
    MsnConfig,
};
use cpac_types::CompressConfig;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;

// ---------------------------------------------------------------------------
// Data generators
// ---------------------------------------------------------------------------

fn gen_json_log(rows: usize) -> Vec<u8> {
    let mut s = String::new();
    for i in 0..rows {
        s.push_str(&format!(
            r#"{{"id":{},"host":"srv{}","level":"info","code":200,"latency_ms":{}}}"#,
            i,
            i % 8,
            i % 500
        ));
        s.push('\n');
    }
    s.into_bytes()
}

fn gen_csv(rows: usize) -> Vec<u8> {
    let mut s = String::from("id,name,value,status\n");
    for i in 0..rows {
        s.push_str(&format!(
            "{},item_{},{},{}\n",
            i,
            i,
            i * 7 % 1000,
            if i % 2 == 0 { "ok" } else { "err" }
        ));
    }
    s.into_bytes()
}

fn gen_binary(size: usize) -> Vec<u8> {
    (0u8..=255).cycle().take(size).collect()
}

// ---------------------------------------------------------------------------
// Compression benchmarks
// ---------------------------------------------------------------------------

/// Benchmark streaming compression with and without MSN on JSON log data.
fn bench_streaming_compress_json(c: &mut Criterion) {
    let mut g = c.benchmark_group("streaming_compress_json");
    for rows in [100usize, 1000, 5000] {
        let data = gen_json_log(rows);
        g.throughput(Throughput::Bytes(data.len() as u64));

        // With MSN
        let cfg_msn = CompressConfig {
            enable_msn: true,
            msn_confidence: 0.7,
            ..Default::default()
        };
        let msn_cfg = MsnConfig::default();
        g.bench_with_input(BenchmarkId::new("with_msn", rows), &data, |b, d| {
            b.iter(|| {
                let mut c =
                    StreamingCompressor::with_msn(cfg_msn.clone(), msn_cfg.clone(), 4096, 64 << 20)
                        .unwrap();
                c.write(black_box(d)).unwrap();
                c.finish().unwrap()
            })
        });

        // Without MSN
        let cfg_raw = CompressConfig {
            enable_msn: false,
            ..Default::default()
        };
        let msn_disabled = MsnConfig::disabled();
        g.bench_with_input(BenchmarkId::new("no_msn", rows), &data, |b, d| {
            b.iter(|| {
                let mut c = StreamingCompressor::with_msn(
                    cfg_raw.clone(),
                    msn_disabled.clone(),
                    4096,
                    64 << 20,
                )
                .unwrap();
                c.write(black_box(d)).unwrap();
                c.finish().unwrap()
            })
        });
    }
    g.finish();
}

/// Benchmark streaming compression on CSV data.
fn bench_streaming_compress_csv(c: &mut Criterion) {
    let mut g = c.benchmark_group("streaming_compress_csv");
    for rows in [500usize, 2000] {
        let data = gen_csv(rows);
        g.throughput(Throughput::Bytes(data.len() as u64));

        let cfg_msn = CompressConfig {
            enable_msn: true,
            msn_confidence: 0.7,
            ..Default::default()
        };
        let msn_cfg = MsnConfig::default();
        g.bench_with_input(BenchmarkId::new("with_msn", rows), &data, |b, d| {
            b.iter(|| {
                let mut c =
                    StreamingCompressor::with_msn(cfg_msn.clone(), msn_cfg.clone(), 4096, 64 << 20)
                        .unwrap();
                c.write(black_box(d)).unwrap();
                c.finish().unwrap()
            })
        });
    }
    g.finish();
}

/// Benchmark streaming compression on binary (random) data.
fn bench_streaming_compress_binary(c: &mut Criterion) {
    let mut g = c.benchmark_group("streaming_compress_binary");
    for size in [16 * 1024usize, 128 * 1024] {
        let data = gen_binary(size);
        g.throughput(Throughput::Bytes(data.len() as u64));

        let cfg = CompressConfig {
            enable_msn: false,
            ..Default::default()
        };
        g.bench_with_input(BenchmarkId::new("compress", size), &data, |b, d| {
            b.iter(|| {
                let mut c = StreamingCompressor::with_msn(
                    cfg.clone(),
                    MsnConfig::disabled(),
                    4096,
                    64 << 20,
                )
                .unwrap();
                c.write(black_box(d)).unwrap();
                c.finish().unwrap()
            })
        });
    }
    g.finish();
}

// ---------------------------------------------------------------------------
// Decompression benchmarks
// ---------------------------------------------------------------------------

/// Benchmark streaming decompression throughput.
fn bench_streaming_decompress(c: &mut Criterion) {
    let mut g = c.benchmark_group("streaming_decompress");

    // Pre-compress a JSON log corpus to measure decompression only.
    let data = gen_json_log(2000);
    let cfg = CompressConfig {
        enable_msn: true,
        msn_confidence: 0.7,
        ..Default::default()
    };
    let mut comp =
        StreamingCompressor::with_msn(cfg, MsnConfig::default(), 4096, 64 << 20).unwrap();
    comp.write(&data).unwrap();
    let frame = comp.finish().unwrap();

    g.throughput(Throughput::Bytes(data.len() as u64));
    g.bench_function("json_log_msn", |b| {
        b.iter(|| {
            let mut d = StreamingDecompressor::new().unwrap();
            d.feed(black_box(&frame)).unwrap();
            d.read_output()
        })
    });

    // Binary (no MSN)
    let data_bin = gen_binary(64 * 1024);
    let cfg_bin = CompressConfig {
        enable_msn: false,
        ..Default::default()
    };
    let mut comp_bin =
        StreamingCompressor::with_msn(cfg_bin, MsnConfig::disabled(), 4096, 64 << 20).unwrap();
    comp_bin.write(&data_bin).unwrap();
    let frame_bin = comp_bin.finish().unwrap();

    g.throughput(Throughput::Bytes(data_bin.len() as u64));
    g.bench_function("binary_no_msn", |b| {
        b.iter(|| {
            let mut d = StreamingDecompressor::new().unwrap();
            d.feed(black_box(&frame_bin)).unwrap();
            d.read_output()
        })
    });

    g.finish();
}

criterion_group!(
    benches,
    bench_streaming_compress_json,
    bench_streaming_compress_csv,
    bench_streaming_compress_binary,
    bench_streaming_decompress,
);
criterion_main!(benches);
