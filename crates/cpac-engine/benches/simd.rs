// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Criterion microbenchmarks for SIMD-accelerated transforms.
//!
//! Compares the `*_fast` (auto-dispatched) SIMD path against the plain
//! scalar implementations for delta and zigzag at several data sizes.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;

// ── data generators ─────────────────────────────────────────────────────

fn gen_sequential(size: usize) -> Vec<u8> {
    (0u8..=255).cycle().take(size).collect()
}

fn gen_signed(size: usize) -> Vec<u8> {
    (0..size)
        .map(|i| ((i as i16 * 3 - 128) & 0xFF) as u8)
        .collect()
}

// ── delta benchmarks ────────────────────────────────────────────────────

fn bench_delta(c: &mut Criterion) {
    let mut g = c.benchmark_group("simd_delta");

    for size in [64, 256, 1024, 16 * 1024, 64 * 1024, 256 * 1024] {
        let data = gen_sequential(size);
        g.throughput(Throughput::Bytes(size as u64));

        // SIMD-dispatched (auto picks best tier)
        g.bench_with_input(BenchmarkId::new("fast_enc", size), &data, |b, d| {
            b.iter(|| cpac_transforms::simd::delta_encode_fast(black_box(d)))
        });

        // Scalar fallback
        g.bench_with_input(BenchmarkId::new("scalar_enc", size), &data, |b, d| {
            b.iter(|| cpac_transforms::delta::delta_encode(black_box(d)))
        });

        // Decode (currently scalar for both paths)
        let encoded = cpac_transforms::simd::delta_encode_fast(&data);
        g.bench_with_input(BenchmarkId::new("fast_dec", size), &encoded, |b, d| {
            b.iter(|| cpac_transforms::simd::delta_decode_fast(black_box(d)))
        });
    }

    g.finish();
}

// ── zigzag benchmarks ───────────────────────────────────────────────────

fn bench_zigzag(c: &mut Criterion) {
    let mut g = c.benchmark_group("simd_zigzag");

    for size in [64, 256, 1024, 16 * 1024, 64 * 1024, 256 * 1024] {
        let data = gen_signed(size);
        g.throughput(Throughput::Bytes(size as u64));

        // SIMD-dispatched encode
        g.bench_with_input(BenchmarkId::new("fast_enc", size), &data, |b, d| {
            b.iter(|| cpac_transforms::simd::zigzag_encode_fast(black_box(d)))
        });

        // SIMD-dispatched decode
        let encoded = cpac_transforms::simd::zigzag_encode_fast(&data);
        g.bench_with_input(BenchmarkId::new("fast_dec", size), &encoded, |b, d| {
            b.iter(|| cpac_transforms::simd::zigzag_decode_fast(black_box(d)))
        });
    }

    g.finish();
}

// ── transpose benchmarks ────────────────────────────────────────────────

fn bench_transpose(c: &mut Criterion) {
    let mut g = c.benchmark_group("simd_transpose");

    for size in [256, 1024, 16 * 1024, 64 * 1024] {
        let data = gen_sequential(size);
        g.throughput(Throughput::Bytes(size as u64));

        for width in [4usize, 8] {
            let label = format!("{size}_w{width}");

            // SIMD-dispatched encode
            g.bench_with_input(BenchmarkId::new("fast_enc", &label), &data, |b, d| {
                b.iter(|| cpac_transforms::simd::transpose_encode_fast(black_box(d), width))
            });

            // Scalar encode
            g.bench_with_input(BenchmarkId::new("scalar_enc", &label), &data, |b, d| {
                b.iter(|| cpac_transforms::transpose::transpose_encode(black_box(d), width))
            });

            // SIMD-dispatched decode
            if let Ok(encoded) = cpac_transforms::simd::transpose_encode_fast(&data, width) {
                g.bench_with_input(BenchmarkId::new("fast_dec", &label), &encoded, |b, d| {
                    b.iter(|| cpac_transforms::simd::transpose_decode_fast(black_box(d), width))
                });
            }
        }
    }

    g.finish();
}

criterion_group!(benches, bench_delta, bench_zigzag, bench_transpose);
criterion_main!(benches);
