// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Byte-plane split transform.
//!
//! For fixed-width integer data, splits N-byte values into N separate planes
//! (all byte-0s, all byte-1s, etc.). High bytes of similar integers are
//! nearly identical → near-zero entropy per plane.
//!
//! Wire format: `[width:1][count:4 LE][plane_0][plane_1]...[plane_{width-1}]`

use cpac_types::{CpacError, CpacResult, CpacType, TypeTag};

use crate::traits::{TransformContext, TransformNode};

/// Transform ID for byte-plane split (wire format).
pub const TRANSFORM_ID: u8 = 12;

// ---------------------------------------------------------------------------
// Core encode/decode
// ---------------------------------------------------------------------------

/// Split fixed-width values into byte planes.
///
/// `width` is the value width in bytes (1, 2, 4, or 8).
/// Data length must be divisible by `width`.
pub fn byte_plane_encode(data: &[u8], width: usize) -> CpacResult<Vec<u8>> {
    if width == 0 || width == 1 {
        return Ok(data.to_vec()); // no-op for single-byte
    }
    let n = data.len();
    if !n.is_multiple_of(width) {
        return Err(CpacError::Transform(format!(
            "byte_plane: data length {n} not divisible by width {width}"
        )));
    }
    let count = n / width;
    let mut planes = vec![0u8; n];

    for plane in 0..width {
        let dst_offset = plane * count;
        for i in 0..count {
            planes[dst_offset + i] = data[i * width + plane];
        }
    }
    Ok(planes)
}

/// Interleave byte planes back to fixed-width values.
pub fn byte_plane_decode(data: &[u8], width: usize) -> CpacResult<Vec<u8>> {
    if width == 0 || width == 1 {
        return Ok(data.to_vec());
    }
    let n = data.len();
    if !n.is_multiple_of(width) {
        return Err(CpacError::Transform(format!(
            "byte_plane: data length {n} not divisible by width {width}"
        )));
    }
    let count = n / width;
    let mut output = vec![0u8; n];

    for plane in 0..width {
        let src_offset = plane * count;
        for i in 0..count {
            output[i * width + plane] = data[src_offset + i];
        }
    }
    Ok(output)
}

/// Framed encode: `[width:1][count:4 LE][planes]`.
pub fn byte_plane_encode_framed(data: &[u8], width: usize) -> CpacResult<Vec<u8>> {
    let planes = byte_plane_encode(data, width)?;
    let count = (data.len() / width) as u32;
    let mut out = Vec::with_capacity(5 + planes.len());
    out.push(width as u8);
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&planes);
    Ok(out)
}

/// Framed decode.
pub fn byte_plane_decode_framed(data: &[u8]) -> CpacResult<Vec<u8>> {
    if data.len() < 5 {
        return Err(CpacError::Transform(
            "byte_plane: insufficient header".into(),
        ));
    }
    let width = data[0] as usize;
    let _count = u32::from_le_bytes([data[1], data[2], data[3], data[4]]) as usize;
    byte_plane_decode(&data[5..], width)
}

/// Detect if data benefits from byte-plane splitting.
///
/// Measures per-plane byte frequency concentration. Returns the best width
/// if splitting is beneficial, `None` otherwise.
#[must_use]
pub fn detect_byte_plane_width(data: &[u8]) -> Option<usize> {
    let n = data.len();
    if n < 64 {
        return None;
    }

    let mut best_width: Option<usize> = None;
    let mut best_score = 0.0f64;

    for &width in &[4usize, 8, 2] {
        if !n.is_multiple_of(width) {
            continue;
        }
        let count = n / width;
        if count < 8 {
            continue;
        }

        // Score: average per-plane byte concentration
        let probe_count = count.min(512);
        let mut total_concentration = 0.0f64;

        for plane in 0..width {
            let mut freq = [0u32; 256];
            for i in 0..probe_count {
                freq[data[i * width + plane] as usize] += 1;
            }
            let max_freq = *freq.iter().max().unwrap_or(&0) as f64;
            total_concentration += max_freq / probe_count as f64;
        }

        let avg = total_concentration / width as f64;
        if avg > best_score && avg > 0.3 {
            best_score = avg;
            best_width = Some(width);
        }
    }

    best_width
}

