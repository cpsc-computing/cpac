// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! MSN domain handler benchmarks.

use cpac_msn::domains::*;
use cpac_msn::{extract, Domain};
use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;

fn json_extraction(c: &mut Criterion) {
    let mut group = c.benchmark_group("json");

    // Single JSON object (not JSONL)
    let json_data = r#"{"items":[{"name":"Alice","age":30},{"name":"Bob","age":25},{"name":"Charlie","age":35}]}"#;

    group.bench_function("extract", |b| {
        b.iter(|| {
            let domain = JsonDomain;
            black_box(domain.extract(black_box(json_data.as_bytes())).unwrap());
        });
    });

    let domain = JsonDomain;
    let result = domain.extract(json_data.as_bytes()).unwrap();

    group.bench_function("reconstruct", |b| {
        b.iter(|| {
            black_box(domain.reconstruct(black_box(&result)).unwrap());
        });
    });

    group.finish();
}

fn jsonlog_extraction(c: &mut Criterion) {
    let mut group = c.benchmark_group("jsonlog");

    // JSONL data (newline-delimited JSON)
    let jsonl_data = r#"{"name":"Alice","age":30,"city":"NYC"}
{"name":"Bob","age":25,"city":"LA"}
{"name":"Charlie","age":35,"city":"SF"}
{"name":"Diana","age":28,"city":"NYC"}
{"name":"Eve","age":32,"city":"LA"}"#;

    group.bench_function("extract", |b| {
        b.iter(|| {
            let domain = JsonLogDomain;
            black_box(domain.extract(black_box(jsonl_data.as_bytes())).unwrap());
        });
    });

    let domain = JsonLogDomain;
    let result = domain.extract(jsonl_data.as_bytes()).unwrap();

    group.bench_function("reconstruct", |b| {
        b.iter(|| {
            black_box(domain.reconstruct(black_box(&result)).unwrap());
        });
    });

    group.finish();
}

fn csv_extraction(c: &mut Criterion) {
    let mut group = c.benchmark_group("csv");

    let csv_data = b"name,age,city\nAlice,30,NYC\nBob,25,LA\nCharlie,35,SF\nDiana,28,NYC\nEve,32,LA\nFrank,29,NYC\nGrace,31,SF\nHenry,27,LA\nIris,33,NYC\nJack,26,SF";

    group.bench_function("extract", |b| {
        b.iter(|| {
            let domain = CsvDomain;
            black_box(domain.extract(black_box(csv_data)).unwrap());
        });
    });

    group.finish();
}

fn xml_extraction(c: &mut Criterion) {
    let mut group = c.benchmark_group("xml");

    let xml_data = b"<root><item><name>Alice</name><age>30</age></item><item><name>Bob</name><age>25</age></item><item><name>Charlie</name><age>35</age></item></root>";

    group.bench_function("extract", |b| {
        b.iter(|| {
            let domain = XmlDomain;
            black_box(domain.extract(black_box(xml_data)).unwrap());
        });
    });

    group.finish();
}

fn msn_auto_detect(c: &mut Criterion) {
    let mut group = c.benchmark_group("auto_detect");

    let jsonl_data = r#"{"name":"Alice","age":30}
{"name":"Bob","age":25}
{"name":"Charlie","age":35}"#;

    group.bench_function("jsonl", |b| {
        b.iter(|| {
            black_box(extract(black_box(jsonl_data.as_bytes()), None, 0.5).unwrap());
        });
    });

    group.finish();
}

fn compression_ratios(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression_ratios");

    // JSONL data
    let jsonl_data = r#"{"name":"Alice","age":30,"city":"NYC","status":"active","role":"admin"}
{"name":"Bob","age":25,"city":"LA","status":"active","role":"user"}
{"name":"Charlie","age":35,"city":"SF","status":"inactive","role":"admin"}
{"name":"Diana","age":28,"city":"NYC","status":"active","role":"user"}
{"name":"Eve","age":32,"city":"LA","status":"active","role":"admin"}"#;

    group.bench_function("jsonl_ratio", |b| {
        b.iter(|| {
            let domain = JsonLogDomain;
            let result = domain.extract(jsonl_data.as_bytes()).unwrap();
            let orig_size = jsonl_data.len();
            let compressed_size = result.residual.len();
            black_box((orig_size, compressed_size));
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    json_extraction,
    jsonlog_extraction,
    csv_extraction,
    xml_extraction,
    msn_auto_detect,
    compression_ratios
);
criterion_main!(benches);
