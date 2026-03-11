// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Test that MessagePack domain doesn't false positive on plain ASCII text.

use cpac_msn::{extract, reconstruct};

#[test]
fn msgpack_should_not_detect_plain_text() {
    // Plain ASCII text - should NOT be detected as MessagePack
    let plain_text = b"test data with repeated patterns ".repeat(100);

    println!("Testing plain text: {} bytes", plain_text.len());

    // Try extraction with default confidence (0.5)
    let result = extract(&plain_text, None, 0.5).unwrap();

    println!("MSN result:");
    println!("  Applied: {}", result.applied);
    println!("  Domain ID: {:?}", result.domain_id);
    println!("  Confidence: {}", result.confidence);
    println!("  Residual size: {} bytes", result.residual.len());

    // Plain text should NOT trigger MSN (should passthrough).
    // With the zero-copy `not_applied()` sentinel, `residual` is empty and
    // the caller is expected to use the original data directly.
    assert!(
        !result.applied,
        "Plain text should not be detected as MessagePack"
    );
    assert!(
        result.residual.is_empty(),
        "not_applied() sentinel should have empty residual"
    );
}

#[test]
fn msgpack_roundtrip_plain_text_forced() {
    // Even if we somehow force MessagePack detection, roundtrip should work
    let plain_text = b"test data with repeated patterns ".repeat(10);

    // Extract with very low confidence threshold (no domain hint)
    let result = extract(&plain_text, None, 0.1).unwrap();

    // If it was applied, verify roundtrip
    if result.applied {
        let reconstructed = reconstruct(&result).unwrap();
        assert_eq!(
            reconstructed.len(),
            plain_text.len(),
            "Roundtrip should preserve data length"
        );
        assert_eq!(reconstructed, plain_text, "Roundtrip should be lossless");
    }
}
