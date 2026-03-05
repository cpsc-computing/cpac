// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Proptest-based fuzz equivalents.
//!
//! These tests exercise the same code paths as the libfuzzer targets in
//! `fuzz/fuzz_targets/`, but run on stable Rust and Windows. Each test
//! generates randomized inputs via proptest and verifies that the code
//! under test never panics and maintains its invariants.

use cpac_engine::{compress, decompress, CompressConfig};
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Fuzz-equivalent: roundtrip (fuzz_roundtrip)
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// Arbitrary data compressed then decompressed must equal original.
    #[test]
    fn fuzz_roundtrip(data in proptest::collection::vec(any::<u8>(), 0..8192)) {
        let config = CompressConfig::default();
        if let Ok(compressed) = compress(&data, &config) {
            let decompressed = decompress(&compressed.data)
                .expect("decompression of valid compressed data must succeed");
            prop_assert_eq!(&decompressed.data, &data);
        }
        // Compression failing on some inputs is acceptable (e.g. empty)
    }
}

// ---------------------------------------------------------------------------
// Fuzz-equivalent: decompress random bytes (fuzz_decompress)
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Random bytes fed to decompress must never panic — only Ok or Err.
    #[test]
    fn fuzz_decompress(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = decompress(&data);
    }
}

// ---------------------------------------------------------------------------
// Fuzz-equivalent: CAS roundtrip (fuzz_cas_roundtrip)
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// CAS compress → decompress must return original bytes.
    #[test]
    fn fuzz_cas_roundtrip(data in proptest::collection::vec(any::<u8>(), 1..4096)) {
        let compressed = cpac_cas::cas_compress(&data);
        let decompressed = cpac_cas::cas_decompress(&compressed)
            .expect("CAS decompression of valid CAS frame must succeed");
        prop_assert_eq!(&decompressed, &data);
    }
}

// ---------------------------------------------------------------------------
// Fuzz-equivalent: archive decode (fuzz_archive_decode)
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Random bytes fed to list_archive must never panic — only Ok or Err.
    #[test]
    fn fuzz_archive_decode(data in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let _ = cpac_archive::list_archive(&data);
    }
}

// ---------------------------------------------------------------------------
// Fuzz-equivalent: frame decode (fuzz_frame_decode)
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Random bytes with CPAC magic prefix fed to decompress — no panic.
    #[test]
    fn fuzz_frame_with_magic(
        tail in proptest::collection::vec(any::<u8>(), 0..2048)
    ) {
        let mut data = b"CP".to_vec();
        data.extend_from_slice(&tail);
        let _ = decompress(&data);
    }
}

// ---------------------------------------------------------------------------
// Fuzz-equivalent: CAS with structured patterns
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Constraint inference on random integer columns must never panic.
    #[test]
    fn fuzz_cas_inference(
        values in proptest::collection::vec(-1000i64..1000, 2..500)
    ) {
        let cols = vec![("fuzz_col".to_string(), values)];
        let analysis = cpac_cas::analyze_columns(&cols);
        // Sanity: total DoF must be non-negative
        prop_assert!(analysis.total_dof >= 0.0);
        prop_assert!(analysis.constrained_dof >= 0.0);
        prop_assert!(analysis.estimated_benefit >= 0.0);
        prop_assert!(analysis.estimated_benefit <= 1.0);
    }
}

// ---------------------------------------------------------------------------
// Fuzz-equivalent: archive with CPAR magic prefix
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Random bytes with CPAR magic prefix — must not panic.
    #[test]
    fn fuzz_archive_with_magic(
        tail in proptest::collection::vec(any::<u8>(), 0..1024)
    ) {
        let mut data = b"CPAR".to_vec();
        data.extend_from_slice(&tail);
        let _ = cpac_archive::list_archive(&data);
    }
}
