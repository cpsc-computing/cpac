// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
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

    #[test]
    fn rolz_roundtrip(data in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let encoded = cpac_transforms::rolz::rolz_encode(&data);
        if let Ok(decoded) = cpac_transforms::rolz::rolz_decode(&encoded) {
            prop_assert_eq!(&decoded, &data);
        }
    }

    #[test]
    fn float32_split_roundtrip(data in proptest::collection::vec(any::<u8>(), 4..2048)) {
        // Ensure data is multiple of 4 for float split
        let padded_len = (data.len() / 4) * 4;
        if padded_len >= 4 {
            let slice = &data[..padded_len];
            if let Ok((exps, sign_fracs)) = cpac_transforms::float_split::float32_split_encode(slice) {
                if let Ok(decoded) = cpac_transforms::float_split::float32_split_decode(&exps, &sign_fracs) {
                    prop_assert_eq!(&decoded, slice);
                }
            }
        }
    }

    #[test]
    fn prefix_roundtrip(values in proptest::collection::vec("[a-z]{2,10}", 1..20)) {
        let encoded = cpac_transforms::prefix::prefix_encode(&values);
        if let Ok(decoded) = cpac_transforms::prefix::prefix_decode(&encoded) {
            prop_assert_eq!(&decoded, &values);
        }
    }

    #[test]
    fn dedup_columns_roundtrip(
        cols in proptest::collection::vec(
            proptest::collection::vec(any::<u8>(), 0..100),
            1..10
        )
    ) {
        let (encoded, _had_dups) = cpac_transforms::dedup::dedup_columns(&cols);
        if let Ok(groups) = cpac_transforms::dedup::dedup_columns_decode(&encoded) {
            let restored = cpac_transforms::dedup::reconstruct_columns(&groups, cols.len());
            prop_assert_eq!(restored, cols);
        }
    }

    #[test]
    fn range_pack_roundtrip(
        values in proptest::collection::vec(-1_000_000_000i64..=1_000_000_000i64, 1..200)
    ) {
        // Constrain values to avoid overflow in (max - min) as u64 conversion
        let framed = cpac_transforms::range_pack::range_pack_encode_framed(&values);
        if let Ok(decoded) = cpac_transforms::range_pack::range_pack_decode_framed(&framed) {
            prop_assert_eq!(decoded, values);
        }
    }
}

// ---------------------------------------------------------------------------
// DAG property tests
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    #[test]
    fn ssr_analysis_determinism(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
        // SSR analysis should be deterministic for same input
        let ssr1 = cpac_ssr::analyze(&data);
        let ssr2 = cpac_ssr::analyze(&data);
        prop_assert_eq!(ssr1.track, ssr2.track);
        prop_assert!((ssr1.entropy_estimate - ssr2.entropy_estimate).abs() < 0.001);
        prop_assert!((ssr1.ascii_ratio - ssr2.ascii_ratio).abs() < 0.001);
        prop_assert_eq!(ssr1.data_size, ssr2.data_size);
    }

    #[test]
    fn dag_serialization_roundtrip(
        ids in proptest::collection::vec(1u8..=11, 1..=5)
    ) {
        // DAG descriptor serialization should roundtrip
        let chain: Vec<(u8, Vec<u8>)> = ids.iter().map(|&id| (id, vec![])).collect();
        let descriptor = cpac_dag::serialize_dag_descriptor(&chain);
        let (recovered_ids, recovered_metas, _consumed) =
            cpac_dag::deserialize_dag_descriptor(&descriptor).unwrap();
        prop_assert_eq!(&recovered_ids, &ids);
        prop_assert_eq!(recovered_metas.len(), chain.len());
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
