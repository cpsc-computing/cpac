//! Phase 1 investigation: smart transforms at parallel-block sizes.
//!
//! These tests simulate what a parallel sub-block would experience if
//! `skip_expensive_transforms` were removed.  Each test compresses a
//! single block at parallel block size (~17 MB) via the normal single-
//! stream compress/decompress path with smart transforms enabled.

use cpac_engine::{compress, decompress, Backend, CompressConfig};

/// Simulate a parallel block: 17 MB of repetitive text, single-stream
/// with smart transforms + BWT enabled.
#[test]
fn roundtrip_block_size_text_with_smart_transforms() {
    let sentence = b"The quick brown fox jumps over the lazy dog. ";
    // 17 MB — the approximate per-block size the parallel path would create
    let block_size = 17 * 1024 * 1024;
    let data: Vec<u8> = sentence
        .iter()
        .copied()
        .cycle()
        .take(block_size)
        .collect();

    let config = CompressConfig {
        backend: Some(Backend::Zstd),
        enable_smart_transforms: true,
        disable_parallel: true, // force single-stream
        ..Default::default()
    };
    let compressed = compress(&data, &config).unwrap();
    let decompressed = decompress(&compressed.data).unwrap();
    assert_eq!(
        decompressed.data.len(),
        data.len(),
        "block-size smart transform size mismatch"
    );
    if decompressed.data != data {
        let pos = decompressed
            .data
            .iter()
            .zip(data.iter())
            .position(|(a, b)| a != b)
            .unwrap_or(0);
        panic!(
            "block-size smart transform roundtrip failed at byte {}/{}: got {} expected {}",
            pos,
            data.len(),
            decompressed.data[pos],
            data[pos]
        );
    }
}

/// Same as above but with structured JSON data (higher normalize benefit).
#[test]
fn roundtrip_block_size_json_with_smart_transforms() {
    let record = br#"{"name": "Alice", "age": 30, "city": "New York", "active": true}
"#;
    let block_size = 17 * 1024 * 1024;
    let data: Vec<u8> = record
        .iter()
        .copied()
        .cycle()
        .take(block_size)
        .collect();

    let config = CompressConfig {
        backend: Some(Backend::Zstd),
        enable_smart_transforms: true,
        disable_parallel: true,
        ..Default::default()
    };
    let compressed = compress(&data, &config).unwrap();
    let decompressed = decompress(&compressed.data).unwrap();
    assert_eq!(
        decompressed.data.len(),
        data.len(),
        "block-size JSON smart transform size mismatch"
    );
    assert_eq!(
        decompressed.data, data,
        "block-size JSON smart transform content mismatch"
    );
}
