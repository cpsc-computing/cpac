// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! SIMD-accelerated transform kernels.
//!
//! Runtime dispatch hierarchy (highest → lowest):
//! **`x86_64`** — AVX-512 → AVX2 → SSE4.1 → SSE2 → scalar
//! **aarch64** — NEON → scalar
//!
//! Each public `*_fast` function probes CPU features once per call
//! (the checks are essentially free on modern CPUs via `CPUID` cache).

// ---------------------------------------------------------------------------
// Dispatch: pick best available implementation at runtime
// ---------------------------------------------------------------------------

/// SIMD-accelerated byte-level delta encode.
///
/// Dispatch: AVX-512 (64 B) → AVX2 (32 B) → SSE2 (16 B) → NEON → scalar.
#[must_use]
pub fn delta_encode_fast(data: &[u8]) -> Vec<u8> {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx512f")
            && is_x86_feature_detected!("avx512bw")
            && data.len() >= 64
        {
            return unsafe { delta_encode_avx512(data) };
        }
        if is_x86_feature_detected!("avx2") && data.len() >= 32 {
            return unsafe { delta_encode_avx2(data) };
        }
        if is_x86_feature_detected!("sse2") && data.len() >= 16 {
            return unsafe { delta_encode_sse2(data) };
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        if data.len() >= 16 {
            return delta_encode_neon(data);
        }
    }
    crate::delta::delta_encode(data)
}

/// SIMD-accelerated byte-level delta decode.
///
/// Delta decode has a serial prefix-sum dependency so SIMD only helps
/// marginally.  Delegates to scalar for correctness.
#[must_use]
pub fn delta_decode_fast(data: &[u8]) -> Vec<u8> {
    crate::delta::delta_decode(data)
}

/// SIMD-accelerated transpose encode (row-major → column-major).
///
/// Uses AVX2 (32-byte) vectorized scatter when element width and data
/// alignment cooperate; falls back to scalar.
pub fn transpose_encode_fast(data: &[u8], element_width: usize) -> cpac_types::CpacResult<Vec<u8>> {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2")
            && data.len() >= 256
            && element_width <= 64
            && data.len().is_multiple_of(element_width)
        {
            let n_rows = data.len() / element_width;
            if n_rows >= 32 {
                return Ok(unsafe { transpose_encode_avx2(data, element_width) });
            }
        }
    }
    crate::transpose::transpose_encode(data, element_width)
}

/// SIMD-accelerated transpose decode (column-major → row-major).
pub fn transpose_decode_fast(data: &[u8], element_width: usize) -> cpac_types::CpacResult<Vec<u8>> {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2")
            && data.len() >= 256
            && element_width <= 64
            && data.len().is_multiple_of(element_width)
        {
            let num_elements = data.len() / element_width;
            if num_elements >= 32 {
                return Ok(unsafe { transpose_decode_avx2(data, element_width) });
            }
        }
    }
    crate::transpose::transpose_decode(data, element_width)
}

/// SIMD-accelerated zigzag encode.
///
/// Dispatch: AVX-512 (64 B) → AVX2 (32 B) → SSE4.1 (16 B) → SSE2 (16 B)
///           → NEON → scalar.
#[must_use]
pub fn zigzag_encode_fast(data: &[u8]) -> Vec<u8> {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx512f")
            && is_x86_feature_detected!("avx512bw")
            && data.len() >= 64
        {
            return unsafe { zigzag_encode_avx512(data) };
        }
        if is_x86_feature_detected!("avx2") && data.len() >= 32 {
            return unsafe { zigzag_encode_avx2(data) };
        }
        if is_x86_feature_detected!("sse4.1") && data.len() >= 16 {
            return unsafe { zigzag_encode_sse41(data) };
        }
        if is_x86_feature_detected!("sse2") && data.len() >= 16 {
            return unsafe { zigzag_encode_sse2(data) };
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        if data.len() >= 16 {
            return zigzag_encode_neon(data);
        }
    }
    zigzag_encode_scalar(data)
}

/// SIMD-accelerated zigzag decode.
#[must_use]
pub fn zigzag_decode_fast(data: &[u8]) -> Vec<u8> {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx512f")
            && is_x86_feature_detected!("avx512bw")
            && data.len() >= 64
        {
            return unsafe { zigzag_decode_avx512(data) };
        }
        if is_x86_feature_detected!("avx2") && data.len() >= 32 {
            return unsafe { zigzag_decode_avx2(data) };
        }
        if is_x86_feature_detected!("sse4.1") && data.len() >= 16 {
            return unsafe { zigzag_decode_sse41(data) };
        }
        if is_x86_feature_detected!("sse2") && data.len() >= 16 {
            return unsafe { zigzag_decode_sse2(data) };
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        if data.len() >= 16 {
            return zigzag_decode_neon(data);
        }
    }
    zigzag_decode_scalar(data)
}

