// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Property-based tests using proptest.
//!
//! ∀ data: decompress(compress(data)) == data

use cpac_engine::{compress, decompress, Backend, CompressConfig};
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Full pipeline roundtrip
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn roundtrip_arbitrary_bytes(data in proptest::collection::vec(any::<u8>(), 0..8192)) {
        let config = CompressConfig::default();
        let compressed = compress(&data, &config).unwrap();
        let decompressed = decompress(&compressed.data).unwrap();
        prop_assert_eq!(&decompressed.data, &data);
    }

    #[test]
    fn roundtrip_zstd(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let config = CompressConfig { backend: Some(Backend::Zstd), ..Default::default() };
        let compressed = compress(&data, &config).unwrap();
        let decompressed = decompress(&compressed.data).unwrap();
        prop_assert_eq!(&decompressed.data, &data);
    }

    #[test]
    fn roundtrip_brotli(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let config = CompressConfig { backend: Some(Backend::Brotli), ..Default::default() };
        let compressed = compress(&data, &config).unwrap();
        let decompressed = decompress(&compressed.data).unwrap();
        prop_assert_eq!(&decompressed.data, &data);
    }

    #[test]
    fn roundtrip_raw(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let config = CompressConfig { backend: Some(Backend::Raw), ..Default::default() };
        let compressed = compress(&data, &config).unwrap();
        let decompressed = decompress(&compressed.data).unwrap();
        prop_assert_eq!(&decompressed.data, &data);
    }
}

// ---------------------------------------------------------------------------
// Per-transform roundtrip
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn delta_roundtrip(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let encoded = cpac_transforms::delta::delta_encode(&data);
        let decoded = cpac_transforms::delta::delta_decode(&encoded);
        prop_assert_eq!(&decoded, &data);
    }

    #[test]
    fn zigzag_scalar_roundtrip(val in any::<i64>()) {
        let encoded = cpac_transforms::zigzag::zigzag_encode(val);
        let decoded = cpac_transforms::zigzag::zigzag_decode(encoded);
        prop_assert_eq!(decoded, val);
    }

    #[test]
    fn zigzag_batch_roundtrip(values in proptest::collection::vec(any::<i64>(), 0..500)) {
        let encoded = cpac_transforms::zigzag::zigzag_encode_batch(&values);
        let (decoded, _consumed) = cpac_transforms::zigzag::zigzag_decode_batch(&encoded).unwrap();
        prop_assert_eq!(&decoded, &values);
    }

    #[test]
    fn transpose_roundtrip(
        width in 2usize..=16,
        rows in 4usize..=256,
    ) {
        let n = width * rows;
        let data: Vec<u8> = (0u8..=255).cycle().take(n).collect();
        let encoded = cpac_transforms::transpose::transpose_encode(&data, width).unwrap();
        let decoded = cpac_transforms::transpose::transpose_decode(&encoded, width).unwrap();
        prop_assert_eq!(&decoded, &data);
    }
}

// ---------------------------------------------------------------------------
// Frame format roundtrip
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn frame_encode_decode_roundtrip(
        payload in proptest::collection::vec(any::<u8>(), 0..1024),
        backend_id in 0u8..=2,
        original_size in 0u32..=1_000_000,
    ) {
        let backend = cpac_types::Backend::from_id(backend_id).unwrap();
        let dag_desc: Vec<u8> = Vec::new();
        let frame = cpac_frame::encode_frame(&payload, backend, original_size as usize, &dag_desc);
        let (header, decoded_payload) = cpac_frame::decode_frame(&frame).unwrap();
        prop_assert_eq!(decoded_payload, &payload[..]);
        prop_assert_eq!(header.backend, backend);
        prop_assert_eq!(header.original_size as usize, original_size as usize);
    }
}
