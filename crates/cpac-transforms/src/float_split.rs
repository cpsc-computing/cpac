// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Float deconstruct transform.
//!
//! Splits IEEE 754 floats into separate exponent and sign+fraction streams.
//! Each stream compresses better independently because exponent bytes are
//! highly correlated in real data.

use cpac_types::{CpacError, CpacResult, CpacType, TypeTag};

use crate::traits::{TransformContext, TransformNode};

/// Transform ID for float split (wire format).
pub const TRANSFORM_ID: u8 = 2;

// ---------------------------------------------------------------------------
// float32 split (matches Python float32_split_encode / decode)
// ---------------------------------------------------------------------------

/// Split float32 array into (exponent, `sign_fraction`) streams.
pub fn float32_split_encode(data: &[u8]) -> CpacResult<(Vec<u8>, Vec<u8>)> {
    let n = data.len();
    if !n.is_multiple_of(4) {
        return Err(CpacError::Transform(format!(
            "float32 data length {n} not divisible by 4"
        )));
    }
    let count = n / 4;
    let mut exponents = vec![0u8; count];
    let mut sign_fracs = vec![0u8; count * 3];
    for i in 0..count {
        let f32_bits = u32::from_le_bytes([
            data[i * 4],
            data[i * 4 + 1],
            data[i * 4 + 2],
            data[i * 4 + 3],
        ]);
        // Rotate sign bit from MSB to LSB
        let rotated = f32_bits.rotate_left(1);
        exponents[i] = (rotated >> 24) as u8;
        sign_fracs[i * 3] = rotated as u8;
        sign_fracs[i * 3 + 1] = (rotated >> 8) as u8;
        sign_fracs[i * 3 + 2] = (rotated >> 16) as u8;
    }
    Ok((exponents, sign_fracs))
}