// ---------------------------------------------------------------------------
// Scalar zigzag fallbacks
// ---------------------------------------------------------------------------

fn zigzag_encode_scalar(data: &[u8]) -> Vec<u8> {
    data.iter()
        .map(|&b| {
            let s = b as i8;
            ((s << 1) ^ (s >> 7)) as u8
        })
        .collect()
}

fn zigzag_decode_scalar(data: &[u8]) -> Vec<u8> {
    data.iter()
        .map(|&b| (b >> 1) ^ (b & 1).wrapping_neg())
        .collect()
}

// ===========================================================================
// x86_64  SSE2  delta encode  (16 bytes / iteration)
// ===========================================================================

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn delta_encode_sse2(data: &[u8]) -> Vec<u8> {
    use std::arch::x86_64::{__m128i, _mm_loadu_si128, _mm_storeu_si128, _mm_sub_epi8};
    let n = data.len();
    let mut out = vec![0u8; n];
    if n == 0 {
        return out;
    }
    out[0] = data[0];
    let mut i = 1usize;
    while i + 16 <= n {
        let cur = _mm_loadu_si128(data.as_ptr().add(i).cast::<__m128i>());
        let prev = _mm_loadu_si128(data.as_ptr().add(i - 1).cast::<__m128i>());
        _mm_storeu_si128(
            out.as_mut_ptr().add(i).cast::<__m128i>(),
            _mm_sub_epi8(cur, prev),
        );
        i += 16;
    }
    while i < n {
        out[i] = data[i].wrapping_sub(data[i - 1]);
        i += 1;
    }
    out
}

// ===========================================================================
// x86_64  AVX2  delta encode  (32 bytes / iteration)
// ===========================================================================

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn delta_encode_avx2(data: &[u8]) -> Vec<u8> {
    use std::arch::x86_64::{__m256i, _mm256_loadu_si256, _mm256_storeu_si256, _mm256_sub_epi8};
    let n = data.len();
    let mut out = vec![0u8; n];
    if n == 0 {
        return out;
    }
    out[0] = data[0];
    let mut i = 1usize;
    while i + 32 <= n {
        let cur = _mm256_loadu_si256(data.as_ptr().add(i).cast::<__m256i>());
        let prev = _mm256_loadu_si256(data.as_ptr().add(i - 1).cast::<__m256i>());
        _mm256_storeu_si256(
            out.as_mut_ptr().add(i).cast::<__m256i>(),
            _mm256_sub_epi8(cur, prev),
        );
        i += 32;
    }
    while i < n {
        out[i] = data[i].wrapping_sub(data[i - 1]);
        i += 1;
    }
    out
}

// ===========================================================================
// x86_64  AVX-512  delta encode  (64 bytes / iteration)
// ===========================================================================

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f,avx512bw")]
unsafe fn delta_encode_avx512(data: &[u8]) -> Vec<u8> {
    use std::arch::x86_64::{__m512i, _mm512_loadu_si512, _mm512_storeu_si512, _mm512_sub_epi8};
    let n = data.len();
    let mut out = vec![0u8; n];
    if n == 0 {
        return out;
    }
    out[0] = data[0];
    let mut i = 1usize;
    while i + 64 <= n {
        let cur = _mm512_loadu_si512(data.as_ptr().add(i).cast::<__m512i>());
        let prev = _mm512_loadu_si512(data.as_ptr().add(i - 1).cast::<__m512i>());
        _mm512_storeu_si512(
            out.as_mut_ptr().add(i).cast::<__m512i>(),
            _mm512_sub_epi8(cur, prev),
        );
        i += 64;
    }
    while i < n {
        out[i] = data[i].wrapping_sub(data[i - 1]);
        i += 1;
    }
    out
}

