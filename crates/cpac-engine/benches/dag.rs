// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Criterion microbenchmarks for the transform DAG subsystem.
//!
//! Benchmarks compilation, forward execution (compression transforms),
//! and backward execution (decompression transforms) of DAG profiles.

use cpac_engine::{ProfileCache, TransformDAG, TransformRegistry};
use cpac_transforms::TransformContext;
use cpac_types::CpacType;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;

// ── data generators ─────────────────────────────────────────────────────

fn gen_structured(size: usize) -> Vec<u8> {
    // Structured 4-byte records with columnar patterns
    let n_records = size / 4;
    let mut data = Vec::with_capacity(n_records * 4);
    for i in 0..n_records {
        data.push(0x01);
        data.push((i & 0xFF) as u8);
        data.push(0xFF);
        data.push(0x00);
    }
    data
}

fn gen_text(size: usize) -> Vec<u8> {
    b"The quick brown fox jumps over the lazy dog. "
        .iter()
        .cycle()
        .take(size)
        .copied()
        .collect()
}

fn default_ctx(data: &[u8], ascii_ratio: f64) -> TransformContext {
    TransformContext {
        entropy_estimate: 4.5,
        ascii_ratio,
        data_size: data.len(),
    }
}

// ── DAG compilation benchmarks ──────────────────────────────────────────

fn bench_dag_compile(c: &mut Criterion) {
    let registry = TransformRegistry::with_builtins();
    let mut g = c.benchmark_group("dag_compile");

    // Single-transform DAG
    g.bench_function("single_delta", |b| {
        b.iter(|| TransformDAG::compile(black_box(&registry), black_box(&["delta"])))
    });

    // Multi-transform chain
    let chain = ["delta", "zigzag", "transpose"];
    g.bench_function("chain_3", |b| {
        b.iter(|| TransformDAG::compile(black_box(&registry), black_box(&chain)))
    });

    g.finish();
}

// ── DAG auto-select benchmarks ──────────────────────────────────────────

fn bench_dag_auto_select(c: &mut Criterion) {
    let registry = TransformRegistry::with_builtins();
    let mut g = c.benchmark_group("dag_auto_select");

    for size in [1024usize, 16 * 1024, 64 * 1024] {
        let data = gen_structured(size);
        let ctx = default_ctx(&data, 0.1);
        let input = CpacType::Serial(data);
        g.throughput(Throughput::Bytes(size as u64));

        g.bench_with_input(BenchmarkId::new("structured", size), &input, |b, inp| {
            b.iter(|| {
                TransformDAG::auto_select(black_box(&registry), black_box(inp), black_box(&ctx))
            })
        });
    }

    g.finish();
}

// ── DAG forward/backward execution benchmarks ───────────────────────────

fn bench_dag_execute(c: &mut Criterion) {
    let registry = TransformRegistry::with_builtins();
    let mut g = c.benchmark_group("dag_execute");

    // Single-step: delta only
    let dag_delta = TransformDAG::compile(&registry, &["delta"]).unwrap();

    for size in [1024usize, 16 * 1024, 64 * 1024] {
        let data = gen_structured(size);
        let ctx = default_ctx(&data, 0.1);
        let input = CpacType::Serial(data.clone());
        g.throughput(Throughput::Bytes(size as u64));

        g.bench_with_input(BenchmarkId::new("fwd_delta", size), &input, |b, inp| {
            b.iter(|| dag_delta.execute_forward(black_box(inp.clone()), black_box(&ctx)))
        });

        // Forward + backward roundtrip
        if let Ok((encoded, meta)) = dag_delta.execute_forward(input.clone(), &ctx) {
            g.bench_with_input(
                BenchmarkId::new("bwd_delta", size),
                &(encoded.clone(), meta.clone()),
                |b, (enc, m)| {
                    b.iter(|| dag_delta.execute_backward(black_box(enc.clone()), black_box(m)))
                },
            );
        }
    }

    // Multi-step: delta + zigzag
    let dag_dz = TransformDAG::compile(&registry, &["delta", "zigzag"]).unwrap();

    for size in [1024usize, 16 * 1024] {
        let data = gen_text(size);
        let ctx = default_ctx(&data, 0.95);
        let input = CpacType::Serial(data.clone());
        g.throughput(Throughput::Bytes(size as u64));

        g.bench_with_input(
            BenchmarkId::new("fwd_delta_zigzag", size),
            &input,
            |b, inp| b.iter(|| dag_dz.execute_forward(black_box(inp.clone()), black_box(&ctx))),
        );

        if let Ok((encoded, meta)) = dag_dz.execute_forward(input.clone(), &ctx) {
            g.bench_with_input(
                BenchmarkId::new("bwd_delta_zigzag", size),
                &(encoded.clone(), meta.clone()),
                |b, (enc, m)| {
                    b.iter(|| dag_dz.execute_backward(black_box(enc.clone()), black_box(m)))
                },
            );
        }
    }

    g.finish();
}

// ── Profile cache lookup benchmarks ─────────────────────────────────────

fn bench_profile_cache(c: &mut Criterion) {
    let cache = ProfileCache::with_builtins();
    let mut g = c.benchmark_group("dag_profile_cache");

    g.bench_function("lookup_generic", |b| {
        b.iter(|| cache.get_profile(black_box("generic")))
    });

    g.bench_function("list_names", |b| b.iter(|| cache.profile_names()));

    g.finish();
}

criterion_group!(
    benches,
    bench_dag_compile,
    bench_dag_auto_select,
    bench_dag_execute,
    bench_profile_cache
);
criterion_main!(benches);
