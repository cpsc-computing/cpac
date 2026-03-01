// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! CPAC wire format: self-describing frame encode/decode.
//!
//! Frame layout:
//! ```text
//! "CP" (2B) | version (1B) | flags (2B) | backend_id (1B)
//! | original_size (4B LE) | dag_descriptor_len (2B LE) | dag_descriptor
//! | payload
//! ```

use cpac_types::{Backend, CpacError, CpacResult};

/// Magic bytes identifying a CPAC frame.
pub const MAGIC: &[u8; 2] = b"CP";

/// Current frame format version.
pub const VERSION: u8 = 1;

/// Minimum header size (magic + version + flags + backend + orig_size + dag_len).
const MIN_HEADER: usize = 2 + 1 + 2 + 1 + 4 + 2; // 12 bytes

/// Frame header parsed from wire format.
#[derive(Clone, Debug)]
pub struct FrameHeader {
    pub version: u8,
    pub flags: u16,
    pub backend: Backend,
    pub original_size: u32,
    pub dag_descriptor: Vec<u8>,
}

/// Encode a frame containing the given compressed payload.
pub fn encode_frame(
    payload: &[u8],
    backend: Backend,
    original_size: usize,
    dag_descriptor: &[u8],
) -> Vec<u8> {
    let orig = original_size as u32;
    let dag_len = dag_descriptor.len() as u16;
    let flags: u16 = 0;

    let total = MIN_HEADER + dag_descriptor.len() + payload.len();
    let mut buf = Vec::with_capacity(total);

    buf.extend_from_slice(MAGIC);
    buf.push(VERSION);
    buf.extend_from_slice(&flags.to_le_bytes());
    buf.push(backend.id());
    buf.extend_from_slice(&orig.to_le_bytes());
    buf.extend_from_slice(&dag_len.to_le_bytes());
    buf.extend_from_slice(dag_descriptor);
    buf.extend_from_slice(payload);

    buf
}

/// Decode a frame, returning the header and payload slice.
pub fn decode_frame(data: &[u8]) -> CpacResult<(FrameHeader, &[u8])> {
    if data.len() < MIN_HEADER {
        return Err(CpacError::InvalidFrame(format!(
            "too short: {} < {MIN_HEADER}",
            data.len()
        )));
    }

    if &data[0..2] != MAGIC {
        return Err(CpacError::InvalidFrame("bad magic bytes".into()));
    }

    let version = data[2];
    if version != VERSION {
        return Err(CpacError::InvalidFrame(format!(
            "unsupported version: {version}"
        )));
    }

    let flags = u16::from_le_bytes([data[3], data[4]]);
    let backend = Backend::from_id(data[5])?;
    let original_size = u32::from_le_bytes([data[6], data[7], data[8], data[9]]);
    let dag_len = u16::from_le_bytes([data[10], data[11]]) as usize;

    let dag_end = MIN_HEADER + dag_len;
    if data.len() < dag_end {
        return Err(CpacError::InvalidFrame("truncated DAG descriptor".into()));
    }

    let dag_descriptor = data[MIN_HEADER..dag_end].to_vec();
    let payload = &data[dag_end..];

    Ok((
        FrameHeader {
            version,
            flags,
            backend,
            original_size,
            dag_descriptor,
        },
        payload,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        let payload = b"compressed data here";
        let original_size = 1234usize;
        let dag = b"";

        let frame = encode_frame(payload, Backend::Zstd, original_size, dag);
        let (header, decoded_payload) = decode_frame(&frame).unwrap();

        assert_eq!(header.version, VERSION);
        assert_eq!(header.backend, Backend::Zstd);
        assert_eq!(header.original_size, 1234);
        assert!(header.dag_descriptor.is_empty());
        assert_eq!(decoded_payload, payload);
    }

    #[test]
    fn encode_decode_with_dag() {
        let payload = b"payload";
        let dag = b"dag-meta";

        let frame = encode_frame(payload, Backend::Brotli, 42, dag);
        let (header, decoded_payload) = decode_frame(&frame).unwrap();

        assert_eq!(header.backend, Backend::Brotli);
        assert_eq!(header.original_size, 42);
        assert_eq!(header.dag_descriptor, dag);
        assert_eq!(decoded_payload, payload);
    }

    #[test]
    fn decode_too_short() {
        assert!(decode_frame(b"CP").is_err());
    }

    #[test]
    fn decode_bad_magic() {
        let mut frame = encode_frame(b"x", Backend::Raw, 1, b"");
        frame[0] = b'X';
        assert!(decode_frame(&frame).is_err());
    }
}