// ---------------------------------------------------------------------------
// TransformNode implementation
// ---------------------------------------------------------------------------

/// Byte-plane split transform node for the compression DAG.
pub struct BytePlaneTransform;

impl TransformNode for BytePlaneTransform {
    fn name(&self) -> &'static str {
        "byte_plane"
    }

    fn id(&self) -> u8 {
        TRANSFORM_ID
    }

    fn accepts(&self) -> &[TypeTag] {
        &[TypeTag::Serial, TypeTag::IntColumn]
    }

    fn produces(&self) -> TypeTag {
        TypeTag::Serial
    }

    fn estimate_gain(&self, input: &CpacType, ctx: &TransformContext) -> Option<f64> {
        if ctx.entropy_estimate > 7.0 || ctx.ascii_ratio > 0.85 {
            return None;
        }
        match input {
            CpacType::Serial(data) => detect_byte_plane_width(data).map(|w| w as f64 * 0.8),
            CpacType::IntColumn {
                original_width, ..
            } => {
                if *original_width >= 2 {
                    Some(f64::from(*original_width) * 0.8)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn encode(&self, input: CpacType, _ctx: &TransformContext) -> CpacResult<(CpacType, Vec<u8>)> {
        match input {
            CpacType::Serial(data) => {
                let width = detect_byte_plane_width(&data).ok_or_else(|| {
                    CpacError::Transform("byte_plane: no suitable width detected".into())
                })?;
                let framed = byte_plane_encode_framed(&data, width)?;
                Ok((CpacType::Serial(framed), Vec::new()))
            }
            CpacType::IntColumn {
                values,
                original_width,
            } => {
                let width = original_width as usize;
                let mut bytes = Vec::with_capacity(values.len() * width);
                for &v in &values {
                    bytes.extend_from_slice(&v.to_le_bytes()[..width]);
                }
                let framed = byte_plane_encode_framed(&bytes, width)?;
                Ok((CpacType::Serial(framed), Vec::new()))
            }
            _ => Err(CpacError::Transform(
                "byte_plane: unsupported input type".into(),
            )),
        }
    }

    fn decode(&self, input: CpacType, _metadata: &[u8]) -> CpacResult<CpacType> {
        match input {
            CpacType::Serial(data) => {
                let decoded = byte_plane_decode_framed(&data)?;
                Ok(CpacType::Serial(decoded))
            }
            _ => Err(CpacError::Transform(
                "byte_plane: unsupported input type".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_4byte() {
        let data: Vec<u8> = (0..400)
            .map(|i| {
                let val = (i / 4) as u32;
                val.to_le_bytes()[(i % 4) as usize]
            })
            .collect();
        let encoded = byte_plane_encode(&data, 4).unwrap();
        let decoded = byte_plane_decode(&encoded, 4).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn roundtrip_framed() {
        let mut data = Vec::with_capacity(400);
        for i in 0u32..100 {
            data.extend_from_slice(&i.to_le_bytes());
        }
        let framed = byte_plane_encode_framed(&data, 4).unwrap();
        let decoded = byte_plane_decode_framed(&framed).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn width_1_is_noop() {
        let data = vec![1, 2, 3, 4, 5];
        assert_eq!(byte_plane_encode(&data, 1).unwrap(), data);
    }

    #[test]
    fn error_not_divisible() {
        assert!(byte_plane_encode(&[1, 2, 3], 4).is_err());
    }

    #[test]
    fn transform_node_roundtrip() {
        let t = BytePlaneTransform;
        // Build structured data with detectable planes
        let mut data = Vec::with_capacity(800);
        for i in 0u32..200 {
            data.extend_from_slice(&i.to_le_bytes());
        }
        let ctx = TransformContext {
            entropy_estimate: 3.0,
            ascii_ratio: 0.1,
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
