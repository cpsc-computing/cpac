// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! NEON SIMD implementations for aarch64.

#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::*;

/// NEON delta encode (16 bytes per iteration).
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub unsafe fn delta_encode_neon(data: &mut [u8]) {
    if data.len() < 16 {
        super::scalar::delta_encode_scalar(data);
        return;
    }
    
    let mut prev = 0u8;
    let chunks = data.chunks_exact_mut(16);
    let remainder = chunks.into_remainder();
    
    for chunk in data.chunks_exact_mut(16) {
        let values = vld1q_u8(chunk.as_ptr());
        let prev_vec = vdupq_n_u8(prev);
        
        // Horizontal delta within vector
        let shifted = vextq_u8(prev_vec, values, 15);
        let deltas = vsubq_u8(values, shifted);
        
        vst1q_u8(chunk.as_mut_ptr(), deltas);
        
        prev = chunk[15];
    }
    
    // Handle remainder
    super::scalar::delta_encode_scalar_with_prev(remainder, prev);
}

/// NEON delta decode (16 bytes per iteration).
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub unsafe fn delta_decode_neon(data: &mut [u8]) {
    if data.len() < 16 {
        super::scalar::delta_decode_scalar(data);
        return;
    }
    
    let mut prev = 0u8;
    
    for chunk in data.chunks_exact_mut(16) {
        let deltas = vld1q_u8(chunk.as_ptr());
        
        // Prefix sum (cumulative add)
        let mut acc = vdupq_n_u8(prev);
        let mut result = deltas;
        
        for i in 0..16 {
            let val = vgetq_lane_u8(result, i);
            let sum = prev.wrapping_add(val);
            result = vsetq_lane_u8(sum, result, i);
            prev = sum;
        }
        
        vst1q_u8(chunk.as_mut_ptr(), result);
    }
}

/// NEON zigzag encode (16 bytes per iteration).
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub unsafe fn zigzag_encode_neon(data: &mut [u8]) {
    for chunk in data.chunks_exact_mut(16) {
        let values = vld1q_u8(chunk.as_ptr());
        
        // Zigzag: (n << 1) ^ (n >> 7)
        let shifted_left = vshlq_n_u8(values, 1);
        let shifted_right = vshrq_n_u8(values, 7);
        let zigzagged = veorq_u8(shifted_left, shifted_right);
        
        vst1q_u8(chunk.as_mut_ptr(), zigzagged);
    }
    
    // Remainder
    let remainder_start = (data.len() / 16) * 16;
    super::scalar::zigzag_encode_scalar(&mut data[remainder_start..]);
}

/// NEON zigzag decode (16 bytes per iteration).
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
pub unsafe fn zigzag_decode_neon(data: &mut [u8]) {
    for chunk in data.chunks_exact_mut(16) {
        let values = vld1q_u8(chunk.as_ptr());
        
        // Reverse: (n >> 1) ^ -(n & 1)
        let shifted = vshrq_n_u8(values, 1);
        let mask = vandq_u8(values, vdupq_n_u8(1));
        let negated = vnegq_s8(vreinterpretq_s8_u8(mask));
        let decoded = veorq_u8(shifted, vreinterpretq_u8_s8(negated));
        
        vst1q_u8(chunk.as_mut_ptr(), decoded);
    }
    
    let remainder_start = (data.len() / 16) * 16;
    super::scalar::zigzag_decode_scalar(&mut data[remainder_start..]);
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    #[cfg(target_arch = "aarch64")]
    fn test_neon_delta_roundtrip() {
        let mut data: Vec<u8> = (0..64).collect();
        let original = data.clone();
        
        unsafe {
            delta_encode_neon(&mut data);
            delta_decode_neon(&mut data);
        }
        
        assert_eq!(data, original);
    }
}
