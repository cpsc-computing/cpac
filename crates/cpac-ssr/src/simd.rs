// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! SIMD-accelerated SSR scanning primitives.
//!
//! Provides fast byte-frequency histogram and ASCII-ratio computation with
//! runtime dispatch: AVX2 → SSE2 → scalar (x86_64), NEON → scalar (aarch64).

// ---------------------------------------------------------------------------
// Public API — runtime dispatch
// ---------------------------------------------------------------------------

/// Count bytes matching printable ASCII (0x20–0x7E) or whitespace (0x09, 0x0A, 0x0D).
///
/// Dispatches to the fastest available SIMD path at runtime.
pub fn count_ascii_bytes(data: &[u8]) -> usize {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") {
            // SAFETY: feature detected at runtime.
            return unsafe { count_ascii_avx2(data) };
        }
        if is_x86_feature_detected!("sse2") {
            return unsafe { count_ascii_sse2(data) };
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        // NEON is always available on aarch64.
        return unsafe { count_ascii_neon(data) };
    }

    count_ascii_scalar(data)
}

/// Build a 256-bin byte-frequency histogram.
///
/// Uses a 4× unrolled loop for instruction-level parallelism (ILP).
/// SIMD histogram via VPSHUFB is theoretically possible but the gather/scatter
/// overhead makes it slower than unrolled scalar for histograms on modern OoO CPUs.
pub fn byte_histogram(data: &[u8]) -> [u64; 256] {
    // 4-way unrolled histogram: 4 independent count arrays to break
    // dependency chains and allow the CPU to pipeline stores.
    let mut c0 = [0u64; 256];
    let mut c1 = [0u64; 256];
    let mut c2 = [0u64; 256];
    let mut c3 = [0u64; 256];

    let chunks = data.chunks_exact(4);
    let remainder = chunks.remainder();

    for chunk in chunks {
        c0[chunk[0] as usize] += 1;
        c1[chunk[1] as usize] += 1;
        c2[chunk[2] as usize] += 1;
        c3[chunk[3] as usize] += 1;
    }
    for &b in remainder {
        c0[b as usize] += 1;
    }

    // Merge
    for i in 0..256 {
        c0[i] += c1[i] + c2[i] + c3[i];
    }

    c0
}

// ---------------------------------------------------------------------------
// Scalar fallback
// ---------------------------------------------------------------------------

/// Scalar ASCII byte counter (portable fallback).
fn count_ascii_scalar(data: &[u8]) -> usize {
    data.iter()
        .filter(|&&b| (0x20..=0x7E).contains(&b) || b == 0x09 || b == 0x0A || b == 0x0D)
        .count()
}

// ---------------------------------------------------------------------------
// x86_64: AVX2 implementation (32 bytes/iteration)
// ---------------------------------------------------------------------------

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn count_ascii_avx2(data: &[u8]) -> usize {
    use std::arch::x86_64::*;

    let len = data.len();
    let mut total: usize = 0;
    let mut i: usize = 0;

    // Constants for range checks.
    let lo = _mm256_set1_epi8(0x1F_u8 as i8);  // < 0x20 → not printable
    let hi = _mm256_set1_epi8(0x7E_u8 as i8);  // <= 0x7E → printable
    let tab = _mm256_set1_epi8(0x09_u8 as i8);  // \t
    let lf = _mm256_set1_epi8(0x0A_u8 as i8);   // \n
    let cr = _mm256_set1_epi8(0x0D_u8 as i8);   // \r

    while i + 32 <= len {
        let v = _mm256_loadu_si256(data.as_ptr().add(i) as *const __m256i);

        // Printable range: b > 0x1F && b <= 0x7E
        // Use unsigned comparison trick: subtract lo+1 then compare < (0x7E - 0x20 + 1)
        // But with signed ops we do: cmpgt(v, 0x1F) & ~cmpgt(v, 0x7E)
        let gt_lo = _mm256_cmpgt_epi8(v, lo);
        let gt_hi = _mm256_cmpgt_epi8(v, hi);
        let printable = _mm256_andnot_si256(gt_hi, gt_lo);

        // Whitespace: exact matches for \t, \n, \r
        let is_tab = _mm256_cmpeq_epi8(v, tab);
        let is_lf = _mm256_cmpeq_epi8(v, lf);
        let is_cr = _mm256_cmpeq_epi8(v, cr);
        let ws = _mm256_or_si256(is_tab, _mm256_or_si256(is_lf, is_cr));

        let mask = _mm256_or_si256(printable, ws);
        let bits = _mm256_movemask_epi8(mask) as u32;
        total += bits.count_ones() as usize;

        i += 32;
    }

    // Scalar tail
    for &b in &data[i..] {
        if (0x20..=0x7E).contains(&b) || b == 0x09 || b == 0x0A || b == 0x0D {
            total += 1;
        }
    }

    total
}

