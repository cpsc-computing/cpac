// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Columnar byte-level transpose.
//!
//! For fixed-width records of W bytes, rearranges data so all byte-position-0
//! values are contiguous, then all byte-position-1 values, etc.
//!
//! Example (3 records × 4 bytes):
//! ```text
//! Input:  [A0 A1 A2 A3] [B0 B1 B2 B3] [C0 C1 C2 C3]
//! Output: [A0 B0 C0] [A1 B1 C1] [A2 B2 C2] [A3 B3 C3]
//! ```
//!
//! This dramatically improves entropy coding because same-semantic bytes
//! (e.g. all high bytes of int32) cluster together.

use cpac_types::{CpacError, CpacResult, CpacType, TypeTag};

use crate::traits::{TransformContext, TransformNode};

/// Transform ID for transpose encoding (wire format).
pub const TRANSFORM_ID: u8 = 1;

// ---------------------------------------------------------------------------
// Core transpose (matches Python transpose_encode / transpose_decode)
// ---------------------------------------------------------------------------

/// Transpose fixed-width records to columnar layout.
///
/// `element_width` is the record width in bytes.
/// Data length must be divisible by `element_width`.
pub fn transpose_encode(data: &[u8], element_width: usize) -> CpacResult<Vec<u8>> {
    if element_width == 0 {
        return Err(CpacError::Transform(
            "transpose: element_width must be > 0".into(),
        ));
    }
    if element_width == 1 || data.is_empty() {
        return Ok(data.to_vec());
    }
    let n = data.len();
    if !n.is_multiple_of(element_width) {
        return Err(CpacError::Transform(format!(
            "transpose: data length {n} not divisible by element_width {element_width}"
        )));
    }
    let num_elements = n / element_width;
    let mut out = vec![0u8; n];
    for col in 0..element_width {
        let dst_offset = col * num_elements;
        for row in 0..num_elements {
            out[dst_offset + row] = data[row * element_width + col];
        }
    }
    Ok(out)
}

