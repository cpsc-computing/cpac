// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Generate golden vectors for individual transforms.

#![allow(clippy::useless_vec)]

use std::fs;
use std::path::Path;

const FIXTURE_DIR: &str = "tests/fixtures/transforms";

fn setup_fixture_dir() {
    fs::create_dir_all(FIXTURE_DIR).unwrap();
}

/// Generate test data for each transform type.
fn generate_test_data() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        // Delta transform test (sequential integers)
        ("delta_sequential", (0u8..=255).cycle().take(512).collect()),
        // Zigzag transform test (signed-like data)
        (
            "zigzag_signed",
            vec![0x00, 0xFF, 0x01, 0xFE, 0x02, 0xFD].repeat(32),
        ),
        // Transpose test (structured data)
        ("transpose_matrix", (0u8..=15).cycle().take(256).collect()),
        // ROLZ test (repetitive text)
        (
            "rolz_text",
            b"the quick brown fox jumps over the lazy dog ".repeat(10),
        ),
        // Float split test (float data as bytes)
        ("float_split", {
            let floats: Vec<f32> = (0..64).map(|i| i as f32 * 0.1).collect();
            floats.iter().flat_map(|f| f.to_le_bytes()).collect()
        }),
        // Field LZ test (columnar data)
        (
            "field_lz_columnar",
            b"ID,Name,Value\n1,Alice,100\n2,Bob,200\n3,Carol,300\n".to_vec(),
        ),
        // Range pack test (small range integers)
        (
            "range_pack_small",
            vec![1, 2, 3, 4, 5, 1, 2, 3, 4, 5].repeat(10),
        ),
        // Tokenize test (repeated tokens)
        (
            "tokenize_words",
            b"hello world hello world hello world".to_vec(),
        ),
        // Prefix test (common prefixes)
        (
            "prefix_paths",
            b"/usr/local/bin\n/usr/local/lib\n/usr/local/share\n".to_vec(),
        ),
        // Dedup test (duplicate blocks)
        (
            "dedup_blocks",
            b"BLOCKBLOCKBLOCKBLOCKBLOCKBLOCKBLOCKBLOCKBLOCKBLOCK".to_vec(),
        ),
        // Parse int test (numeric strings)
        (
            "parse_int_numbers",
            b"123\n456\n789\n123\n456\n789\n".to_vec(),
        ),
    ]
}

#[test]
#[ignore]
fn generate_all_transform_goldens() {
    use cpac_engine::compress;
    use cpac_types::{Backend, CompressConfig};

    setup_fixture_dir();

    let test_data = generate_test_data();
    let count = test_data.len();

    for (name, data) in test_data {
        println!("Generating golden vector: {}", name);

        // Generate with all backends
        for backend in &[Backend::Raw, Backend::Zstd, Backend::Brotli] {
            let config = CompressConfig {
                backend: Some(*backend),
                ..Default::default()
            };

            let result = compress(&data, &config).unwrap();

            let filename = format!("{}_{:?}.cpac", name, backend);
            let path = Path::new(FIXTURE_DIR).join(&filename);
            fs::write(&path, &result.data).unwrap();

            println!(
                "  ✓ {} ({} -> {} bytes, {:.2}x)",
                filename,
                result.original_size,
                result.compressed_size,
                result.ratio()
            );
        }
    }

    println!(
        "\n✅ Generated {} golden vectors in {}",
        count * 3,
        FIXTURE_DIR
    );
}

#[test]
fn validate_transform_goldens() {
    use cpac_engine::decompress;

    let fixture_dir = Path::new(FIXTURE_DIR);
    if !fixture_dir.exists() {
        println!(
            "Skipping: {} not found (run generate test first)",
            FIXTURE_DIR
        );
        return;
    }

    let mut count = 0;
    for entry in fs::read_dir(fixture_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) != Some("cpac") {
            continue;
        }

        let data = fs::read(&path).unwrap();
        let result = decompress(&data);

        assert!(
            result.is_ok(),
            "Failed to decompress {}: {:?}",
            path.display(),
            result.err()
        );

        count += 1;
    }

    assert!(count > 0, "No golden vectors found in {}", FIXTURE_DIR);
    println!("✓ Validated {} transform golden vectors", count);
}