// ---------------------------------------------------------------------------
// x86_64: SSE2 implementation (16 bytes/iteration)
// ---------------------------------------------------------------------------

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn count_ascii_sse2(data: &[u8]) -> usize {
    use std::arch::x86_64::*;

    let len = data.len();
    let mut total: usize = 0;
    let mut i: usize = 0;

    let lo = _mm_set1_epi8(0x1F_u8 as i8);
    let hi = _mm_set1_epi8(0x7E_u8 as i8);
    let tab = _mm_set1_epi8(0x09_u8 as i8);
    let lf = _mm_set1_epi8(0x0A_u8 as i8);
    let cr = _mm_set1_epi8(0x0D_u8 as i8);

    while i + 16 <= len {
        let v = _mm_loadu_si128(data.as_ptr().add(i) as *const __m128i);

        let gt_lo = _mm_cmpgt_epi8(v, lo);
        let gt_hi = _mm_cmpgt_epi8(v, hi);
        let printable = _mm_andnot_si128(gt_hi, gt_lo);

        let is_tab = _mm_cmpeq_epi8(v, tab);
        let is_lf = _mm_cmpeq_epi8(v, lf);
        let is_cr = _mm_cmpeq_epi8(v, cr);
        let ws = _mm_or_si128(is_tab, _mm_or_si128(is_lf, is_cr));

        let mask = _mm_or_si128(printable, ws);
        let bits = _mm_movemask_epi8(mask) as u32;
        total += bits.count_ones() as usize;

        i += 16;
    }

    for &b in &data[i..] {
        if (0x20..=0x7E).contains(&b) || b == 0x09 || b == 0x0A || b == 0x0D {
            total += 1;
        }
    }

    total
}

// ---------------------------------------------------------------------------
// aarch64: NEON implementation (16 bytes/iteration)
// ---------------------------------------------------------------------------

#[cfg(target_arch = "aarch64")]
unsafe fn count_ascii_neon(data: &[u8]) -> usize {
    use std::arch::aarch64::*;

    let len = data.len();
    let mut total: usize = 0;
    let mut i: usize = 0;

    let v_0x20 = vdupq_n_u8(0x20);
    let v_0x7e = vdupq_n_u8(0x7E);
    let v_tab = vdupq_n_u8(0x09);
    let v_lf = vdupq_n_u8(0x0A);
    let v_cr = vdupq_n_u8(0x0D);

    // Accumulate in a u8 vector, flush every 255 iterations to avoid overflow.
    let mut acc = vdupq_n_u8(0);
    let mut iter_count = 0u32;

    while i + 16 <= len {
        let v = vld1q_u8(data.as_ptr().add(i));

        // printable: v >= 0x20 && v <= 0x7E
        let ge_20 = vcgeq_u8(v, v_0x20);
        let le_7e = vcleq_u8(v, v_0x7e);
        let printable = vandq_u8(ge_20, le_7e);

        // whitespace
        let is_tab = vceqq_u8(v, v_tab);
        let is_lf = vceqq_u8(v, v_lf);
        let is_cr = vceqq_u8(v, v_cr);
        let ws = vorrq_u8(is_tab, vorrq_u8(is_lf, is_cr));

        let mask = vorrq_u8(printable, ws);
        // Each matching lane is 0xFF; subtract from acc (0xFF = -1 in u8 wrapping)
        // to count: acc += (mask & 1) per byte. Since mask is 0xFF or 0x00,
        // we can use saturating subtract trick: acc = acc - mask (where mask = 0xFF = -1).
        acc = vsubq_u8(acc, mask);

        iter_count += 1;
        if iter_count == 255 {
            // Flush: horizontal sum of acc
            total += vaddlvq_u8(acc) as usize;
            acc = vdupq_n_u8(0);
            iter_count = 0;
        }

        i += 16;
    }

    // Final flush
    total += vaddlvq_u8(acc) as usize;

    // Scalar tail
    for &b in &data[i..] {
        if (0x20..=0x7E).contains(&b) || b == 0x09 || b == 0x0A || b == 0x0D {
            total += 1;
        }
    }

    total
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_count_matches_scalar() {
        let data = b"Hello, World!\n\t\r\x00\x01\x7F\x80\xFF";
        let expected = count_ascii_scalar(data);
        let got = count_ascii_bytes(data);
        assert_eq!(got, expected, "SIMD and scalar must agree");
    }

    #[test]
    fn ascii_count_all_printable() {
        let data: Vec<u8> = (0x20..=0x7E).collect();
        assert_eq!(count_ascii_bytes(&data), data.len());
    }

    #[test]
    fn ascii_count_whitespace_only() {
        assert_eq!(count_ascii_bytes(b"\t\n\r"), 3);
    }

    #[test]
    fn ascii_count_all_binary() {
        let data: Vec<u8> = (0x80..=0xFF).collect();
        assert_eq!(count_ascii_bytes(&data), 0);
    }

    #[test]
    fn ascii_count_large_buffer() {
        // Exercise SIMD main loop + tail
        let data: Vec<u8> = (0u8..=255).cycle().take(10_000).collect();
        let expected = count_ascii_scalar(&data);
        assert_eq!(count_ascii_bytes(&data), expected);
    }

    #[test]
    fn ascii_count_empty() {
        assert_eq!(count_ascii_bytes(b""), 0);
    }

    #[test]
    fn histogram_basic() {
        let data = b"aabbcc";
        let h = byte_histogram(data);
        assert_eq!(h[b'a' as usize], 2);
        assert_eq!(h[b'b' as usize], 2);
        assert_eq!(h[b'c' as usize], 2);
        assert_eq!(h[b'd' as usize], 0);
    }

    #[test]
    fn histogram_all_bytes() {
        let data: Vec<u8> = (0u8..=255).collect();
        let h = byte_histogram(&data);
        for count in h {
            assert_eq!(count, 1);
        }
    }

    #[test]
    fn histogram_empty() {
        let h = byte_histogram(b"");
        for count in h {
            assert_eq!(count, 0);
        }
    }
}