/// Reconstruct float32 array from split streams.
pub fn float32_split_decode(exponents: &[u8], sign_fracs: &[u8]) -> CpacResult<Vec<u8>> {
    let count = exponents.len();
    if sign_fracs.len() != count * 3 {
        return Err(CpacError::Transform(
            "float32 sign_fracs length mismatch".into(),
        ));
    }
    let mut out = vec![0u8; count * 4];
    for i in 0..count {
        let rotated: u32 = u32::from(sign_fracs[i * 3])
            | (u32::from(sign_fracs[i * 3 + 1]) << 8)
            | (u32::from(sign_fracs[i * 3 + 2]) << 16)
            | (u32::from(exponents[i]) << 24);
        let f32_bits = ((rotated >> 1) & 0x7FFF_FFFF) | ((rotated & 1) << 31);
        out[i * 4..i * 4 + 4].copy_from_slice(&f32_bits.to_le_bytes());
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Framed float split (self-describing)
// ---------------------------------------------------------------------------

/// Encode with frame header: `[float_width:1][count:4 LE][exp_len:4 LE][exps][sign_fracs]`.
pub fn float_split_encode_framed(data: &[u8], float_width: u8) -> CpacResult<Vec<u8>> {
    if float_width != 4 {
        return Err(CpacError::Transform(format!(
            "unsupported float_width: {float_width} (only 4 supported in Phase 3)"
        )));
    }
    let (exps, sfs) = float32_split_encode(data)?;
    let count = (data.len() / float_width as usize) as u32;
    let exp_len = exps.len() as u32;
    let mut out = Vec::with_capacity(9 + exps.len() + sfs.len());
    out.push(float_width);
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&exp_len.to_le_bytes());
    out.extend_from_slice(&exps);
    out.extend_from_slice(&sfs);
    Ok(out)
}

/// Decode framed float split.
pub fn float_split_decode_framed(data: &[u8]) -> CpacResult<Vec<u8>> {
    if data.len() < 9 {
        return Err(CpacError::Transform(
            "float_split: insufficient header".into(),
        ));
    }
    let float_width = data[0];
    let count = u32::from_le_bytes([data[1], data[2], data[3], data[4]]) as usize;
    let exp_len = u32::from_le_bytes([data[5], data[6], data[7], data[8]]) as usize;
    let exps = &data[9..9 + exp_len];
    let sf_start = 9 + exp_len;
    let sf_len = if float_width == 4 {
        count * 3
    } else {
        count * 7
    };
    if sf_start + sf_len > data.len() {
        return Err(CpacError::Transform(
            "float_split: truncated payload".into(),
        ));
    }
    let sfs = &data[sf_start..sf_start + sf_len];
    if float_width == 4 {
        float32_split_decode(exps, sfs)
    } else {
        Err(CpacError::Transform(format!(
            "unsupported float_width: {float_width}"
        )))
    }
}

// ---------------------------------------------------------------------------
// Detect float data (matches Python _detect_float_data)
// ---------------------------------------------------------------------------

/// Detect if data is primarily float32 values. Returns float width or None.
#[must_use]
pub fn detect_float_data(data: &[u8]) -> Option<u8> {
    let n = data.len();
    if n < 32 || !n.is_multiple_of(4) {
        return None;
    }
    let count = (n / 4).min(256);
    let mut valid = 0usize;
    for i in 0..count {
        let bits = u32::from_le_bytes([
            data[i * 4],
            data[i * 4 + 1],
            data[i * 4 + 2],
            data[i * 4 + 3],
        ]);
        let val = f32::from_bits(bits);
        if val.is_finite() && (val == 0.0 || val.abs() > 1e-30) && val.abs() < 1e38 {
            valid += 1;
        }
    }
    if valid > count * 4 / 5 {
        Some(4)
    } else {
        None
    }
}

/// Float split transform node.
pub struct FloatSplitTransform;

impl TransformNode for FloatSplitTransform {
    fn name(&self) -> &'static str {
        "float_split"
    }
    fn id(&self) -> u8 {
        TRANSFORM_ID
    }
    fn accepts(&self) -> &[TypeTag] {
        &[TypeTag::Serial]
    }
    fn produces(&self) -> TypeTag {
        TypeTag::Serial
    }
    fn estimate_gain(&self, input: &CpacType, ctx: &TransformContext) -> Option<f64> {
        if ctx.entropy_estimate > 6.5 || ctx.ascii_ratio > 0.85 {
            return None;
        }
        match input {
            CpacType::Serial(data) => detect_float_data(data).map(|_| 3.0),
            _ => None,
        }
    }
    fn encode(&self, input: CpacType, _ctx: &TransformContext) -> CpacResult<(CpacType, Vec<u8>)> {
        match input {
            CpacType::Serial(data) => {
                let framed = float_split_encode_framed(&data, 4)?;
                Ok((CpacType::Serial(framed), Vec::new()))
            }
            _ => Err(CpacError::Transform("float_split: unsupported type".into())),
        }
    }
    fn decode(&self, input: CpacType, _metadata: &[u8]) -> CpacResult<CpacType> {
        match input {
            CpacType::Serial(data) => {
                let decoded = float_split_decode_framed(&data)?;
                Ok(CpacType::Serial(decoded))
            }
            _ => Err(CpacError::Transform("float_split: unsupported type".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float32_roundtrip() {
        let floats: Vec<f32> = vec![1.0, -2.5, 0.0, 3.125, 100.0, -0.001];
        let data: Vec<u8> = floats.iter().flat_map(|f| f.to_le_bytes()).collect();
        let (exps, sfs) = float32_split_encode(&data).unwrap();
        let restored = float32_split_decode(&exps, &sfs).unwrap();
        assert_eq!(restored, data);
    }

    #[test]
    fn framed_roundtrip() {
        let floats: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let data: Vec<u8> = floats.iter().flat_map(|f| f.to_le_bytes()).collect();
        let framed = float_split_encode_framed(&data, 4).unwrap();
        let restored = float_split_decode_framed(&framed).unwrap();
        assert_eq!(restored, data);
    }

    #[test]
    fn detect_float_array() {
        let floats: Vec<f32> = (0..100).map(|i| i as f32 * 1.5).collect();
        let data: Vec<u8> = floats.iter().flat_map(|f| f.to_le_bytes()).collect();
        assert_eq!(detect_float_data(&data), Some(4));
    }
}