// ===========================================================================
// x86_64  AVX2  transpose encode / decode  (auto-vectorised loop body)
// ===========================================================================

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn transpose_encode_avx2(data: &[u8], element_width: usize) -> Vec<u8> {
    let n = data.len();
    let num_elements = n / element_width;
    let mut out = vec![0u8; n];
    for col in 0..element_width {
        let dst_offset = col * num_elements;
        for row in 0..num_elements {
            *out.get_unchecked_mut(dst_offset + row) =
                *data.get_unchecked(row * element_width + col);
        }
    }
    out
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn transpose_decode_avx2(data: &[u8], element_width: usize) -> Vec<u8> {
    let n = data.len();
    let num_elements = n / element_width;
    let mut out = vec![0u8; n];
    for col in 0..element_width {
        let src_offset = col * num_elements;
        for row in 0..num_elements {
            *out.get_unchecked_mut(row * element_width + col) =
                *data.get_unchecked(src_offset + row);
        }
    }
    out
}

// ===========================================================================
// x86_64  SSE2  zigzag encode / decode  (16 bytes / iteration)
// ===========================================================================

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn zigzag_encode_sse2(data: &[u8]) -> Vec<u8> {
    use std::arch::x86_64::{
        __m128i, _mm_add_epi8, _mm_cmpgt_epi8, _mm_loadu_si128, _mm_setzero_si128,
        _mm_storeu_si128, _mm_xor_si128,
    };
    let n = data.len();
    let mut out = vec![0u8; n];
    let mut i = 0usize;
    let zeros = _mm_setzero_si128();
    while i + 16 <= n {
        let v = _mm_loadu_si128(data.as_ptr().add(i).cast::<__m128i>());
        let shifted_left = _mm_add_epi8(v, v);
        let sign = _mm_cmpgt_epi8(zeros, v);
        _mm_storeu_si128(
            out.as_mut_ptr().add(i).cast::<__m128i>(),
            _mm_xor_si128(shifted_left, sign),
        );
        i += 16;
    }
    while i < n {
        let s = data[i] as i8;
        out[i] = ((s << 1) ^ (s >> 7)) as u8;
        i += 1;
    }
    out
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn zigzag_decode_sse2(data: &[u8]) -> Vec<u8> {
    use std::arch::x86_64::{
        __m128i, _mm_and_si128, _mm_loadu_si128, _mm_set1_epi8, _mm_setzero_si128, _mm_srli_epi16,
        _mm_storeu_si128, _mm_sub_epi8, _mm_xor_si128,
    };
    let n = data.len();
    let mut out = vec![0u8; n];
    let mut i = 0usize;
    let ones = _mm_set1_epi8(1);
    while i + 16 <= n {
        let v = _mm_loadu_si128(data.as_ptr().add(i).cast::<__m128i>());
        let shr = _mm_and_si128(_mm_srli_epi16(v, 1), _mm_set1_epi8(0x7F));
        let neg_low = _mm_sub_epi8(_mm_setzero_si128(), _mm_and_si128(v, ones));
        _mm_storeu_si128(
            out.as_mut_ptr().add(i).cast::<__m128i>(),
            _mm_xor_si128(shr, neg_low),
        );
        i += 16;
    }
    while i < n {
        let b = data[i];
        out[i] = (b >> 1) ^ (b & 1).wrapping_neg();
        i += 1;
    }
    out
}

// ===========================================================================
// x86_64  SSE4.1  zigzag encode / decode  (16 bytes, uses blendv)
// ===========================================================================

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse4.1")]
unsafe fn zigzag_encode_sse41(data: &[u8]) -> Vec<u8> {
    use std::arch::x86_64::{
        __m128i, _mm_add_epi8, _mm_blendv_epi8, _mm_loadu_si128, _mm_set1_epi8, _mm_setzero_si128,
        _mm_storeu_si128, _mm_xor_si128,
    };
    let n = data.len();
    let mut out = vec![0u8; n];
    let mut i = 0usize;
    let zeros = _mm_setzero_si128();
    while i + 16 <= n {
        let v = _mm_loadu_si128(data.as_ptr().add(i).cast::<__m128i>());
        let shifted_left = _mm_add_epi8(v, v);
        // SSE4.1 blendv: sign = 0xFF where v < 0 (signed), 0x00 otherwise
        let sign = _mm_blendv_epi8(zeros, _mm_set1_epi8(-1), v);
        _mm_storeu_si128(
            out.as_mut_ptr().add(i).cast::<__m128i>(),
            _mm_xor_si128(shifted_left, sign),
        );
        i += 16;
    }
    while i < n {
        let s = data[i] as i8;
        out[i] = ((s << 1) ^ (s >> 7)) as u8;
        i += 1;
    }
    out
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse4.1")]
unsafe fn zigzag_decode_sse41(data: &[u8]) -> Vec<u8> {
    use std::arch::x86_64::{
        __m128i, _mm_and_si128, _mm_loadu_si128, _mm_set1_epi8, _mm_setzero_si128, _mm_srli_epi16,
        _mm_storeu_si128, _mm_sub_epi8, _mm_xor_si128,
    };
    let n = data.len();
    let mut out = vec![0u8; n];
    let mut i = 0usize;
    let ones = _mm_set1_epi8(1);
    while i + 16 <= n {
        let v = _mm_loadu_si128(data.as_ptr().add(i).cast::<__m128i>());
        let shr = _mm_and_si128(_mm_srli_epi16(v, 1), _mm_set1_epi8(0x7F));
        let neg_low = _mm_sub_epi8(_mm_setzero_si128(), _mm_and_si128(v, ones));
        _mm_storeu_si128(
            out.as_mut_ptr().add(i).cast::<__m128i>(),
            _mm_xor_si128(shr, neg_low),
        );
        i += 16;
    }
    while i < n {
        let b = data[i];
        out[i] = (b >> 1) ^ (b & 1).wrapping_neg();
        i += 1;
    }
    out
}

// ===========================================================================
// x86_64  AVX2  zigzag encode / decode  (32 bytes / iteration)
// ===========================================================================

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn zigzag_encode_avx2(data: &[u8]) -> Vec<u8> {
    use std::arch::x86_64::{
        __m256i, _mm256_add_epi8, _mm256_cmpgt_epi8, _mm256_loadu_si256, _mm256_setzero_si256,
        _mm256_storeu_si256, _mm256_xor_si256,
    };
    let n = data.len();
    let mut out = vec![0u8; n];
    let mut i = 0usize;
    let zeros = _mm256_setzero_si256();
    while i + 32 <= n {
        let v = _mm256_loadu_si256(data.as_ptr().add(i).cast::<__m256i>());
        let shifted_left = _mm256_add_epi8(v, v);
        let sign = _mm256_cmpgt_epi8(zeros, v);
        _mm256_storeu_si256(
            out.as_mut_ptr().add(i).cast::<__m256i>(),
            _mm256_xor_si256(shifted_left, sign),
        );
        i += 32;
    }
    while i < n {
        let s = data[i] as i8;
        out[i] = ((s << 1) ^ (s >> 7)) as u8;
        i += 1;
    }
    out
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn zigzag_decode_avx2(data: &[u8]) -> Vec<u8> {
    use std::arch::x86_64::{
        __m256i, _mm256_and_si256, _mm256_loadu_si256, _mm256_set1_epi8, _mm256_setzero_si256,
        _mm256_srli_epi16, _mm256_storeu_si256, _mm256_sub_epi8, _mm256_xor_si256,
    };
    let n = data.len();
    let mut out = vec![0u8; n];
    let mut i = 0usize;
    let ones = _mm256_set1_epi8(1);
    let mask_7f = _mm256_set1_epi8(0x7F);
    while i + 32 <= n {
        let v = _mm256_loadu_si256(data.as_ptr().add(i).cast::<__m256i>());
        let shr = _mm256_and_si256(_mm256_srli_epi16(v, 1), mask_7f);
        let neg_low = _mm256_sub_epi8(_mm256_setzero_si256(), _mm256_and_si256(v, ones));
        _mm256_storeu_si256(
            out.as_mut_ptr().add(i).cast::<__m256i>(),
            _mm256_xor_si256(shr, neg_low),
        );
        i += 32;
    }
    while i < n {
        let b = data[i];
        out[i] = (b >> 1) ^ (b & 1).wrapping_neg();
        i += 1;
    }
    out
}

// ===========================================================================
// x86_64  AVX-512  zigzag encode / decode  (64 bytes / iteration)
// ===========================================================================

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f,avx512bw")]
unsafe fn zigzag_encode_avx512(data: &[u8]) -> Vec<u8> {
    use std::arch::x86_64::{
        __m512i, _mm512_add_epi8, _mm512_cmpgt_epi8_mask, _mm512_loadu_si512, _mm512_movm_epi8,
        _mm512_setzero_si512, _mm512_storeu_si512, _mm512_xor_si512,
    };
    let n = data.len();
    let mut out = vec![0u8; n];
    let mut i = 0usize;
    let zeros = _mm512_setzero_si512();
    while i + 64 <= n {
        let v = _mm512_loadu_si512(data.as_ptr().add(i).cast::<__m512i>());
        let shifted_left = _mm512_add_epi8(v, v);
        // Arithmetic sign: cmp > 0 gives mask of negative bytes
        let sign_mask = _mm512_cmpgt_epi8_mask(zeros, v);
        let sign = _mm512_movm_epi8(sign_mask);
        _mm512_storeu_si512(
            out.as_mut_ptr().add(i).cast::<__m512i>(),
            _mm512_xor_si512(shifted_left, sign),
        );
        i += 64;
    }
    while i < n {
        let s = data[i] as i8;
        out[i] = ((s << 1) ^ (s >> 7)) as u8;
        i += 1;
    }
    out
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f,avx512bw")]
unsafe fn zigzag_decode_avx512(data: &[u8]) -> Vec<u8> {
    use std::arch::x86_64::{
        __m512i, _mm512_and_si512, _mm512_loadu_si512, _mm512_set1_epi8, _mm512_setzero_si512,
        _mm512_srli_epi16, _mm512_storeu_si512, _mm512_sub_epi8, _mm512_xor_si512,
    };
    let n = data.len();
    let mut out = vec![0u8; n];
    let mut i = 0usize;
    let ones = _mm512_set1_epi8(1);
    let mask_7f = _mm512_set1_epi8(0x7F);
    while i + 64 <= n {
        let v = _mm512_loadu_si512(data.as_ptr().add(i).cast::<__m512i>());
        let shr = _mm512_and_si512(_mm512_srli_epi16(v, 1), mask_7f);
        let neg_low = _mm512_sub_epi8(_mm512_setzero_si512(), _mm512_and_si512(v, ones));
        _mm512_storeu_si512(
            out.as_mut_ptr().add(i).cast::<__m512i>(),
            _mm512_xor_si512(shr, neg_low),
        );
        i += 64;
    }
    while i < n {
        let b = data[i];
        out[i] = (b >> 1) ^ (b & 1).wrapping_neg();
        i += 1;
    }
    out
}

// ===========================================================================
// aarch64  NEON  stubs  (use scalar; real NEON intrinsics TODO)
// ===========================================================================

#[cfg(target_arch = "aarch64")]
fn delta_encode_neon(data: &[u8]) -> Vec<u8> {
    // TODO: implement with std::arch::aarch64 NEON intrinsics
    crate::delta::delta_encode(data)
}

#[cfg(target_arch = "aarch64")]
fn zigzag_encode_neon(data: &[u8]) -> Vec<u8> {
    zigzag_encode_scalar(data)
}

#[cfg(target_arch = "aarch64")]
fn zigzag_decode_neon(data: &[u8]) -> Vec<u8> {
    zigzag_decode_scalar(data)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_fast_roundtrip() {
        let data: Vec<u8> = (0..256).map(|i| (i % 256) as u8).collect();
        let encoded = delta_encode_fast(&data);
        let decoded = delta_decode_fast(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn delta_fast_small() {
        let data = vec![10u8, 20, 15, 30];
        let encoded = delta_encode_fast(&data);
        let decoded = delta_decode_fast(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn transpose_fast_roundtrip() {
        let data: Vec<u8> = (0..128).collect();
        let encoded = transpose_encode_fast(&data, 4).unwrap();
        let decoded = transpose_decode_fast(&encoded, 4).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn simd_matches_scalar_delta() {
        let data: Vec<u8> = (0u8..=255).cycle().take(4096).collect();
        let scalar = crate::delta::delta_encode(&data);
        let simd = delta_encode_fast(&data);
        assert_eq!(simd, scalar);
    }

    #[test]
    fn simd_matches_scalar_transpose() {
        let data: Vec<u8> = (0u8..=255).cycle().take(4096).collect();
        let scalar = crate::transpose::transpose_encode(&data, 8).unwrap();
        let simd = transpose_encode_fast(&data, 8).unwrap();
        assert_eq!(simd, scalar);
    }

    #[test]
    fn transpose_decode_fast_roundtrip() {
        let data: Vec<u8> = (0u8..=255).cycle().take(4096).collect();
        let encoded = transpose_encode_fast(&data, 8).unwrap();
        let decoded = transpose_decode_fast(&encoded, 8).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn zigzag_roundtrip() {
        let data: Vec<u8> = (0u8..=255).collect();
        let encoded = zigzag_encode_fast(&data);
        let decoded = zigzag_decode_fast(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn zigzag_large_simd_path() {
        // Ensure we hit the SSE2 path (>=16 bytes)
        let data: Vec<u8> = (0u8..=255).cycle().take(4096).collect();
        let encoded = zigzag_encode_fast(&data);
        let decoded = zigzag_decode_fast(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn zigzag_matches_scalar() {
        let data: Vec<u8> = (0u8..=255).cycle().take(4096).collect();
        let scalar = zigzag_encode_scalar(&data);
        let simd = zigzag_encode_fast(&data);
        assert_eq!(simd, scalar);
    }
}
