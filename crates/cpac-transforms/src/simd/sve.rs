// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! ARM SVE/SVE2 SIMD kernels for scalable vector operations.
//!
//! SVE provides scalable vectors that work across different vector lengths,
//! future-proofing for Neoverse V2, Graviton4, and beyond.

#![cfg(target_arch = "aarch64")]

use cpac_types::CpacResult;

/// Check if SVE is available at runtime.
pub fn is_sve_available() -> bool {
    #[cfg(target_feature = "sve")]
    {
        std::arch::is_aarch64_feature_detected!("sve")
    }
    #[cfg(not(target_feature = "sve"))]
    {
        false
    }
}

/// Check if SVE2 is available at runtime.
pub fn is_sve2_available() -> bool {
    #[cfg(target_feature = "sve2")]
    {
        std::arch::is_aarch64_feature_detected!("sve2")
    }
    #[cfg(not(target_feature = "sve2"))]
    {
        false
    }
}

/// Delta encode using SVE (scalable vector extension).
///
/// Note: This is a placeholder implementation. Actual SVE intrinsics
/// require nightly Rust and are architecture-specific.
#[cfg(target_feature = "sve")]
pub fn delta_encode_sve(data: &[u8], stride: usize) -> CpacResult<Vec<u8>> {
    // Fallback to scalar for now - SVE intrinsics not stable in Rust yet
    crate::delta::delta_encode_scalar(data, stride)
}

/// Delta encode fallback (SVE not available).
#[cfg(not(target_feature = "sve"))]
pub fn delta_encode_sve(data: &[u8], stride: usize) -> CpacResult<Vec<u8>> {
    crate::delta::delta_encode_scalar(data, stride)
}

/// Delta decode using SVE.
#[cfg(target_feature = "sve")]
pub fn delta_decode_sve(data: &[u8], stride: usize) -> CpacResult<Vec<u8>> {
    crate::delta::delta_decode_scalar(data, stride)
}

#[cfg(not(target_feature = "sve"))]
pub fn delta_decode_sve(data: &[u8], stride: usize) -> CpacResult<Vec<u8>> {
    crate::delta::delta_decode_scalar(data, stride)
}

/// ZigZag encode using SVE.
#[cfg(target_feature = "sve")]
pub fn zigzag_encode_sve(data: &[i64]) -> Vec<u64> {
    crate::zigzag::zigzag_encode_scalar(data)
}

#[cfg(not(target_feature = "sve"))]
pub fn zigzag_encode_sve(data: &[i64]) -> Vec<u64> {
    crate::zigzag::zigzag_encode_scalar(data)
}

/// ZigZag decode using SVE.
#[cfg(target_feature = "sve")]
pub fn zigzag_decode_sve(data: &[u64]) -> Vec<i64> {
    crate::zigzag::zigzag_decode_scalar(data)
}

#[cfg(not(target_feature = "sve"))]
pub fn zigzag_decode_sve(data: &[u64]) -> Vec<i64> {
    crate::zigzag::zigzag_decode_scalar(data)
}

// ---------------------------------------------------------------------------
// SVE Implementation Notes
// ---------------------------------------------------------------------------
//
// ARM SVE intrinsics in Rust are currently unstable and require:
// 1. Nightly Rust with #![feature(stdarch_aarch64_sve)]
// 2. Target CPU with SVE support (Neoverse V1/V2, Graviton3+)
// 3. Explicit RUSTFLAGS="-C target-cpu=neoverse-v1"
//
// Example SVE delta encoding (pseudo-code):
//
// ```rust
// use std::arch::aarch64::*;
//
// unsafe {
//     let vl = svcntb(); // Get vector length in bytes
//     let mut i = 0;
//     let mut prev = svdup_n_u8(0);
//     
//     while i < data.len() {
//         let pg = svwhilelt_b8_u64(i as u64, data.len() as u64);
//         let curr = svld1_u8(pg, data.as_ptr().add(i));
//         let delta = svsub_u8_z(pg, curr, prev);
//         svst1_u8(pg, output.as_mut_ptr().add(i), delta);
//         prev = svlasta_u8(pg, curr); // Last active element
//         i += svcntb();
//     }
// }
// ```
//
// For production deployment:
// - Gate SVE code behind runtime detection (is_sve_available())
// - Provide NEON fallback for older ARM64 chips
// - Use feature flags to conditionally compile SVE paths
// - Test on actual Graviton3/Neoverse V1 hardware

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sve_availability() {
        // Just check that detection doesn't panic
        let _sve = is_sve_available();
        let _sve2 = is_sve2_available();
    }

    #[test]
    fn sve_delta_fallback() {
        let data = vec![1, 2, 3, 4, 5];
        let encoded = delta_encode_sve(&data, 1).unwrap();
        let decoded = delta_decode_sve(&encoded, 1).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn sve_zigzag_fallback() {
        let data: Vec<i64> = vec![-5, -1, 0, 1, 5];
        let encoded = zigzag_encode_sve(&data);
        let decoded = zigzag_decode_sve(&encoded);
        assert_eq!(decoded, data);
    }
}
