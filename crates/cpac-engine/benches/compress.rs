// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Criterion microbenchmarks: transforms, entropy backends, full pipeline.

use cpac_engine::{compress, decompress, Backend, CompressConfig};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;

// ── data generators ─────────────────────────────────────────────────────

fn gen_ascii(size: usize) -> Vec<u8> {
    b"The quick brown fox jumps over the lazy dog. "
        .iter()
        .cycle()
        .take(size)
        .copied()
        .collect()
}

fn gen_repetitive(size: usize) -> Vec<u8> {
    b"abcdef".iter().cycle().take(size).copied().collect()
}

fn gen_binary(size: usize) -> Vec<u8> {
    (0u8..=255).cycle().take(size).collect()
}

fn gen_csv(rows: usize) -> Vec<u8> {
    let mut out = String::from("id,name,value,status\n");
    for i in 0..rows {
        out.push_str(&format!(
            "{i},item_{i},{},{}\n",
            i * 7 % 1000,
            if i % 2 == 0 { "ok" } else { "err" }
        ));
    }
    out.into_bytes()
}

// ── transform benchmarks ────────────────────────────────────────────────

fn bench_transforms(c: &mut Criterion) {
    let mut g = c.benchmark_group("transforms");
    let d16 = gen_binary(16 * 1024);
    g.throughput(Throughput::Bytes(d16.len() as u64));

    // delta
    g.bench_function("delta_enc_16k", |b| {
        b.iter(|| cpac_transforms::delta::delta_encode(black_box(&d16)))
    });
    let de = cpac_transforms::delta::delta_encode(&d16);
    g.bench_function("delta_dec_16k", |b| {
        b.iter(|| cpac_transforms::delta::delta_decode(black_box(&de)))
    });

    // zigzag batch (i64 → varint bytes → i64)
    let zz_vals: Vec<i64> = d16.iter().map(|&b| b as i64 - 128).collect();
    g.bench_function("zigzag_enc_batch_16k", |b| {
        b.iter(|| cpac_transforms::zigzag::zigzag_encode_batch(black_box(&zz_vals)))
    });
    let zz_enc = cpac_transforms::zigzag::zigzag_encode_batch(&zz_vals);
    g.bench_function("zigzag_dec_batch_16k", |b| {
        b.iter(|| cpac_transforms::zigzag::zigzag_decode_batch(black_box(&zz_enc)))
    });

    // transpose
    g.bench_function("transpose_enc_16k_w8", |b| {
        b.iter(|| cpac_transforms::transpose::transpose_encode(black_box(&d16), 8))
    });
    if let Ok(te) = cpac_transforms::transpose::transpose_encode(&d16, 8) {
        g.bench_function("transpose_dec_16k_w8", |b| {
            b.iter(|| cpac_transforms::transpose::transpose_decode(black_box(&te), 8))
        });
    }

    // rolz
    let text = gen_ascii(16 * 1024);
    g.throughput(Throughput::Bytes(text.len() as u64));
    g.bench_function("rolz_enc_16k", |b| {
        b.iter(|| cpac_transforms::rolz::rolz_encode(black_box(&text)))
    });
    let re = cpac_transforms::rolz::rolz_encode(&text);
    g.bench_function("rolz_dec_16k", |b| {
        b.iter(|| cpac_transforms::rolz::rolz_decode(black_box(&re)))
    });

    g.finish();
}

// ── entropy backend benchmarks ──────────────────────────────────────────

fn bench_backends(c: &mut Criterion) {
    let mut g = c.benchmark_group("backends");

    for size in [1024usize, 16 * 1024, 64 * 1024] {
        let data = gen_ascii(size);
        g.throughput(Throughput::Bytes(data.len() as u64));

        for backend in [Backend::Zstd, Backend::Brotli, Backend::Raw] {
            let label = format!("{backend:?}_{size}");
            let dc = data.clone();
            g.bench_with_input(BenchmarkId::new("compress", &label), &dc, |b, d| {
                b.iter(|| cpac_entropy::compress(black_box(d), backend))
            });
            if let Ok(comp) = cpac_entropy::compress(&data, backend) {
                g.bench_with_input(BenchmarkId::new("decompress", &label), &comp, |b, d| {
                    b.iter(|| cpac_entropy::decompress(black_box(d), backend))
                });
            }
        }
    }

    g.finish();
}

// ── full pipeline benchmarks ────────────────────────────────────────────

fn bench_pipeline(c: &mut Criterion) {
    let mut g = c.benchmark_group("pipeline");

    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("ascii_1k", gen_ascii(1024)),
        ("ascii_16k", gen_ascii(16 * 1024)),
        ("repetitive_16k", gen_repetitive(16 * 1024)),
        ("binary_16k", gen_binary(16 * 1024)),
        ("csv_1k_rows", gen_csv(1000)),
    ];

    let cfg = CompressConfig::default();
    for (name, data) in &cases {
        g.throughput(Throughput::Bytes(data.len() as u64));
        g.bench_with_input(BenchmarkId::new("compress", name), data, |b, d| {
            b.iter(|| compress(black_box(d), black_box(&cfg)))
        });
        if let Ok(comp) = compress(data, &cfg) {
            g.bench_with_input(BenchmarkId::new("decompress", name), &comp.data, |b, d| {
                b.iter(|| decompress(black_box(d)))
            });
        }
    }

    g.finish();
}

criterion_group!(benches, bench_transforms, bench_backends, bench_pipeline);
criterion_main!(benches);
