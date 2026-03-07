// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! XOR-delta transform for floating-point data (Gorilla encoding).
//!
//! XORs consecutive float values. Adjacent similar floats produce XOR
//! results concentrated in the low bits, which compress extremely well.
//!
//! Wire format: `[precision:1][count:4 LE][xor_bytes]`
//! - precision 4 = f32, precision 8 = f64

use cpac_types::{CpacError, CpacResult, CpacType, FloatPrecision, TypeTag};

use crate::traits::{TransformContext, TransformNode};

/// Transform ID for float XOR-delta (wire format).
pub const TRANSFORM_ID: u8 = 15;

// ---------------------------------------------------------------------------
// f64 XOR-delta
// ---------------------------------------------------------------------------

/// XOR-delta encode a slice of f64 values.
///
/// First value stored as-is, subsequent values as XOR with previous.
#[must_use]
pub fn float64_xor_encode(values: &[f64]) -> Vec<u8> {
    if values.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(values.len() * 8);
    let mut prev_bits = values[0].to_bits();
    out.extend_from_slice(&prev_bits.to_le_bytes());

    for &v in &values[1..] {
        let bits = v.to_bits();
        let xor = bits ^ prev_bits;
        out.extend_from_slice(&xor.to_le_bytes());
        prev_bits = bits;
    }
    out
}

/// XOR-delta decode f64 values.
pub fn float64_xor_decode(data: &[u8]) -> CpacResult<Vec<f64>> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    if !data.len().is_multiple_of(8) {
        return Err(CpacError::Transform(
            "float_xor: f64 data not multiple of 8".into(),
        ));
    }
    let count = data.len() / 8;
    let mut values = Vec::with_capacity(count);

    let first_bits = u64::from_le_bytes(data[0..8].try_into().unwrap());
    values.push(f64::from_bits(first_bits));
    let mut prev_bits = first_bits;

    for i in 1..count {
        let xor = u64::from_le_bytes(data[i * 8..(i + 1) * 8].try_into().unwrap());
        let bits = xor ^ prev_bits;
        values.push(f64::from_bits(bits));
        prev_bits = bits;
    }
    Ok(values)
}

// ---------------------------------------------------------------------------
// f32 XOR-delta
// ---------------------------------------------------------------------------

/// XOR-delta encode a slice of f32 values.
#[must_use]
pub fn float32_xor_encode(values: &[f32]) -> Vec<u8> {
    if values.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(values.len() * 4);
    let mut prev_bits = values[0].to_bits();
    out.extend_from_slice(&prev_bits.to_le_bytes());

    for &v in &values[1..] {
        let bits = v.to_bits();
        let xor = bits ^ prev_bits;
        out.extend_from_slice(&xor.to_le_bytes());
        prev_bits = bits;
    }
    out
}

/// XOR-delta decode f32 values.
pub fn float32_xor_decode(data: &[u8]) -> CpacResult<Vec<f32>> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    if !data.len().is_multiple_of(4) {
        return Err(CpacError::Transform(
            "float_xor: f32 data not multiple of 4".into(),
        ));
    }
    let count = data.len() / 4;
    let mut values = Vec::with_capacity(count);

    let first_bits = u32::from_le_bytes(data[0..4].try_into().unwrap());
    values.push(f32::from_bits(first_bits));
    let mut prev_bits = first_bits;

    for i in 1..count {
        let xor = u32::from_le_bytes(data[i * 4..(i + 1) * 4].try_into().unwrap());
        let bits = xor ^ prev_bits;
        values.push(f32::from_bits(bits));
        prev_bits = bits;
    }
    Ok(values)
}

// ---------------------------------------------------------------------------
// Framed encode/decode
// ---------------------------------------------------------------------------

/// Framed encode: `[precision:1][count:4 LE][xor_bytes]`.
pub fn float_xor_encode_framed(values_f64: &[f64], precision: FloatPrecision) -> Vec<u8> {
    let count = values_f64.len() as u32;
    match precision {
        FloatPrecision::F32 => {
            let f32_vals: Vec<f32> = values_f64.iter().map(|&v| v as f32).collect();
            let xor_bytes = float32_xor_encode(&f32_vals);
            let mut out = Vec::with_capacity(5 + xor_bytes.len());
            out.push(4); // f32
            out.extend_from_slice(&count.to_le_bytes());
            out.extend_from_slice(&xor_bytes);
            out
        }
        FloatPrecision::F64 => {
            let xor_bytes = float64_xor_encode(values_f64);
            let mut out = Vec::with_capacity(5 + xor_bytes.len());
            out.push(8); // f64
            out.extend_from_slice(&count.to_le_bytes());
            out.extend_from_slice(&xor_bytes);
            out
        }
    }
}