/// Reverse columnar transpose back to row-major layout.
pub fn transpose_decode(data: &[u8], element_width: usize) -> CpacResult<Vec<u8>> {
    if element_width == 0 {
        return Err(CpacError::Transform(
            "transpose: element_width must be > 0".into(),
        ));
    }
    if element_width == 1 || data.is_empty() {
        return Ok(data.to_vec());
    }
    let n = data.len();
    if !n.is_multiple_of(element_width) {
        return Err(CpacError::Transform(format!(
            "transpose: data length {n} not divisible by element_width {element_width}"
        )));
    }
    let num_elements = n / element_width;
    let mut out = vec![0u8; n];
    for col in 0..element_width {
        let src_offset = col * num_elements;
        for row in 0..num_elements {
            out[row * element_width + col] = data[src_offset + row];
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Record-width detection (matches Python _detect_record_width)
// ---------------------------------------------------------------------------

/// Detect fixed-width record structure in binary data.
///
/// Checks common widths (2, 4, 8, 16, 32) and returns the best match
/// based on column-byte repetition scoring.
#[must_use]
pub fn detect_record_width(data: &[u8]) -> Option<usize> {
    let probe = if data.len() > 8192 {
        &data[..8192]
    } else {
        data
    };
    let n = probe.len();
    if n < 32 {
        return None;
    }

    let mut best_width: Option<usize> = None;
    let mut best_score: f64 = 0.0;

    for &width in &[4usize, 8, 16, 32, 2] {
        if n % width != 0 {
            continue;
        }
        let num_records = n / width;
        if num_records < 4 {
            continue;
        }

        let mut col_score: f64 = 0.0;
        for col in 0..width {
            let mut seen = [false; 256];
            let mut unique = 0usize;
            for row in 0..num_records {
                let b = probe[row * width + col] as usize;
                if !seen[b] {
                    seen[b] = true;
                    unique += 1;
                }
            }
            col_score += 1.0 - (unique as f64 / num_records as f64);
        }
        let avg_score = col_score / width as f64;
        if avg_score > best_score && avg_score > 0.3 {
            best_score = avg_score;
            best_width = Some(width);
        }
    }

    best_width
}

// ---------------------------------------------------------------------------
// TransformNode implementation
// ---------------------------------------------------------------------------

/// Transpose transform node for the compression DAG.
pub struct TransposeTransform;

impl TransformNode for TransposeTransform {
    fn name(&self) -> &'static str {
        "transpose"
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
        if ctx.entropy_estimate > 7.0 || ctx.ascii_ratio > 0.85 {
            return None; // skip for text or high-entropy
        }
        match input {
            CpacType::Serial(data) => detect_record_width(data).map(|w| w as f64 * 0.5),
            _ => None,
        }
    }

    fn encode(&self, input: CpacType, _ctx: &TransformContext) -> CpacResult<(CpacType, Vec<u8>)> {
        match input {
            CpacType::Serial(data) => {
                let width = detect_record_width(&data).ok_or_else(|| {
                    CpacError::Transform("transpose: no fixed-width structure detected".into())
                })?;
                let transposed = transpose_encode(&data, width)?;
                // Store element_width in metadata (2 bytes LE)
                let meta = (width as u16).to_le_bytes().to_vec();
                Ok((CpacType::Serial(transposed), meta))
            }
            _ => Err(CpacError::Transform(
                "transpose: unsupported input type".into(),
            )),
        }
    }

    fn decode(&self, input: CpacType, metadata: &[u8]) -> CpacResult<CpacType> {
        match input {
            CpacType::Serial(data) => {
                if metadata.len() < 2 {
                    return Err(CpacError::Transform(
                        "transpose: metadata missing element_width".into(),
                    ));
                }
                let width = u16::from_le_bytes([metadata[0], metadata[1]]) as usize;
                let restored = transpose_decode(&data, width)?;
                Ok(CpacType::Serial(restored))
            }
            _ => Err(CpacError::Transform(
                "transpose: unsupported input type".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_4byte_records() {
        // 3 records of 4 bytes
        let data = vec![
            0xA0, 0xA1, 0xA2, 0xA3, 0xB0, 0xB1, 0xB2, 0xB3, 0xC0, 0xC1, 0xC2, 0xC3,
        ];
        let transposed = transpose_encode(&data, 4).unwrap();
        // Column 0: [A0, B0, C0], Column 1: [A1, B1, C1], ...
        assert_eq!(
            transposed,
            vec![0xA0, 0xB0, 0xC0, 0xA1, 0xB1, 0xC1, 0xA2, 0xB2, 0xC2, 0xA3, 0xB3, 0xC3]
        );
        let restored = transpose_decode(&transposed, 4).unwrap();
        assert_eq!(restored, data);
    }

    #[test]
    fn roundtrip_width_1() {
        let data = vec![1, 2, 3, 4, 5];
        let encoded = transpose_encode(&data, 1).unwrap();
        assert_eq!(encoded, data); // no-op for width 1
    }

    #[test]
    fn roundtrip_empty() {
        let encoded = transpose_encode(&[], 4).unwrap();
        assert!(encoded.is_empty());
    }

    #[test]
    fn error_not_divisible() {
        let data = vec![1, 2, 3, 4, 5]; // 5 bytes, not divisible by 4
        assert!(transpose_encode(&data, 4).is_err());
    }

    #[test]
    fn error_zero_width() {
        assert!(transpose_encode(&[1, 2], 0).is_err());
    }

    #[test]
    fn detect_structured_data() {
        // Create structured data: 100 records of 4 bytes with distinct
        // constant columns (all low-cardinality so width=4 scores highest).
        let mut data = Vec::with_capacity(400);
        for _ in 0..100u8 {
            data.push(0x01); // field 0: constant
            data.push(0x02); // field 1: constant (different value)
            data.push(0xFF); // field 2: constant
            data.push(0x00); // field 3: constant
        }
        let width = detect_record_width(&data);
        assert_eq!(width, Some(4));
    }

    #[test]
    fn detect_structured_with_varying_column() {
        // Data with one varying column — algorithm may detect width=2 or 4
        let mut data = Vec::with_capacity(400);
        for i in 0u8..100 {
            data.push(0x01);
            data.push(i); // high-cardinality counter
            data.push(0xFF);
            data.push(0x00);
        }
        let width = detect_record_width(&data);
        assert!(width.is_some(), "should detect some structure");
    }

    #[test]
    fn detect_no_structure() {
        // Random-looking data should not detect structure
        let data: Vec<u8> = (0..256).map(|i| ((i * 37 + 13) % 256) as u8).collect();
        let width = detect_record_width(&data);
        assert!(width.is_none());
    }

    #[test]
    fn transform_node_roundtrip() {
        let t = TransposeTransform;
        let mut data = Vec::with_capacity(400);
        for i in 0u8..100 {
            data.push(0x01);
            data.push(i);
            data.push(0xFF);
            data.push(0x00);
        }
        let ctx = TransformContext {
            entropy_estimate: 3.0,
            ascii_ratio: 0.0,
            data_size: data.len(),
        };
        let input = CpacType::Serial(data.clone());
        let (encoded, meta) = t.encode(input, &ctx).unwrap();
        let decoded = t.decode(encoded, &meta).unwrap();
        match decoded {
            CpacType::Serial(d) => assert_eq!(d, data),
            _ => panic!("expected Serial"),
        }
    }
}
