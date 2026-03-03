// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Cross-engine regression tests.
//!
//! These tests validate that the Rust engine produces output compatible with
//! golden test fixtures. The `#[ignore]` Python subprocess tests can be
//! enabled when the Python engine is available on PATH.

use cpac_engine::{compress, decompress, CompressConfig};
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

// ---------------------------------------------------------------------------
// Golden fixture roundtrip (Rust → Rust)
// ---------------------------------------------------------------------------

fn read_fixture(name: &str) -> Vec<u8> {
    let path = fixtures_dir().join(name);
    std::fs::read(&path).unwrap_or_else(|_| panic!("missing fixture: {}", path.display()))
}

#[test]
fn fixture_hello_txt() {
    let data = read_fixture("hello.txt");
    let config = CompressConfig::default();
    let compressed = compress(&data, &config).unwrap();
    let decompressed = decompress(&compressed.data).unwrap();
    assert_eq!(decompressed.data, data);
}

#[test]
fn fixture_zeros_bin() {
    let data = read_fixture("zeros.bin");
    // Force Zstd to test compression (auto-select would pick Raw for low entropy)
    let config = CompressConfig {
        backend: Some(cpac_engine::Backend::Zstd),
        ..Default::default()
    };
    let compressed = compress(&data, &config).unwrap();
    let decompressed = decompress(&compressed.data).unwrap();
    assert_eq!(decompressed.data, data);
    // All zeros should compress extremely well with forced backend
    assert!(compressed.ratio() > 10.0, "ratio {} should be > 10.0 for zeros", compressed.ratio());
}

#[test]
fn fixture_csv_sample() {
    let data = read_fixture("sample.csv");
    let config = CompressConfig::default();
    let compressed = compress(&data, &config).unwrap();
    let decompressed = decompress(&compressed.data).unwrap();
    assert_eq!(decompressed.data, data);
}

// ---------------------------------------------------------------------------
// Cross-engine: Python subprocess (requires `cpac` Python CLI on PATH)
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires Python cpac engine on PATH"]
fn cross_engine_python_compress_rust_decompress() {
    use std::process::Command;

    let data = read_fixture("hello.txt");
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), &data).unwrap();

    let out_path = tmp.path().with_extension("cpac");
    let status = Command::new("python")
        .args(["-m", "cpac", "compress"])
        .arg(tmp.path())
        .arg("-o")
        .arg(&out_path)
        .status()
        .expect("failed to run Python cpac");
    assert!(status.success(), "Python compress failed");

    let compressed = std::fs::read(&out_path).unwrap();
    let decompressed = decompress(&compressed).unwrap();
    assert_eq!(decompressed.data, data, "cross-engine roundtrip failed");
}

#[test]
#[ignore = "requires Python cpac engine on PATH"]
fn cross_engine_rust_compress_python_decompress() {
    use std::process::Command;

    let data = read_fixture("hello.txt");
    let config = CompressConfig::default();
    let compressed = compress(&data, &config).unwrap();

    let tmp_cpac = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp_cpac.path(), &compressed.data).unwrap();

    let out_path = tmp_cpac.path().with_extension("out");
    let status = Command::new("python")
        .args(["-m", "cpac", "decompress"])
        .arg(tmp_cpac.path())
        .arg("-o")
        .arg(&out_path)
        .status()
        .expect("failed to run Python cpac");
    assert!(status.success(), "Python decompress failed");

    let result = std::fs::read(&out_path).unwrap();
    assert_eq!(result, data, "cross-engine roundtrip failed");
}
