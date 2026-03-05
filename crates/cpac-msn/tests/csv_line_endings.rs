// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Test CSV domain with different line endings.

use cpac_msn::{extract, reconstruct};

#[test]
fn csv_unix_line_endings() {
    let data = b"name,age\nAlice,30\nBob,25\n";

    let result = extract(data, None, 0.5).unwrap();
    assert!(result.applied, "CSV should be detected");

    let reconstructed = reconstruct(&result).unwrap();
    assert_eq!(reconstructed, data, "Roundtrip should be lossless");
}

#[test]
fn csv_no_trailing_newline() {
    let data = b"name,age\nAlice,30\nBob,25";

    let result = extract(data, None, 0.5).unwrap();
    assert!(result.applied, "CSV should be detected");

    let reconstructed = reconstruct(&result).unwrap();
    assert_eq!(reconstructed, data, "Roundtrip should be lossless");
}

#[test]
fn csv_windows_line_endings() {
    let data = b"name,age\r\nAlice,30\r\nBob,25\r\n";

    let result = extract(data, None, 0.5).unwrap();
    assert!(result.applied, "CSV should be detected");

    let reconstructed = reconstruct(&result).unwrap();
    assert_eq!(
        reconstructed.len(),
        data.len(),
        "Roundtrip should preserve length"
    );
    assert_eq!(reconstructed, data, "Roundtrip should be lossless");
}