/// Framed decode.
pub fn float_xor_decode_framed(data: &[u8]) -> CpacResult<(Vec<f64>, FloatPrecision)> {
    if data.len() < 5 {
        return Err(CpacError::Transform(
            "float_xor: insufficient header".into(),
        ));
    }
    let precision_byte = data[0];
    let _count = u32::from_le_bytes([data[1], data[2], data[3], data[4]]) as usize;
    let payload = &data[5..];

    match precision_byte {
        4 => {
            let f32_vals = float32_xor_decode(payload)?;
            let f64_vals: Vec<f64> = f32_vals.iter().map(|&v| f64::from(v)).collect();
            Ok((f64_vals, FloatPrecision::F32))
        }
        8 => {
            let f64_vals = float64_xor_decode(payload)?;
            Ok((f64_vals, FloatPrecision::F64))
        }
        _ => Err(CpacError::Transform(format!(
            "float_xor: unsupported precision byte: {precision_byte}"
        ))),
    }
}

// ---------------------------------------------------------------------------
// TransformNode implementation
// ---------------------------------------------------------------------------

/// Float XOR-delta transform node for the compression DAG.
pub struct FloatXorTransform;

impl TransformNode for FloatXorTransform {
    fn name(&self) -> &'static str {
        "float_xor"
    }

    fn id(&self) -> u8 {
        TRANSFORM_ID
    }

    fn accepts(&self) -> &[TypeTag] {
        &[TypeTag::FloatColumn]
    }

    fn produces(&self) -> TypeTag {
        TypeTag::Serial
    }

    fn estimate_gain(&self, input: &CpacType, _ctx: &TransformContext) -> Option<f64> {
        match input {
            CpacType::FloatColumn { values, .. } => {
                if values.len() < 4 {
                    return None;
                }
                // Estimate: check how many XOR results have many leading zeros
                let probe = values.len().min(256);
                let mut zero_bytes = 0usize;
                let mut prev = values[0].to_bits();
                for &v in &values[1..probe] {
                    let bits = v.to_bits();
                    let xor = bits ^ prev;
                    zero_bytes += (xor.leading_zeros() / 8) as usize;
                    prev = bits;
                }
                let avg_zero = zero_bytes as f64 / (probe - 1) as f64;
                if avg_zero >= 1.0 {
                    Some(avg_zero * 2.0)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn encode(&self, input: CpacType, _ctx: &TransformContext) -> CpacResult<(CpacType, Vec<u8>)> {
        match input {
            CpacType::FloatColumn { values, precision } => {
                let framed = float_xor_encode_framed(&values, precision);
                Ok((CpacType::Serial(framed), Vec::new()))
            }
            _ => Err(CpacError::Transform(
                "float_xor: unsupported input type".into(),
            )),
        }
    }

    fn decode(&self, input: CpacType, _metadata: &[u8]) -> CpacResult<CpacType> {
        match input {
            CpacType::Serial(data) => {
                let (values, precision) = float_xor_decode_framed(&data)?;
                Ok(CpacType::FloatColumn { values, precision })
            }
            _ => Err(CpacError::Transform(
                "float_xor: unsupported input type".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_f64() {
        let values = vec![1.0, 1.001, 1.002, 1.003, 2.0, 2.001];
        let encoded = float64_xor_encode(&values);
        let decoded = float64_xor_decode(&encoded).unwrap();
        assert_eq!(decoded, values);
    }

    #[test]
    fn roundtrip_f32() {
        let values = vec![1.0f32, 1.001, 1.002, 1.003, 2.0, 2.001];
        let encoded = float32_xor_encode(&values);
        let decoded = float32_xor_decode(&encoded).unwrap();
        assert_eq!(decoded, values);
    }

    #[test]
    fn xor_concentrates_small_diffs() {
        // Similar values should produce XOR results with many zero bytes
        let values: Vec<f64> = (0..100).map(|i| 100.0 + i as f64 * 0.001).collect();
        let encoded = float64_xor_encode(&values);
        // Skip first 8 bytes (stored as-is), count zero bytes in XOR stream
        let zero_count = encoded[8..].iter().filter(|&&b| b == 0).count();
        let total = encoded.len() - 8;
        assert!(
            zero_count as f64 / total as f64 > 0.3,
            "expected many zero bytes in XOR stream"
        );
    }

    #[test]
    fn framed_roundtrip_f64() {
        let values = vec![1.0, 1.5, 2.0, 2.5, 3.0];
        let framed = float_xor_encode_framed(&values, FloatPrecision::F64);
        let (decoded, prec) = float_xor_decode_framed(&framed).unwrap();
        assert_eq!(prec, FloatPrecision::F64);
        assert_eq!(decoded, values);
    }

    #[test]
    fn transform_node_roundtrip() {
        let t = FloatXorTransform;
        let ctx = TransformContext {
            entropy_estimate: 4.0,
            ascii_ratio: 0.0,
            data_size: 800,
        };
        let values = vec![100.0, 100.5, 101.0, 101.5, 102.0];
        let input = CpacType::FloatColumn {
            values: values.clone(),
            precision: FloatPrecision::F64,
        };
        let (encoded, meta) = t.encode(input, &ctx).unwrap();
        let decoded = t.decode(encoded, &meta).unwrap();
        match decoded {
            CpacType::FloatColumn { values: v, .. } => assert_eq!(v, values),
            _ => panic!("expected FloatColumn"),
        }
    }
}
