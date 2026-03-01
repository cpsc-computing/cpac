// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Range pack: integer narrowing by subtracting min and downcasting.
//!
//! Detects `[min, max]`, subtracts min, packs values in the smallest
//! fitting integer width (1/2/4/8 bytes).

use cpac_types::{CpacError, CpacResult, CpacType, TypeTag};

use crate::traits::{TransformContext, TransformNode};

/// Transform ID for range pack (wire format).
pub const TRANSFORM_ID: u8 = 7;

/// Range-pack a slice of i64 values.
///
/// Returns `(packed_bytes, min_value, pack_width)`.
pub fn range_pack_encode(values: &[i64]) -> (Vec<u8>, i64, u8) {
    if values.is_empty() {
        return (Vec::new(), 0, 0);
    }
    let min_val = *values.iter().min().unwrap();
    let max_val = *values.iter().max().unwrap();
    let range = (max_val - min_val) as u64;

    let pack_width: u8 = if range <= 0xFF {
        1
    } else if range <= 0xFFFF {
        2
    } else if range <= 0xFFFF_FFFF {
        4
    } else {
        8
    };

    let mut packed = Vec::with_capacity(values.len() * pack_width as usize);
    for &v in values {
        let shifted = (v - min_val) as u64;
        match pack_width {
            1 => packed.push(shifted as u8),
            2 => packed.extend_from_slice(&(shifted as u16).to_le_bytes()),
            4 => packed.extend_from_slice(&(shifted as u32).to_le_bytes()),
            _ => packed.extend_from_slice(&shifted.to_le_bytes()),
        }
    }
    (packed, min_val, pack_width)
}

/// Decode range-packed integers.
pub fn range_pack_decode(data: &[u8], count: usize, min_value: i64, pack_width: u8) -> Vec<i64> {
    let mut values = Vec::with_capacity(count);
    for i in 0..count {
        let offset = i * pack_width as usize;
        let v: u64 = match pack_width {
            1 => data[offset] as u64,
            2 => u16::from_le_bytes([data[offset], data[offset + 1]]) as u64,
            4 => u32::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]) as u64,
            _ => u64::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
                data[offset + 4],
                data[offset + 5],
                data[offset + 6],
                data[offset + 7],
            ]),
        };
        values.push(min_value + v as i64);
    }
    values
}

/// Framed encode: `[count:4 LE][min_value:8 LE signed][pack_width:1][packed]`.
pub fn range_pack_encode_framed(values: &[i64]) -> Vec<u8> {
    let (packed, min_val, pack_width) = range_pack_encode(values);
    let mut out = Vec::with_capacity(13 + packed.len());
    out.extend_from_slice(&(values.len() as u32).to_le_bytes());
    out.extend_from_slice(&min_val.to_le_bytes());
    out.push(pack_width);
    out.extend_from_slice(&packed);
    out
}

/// Framed decode.
pub fn range_pack_decode_framed(data: &[u8]) -> CpacResult<Vec<i64>> {
    if data.len() < 13 {
        return Err(CpacError::Transform(
            "range_pack: insufficient header".into(),
        ));
    }
    let count = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let min_val = i64::from_le_bytes([
        data[4], data[5], data[6], data[7], data[8], data[9], data[10], data[11],
    ]);
    let pack_width = data[12];
    Ok(range_pack_decode(&data[13..], count, min_val, pack_width))
}

/// Range pack transform node.
pub struct RangePackTransform;

impl TransformNode for RangePackTransform {
    fn name(&self) -> &str {
        "range_pack"
    }
    fn id(&self) -> u8 {
        TRANSFORM_ID
    }
    fn accepts(&self) -> &[TypeTag] {
        &[TypeTag::IntColumn]
    }
    fn produces(&self) -> TypeTag {
        TypeTag::IntColumn
    }
    fn estimate_gain(&self, input: &CpacType, _ctx: &TransformContext) -> Option<f64> {
        match input {
            CpacType::IntColumn {
                values,
                original_width,
            } => {
                if values.is_empty() {
                    return None;
                }
                let min_val = *values.iter().min().unwrap();
                let max_val = *values.iter().max().unwrap();
                let range = (max_val - min_val) as u64;
                let pack_w = if range <= 0xFF {
                    1
                } else if range <= 0xFFFF {
                    2
                } else if range <= 0xFFFF_FFFF {
                    4
                } else {
                    8
                };
                if pack_w < *original_width {
                    Some((*original_width - pack_w) as f64)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
    fn encode(&self, input: CpacType, _ctx: &TransformContext) -> CpacResult<(CpacType, Vec<u8>)> {
        match input {
            CpacType::IntColumn {
                values,
                original_width,
            } => {
                let framed = range_pack_encode_framed(&values);
                // Metadata stores original_width so we can restore it
                Ok((CpacType::Serial(framed), vec![original_width]))
            }
            _ => Err(CpacError::Transform("range_pack: unsupported type".into())),
        }
    }
    fn decode(&self, input: CpacType, metadata: &[u8]) -> CpacResult<CpacType> {
        match input {
            CpacType::Serial(data) => {
                let values = range_pack_decode_framed(&data)?;
                let original_width = if metadata.is_empty() { 8 } else { metadata[0] };
                Ok(CpacType::IntColumn {
                    values,
                    original_width,
                })
            }
            _ => Err(CpacError::Transform("range_pack: unsupported type".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_small_range() {
        let values = vec![100, 105, 110, 108, 120, 115];
        let (packed, min, width) = range_pack_encode(&values);
        assert_eq!(min, 100);
        assert_eq!(width, 1); // range 0..20 fits in u8
        let decoded = range_pack_decode(&packed, values.len(), min, width);
        assert_eq!(decoded, values);
    }

    #[test]
    fn framed_roundtrip() {
        let values = vec![1000, 2000, 3000, 1500, 2500];
        let framed = range_pack_encode_framed(&values);
        let decoded = range_pack_decode_framed(&framed).unwrap();
        assert_eq!(decoded, values);
    }

    #[test]
    fn negative_values() {
        let values = vec![-100, -50, 0, 50, 100];
        let framed = range_pack_encode_framed(&values);
        let decoded = range_pack_decode_framed(&framed).unwrap();
        assert_eq!(decoded, values);
    }
}
