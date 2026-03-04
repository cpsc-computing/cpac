// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! CPAC wire format: self-describing frame encode/decode.
//!
//! Frame layout:
//!
//! ## CP (Version 1)
//! ```text
//! "CP" (2B) | version=1 (1B) | flags (2B) | backend_id (1B)
//! | original_size (4B LE) | dag_descriptor_len (2B LE) | dag_descriptor
//! | payload
//! ```
//!
//! ## CP2 (Version 2 - with MSN support)
//! ```text
//! "CP" (2B) | version=2 (1B) | flags (2B) | backend_id (1B)
//! | original_size (4B LE) | dag_descriptor_len (2B LE) | msn_metadata_len (2B LE)
//! | dag_descriptor | msn_metadata | payload
//! ```

#![allow(
    clippy::cast_possible_truncation,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

use cpac_types::{Backend, CpacError, CpacResult};

/// Magic bytes identifying a CPAC frame.
pub const MAGIC: &[u8; 2] = b"CP";

/// CP format version (legacy).
pub const VERSION_CP: u8 = 1;

/// CP2 format version (with MSN).
pub const VERSION_CP2: u8 = 2;

/// Current default version.
pub const VERSION: u8 = VERSION_CP;

/// Minimum header size for CP (magic + version + flags + backend + `orig_size` + `dag_len`).
const MIN_HEADER_CP: usize = 2 + 1 + 2 + 1 + 4 + 2; // 12 bytes

/// Minimum header size for CP2 (adds `msn_metadata_len`).
const MIN_HEADER_CP2: usize = MIN_HEADER_CP + 2; // 14 bytes

/// Frame header parsed from wire format.
#[derive(Clone, Debug)]
pub struct FrameHeader {
    pub version: u8,
    pub flags: u16,
    pub backend: Backend,
    pub original_size: u32,
    pub dag_descriptor: Vec<u8>,
    /// MSN metadata (CP2 only, empty for CP)
    pub msn_metadata: Vec<u8>,
}

/// Encode a frame using CP format (version 1).
#[must_use]
pub fn encode_frame(
    payload: &[u8],
    backend: Backend,
    original_size: usize,
    dag_descriptor: &[u8],
) -> Vec<u8> {
    encode_frame_with_version(
        payload,
        backend,
        original_size,
        dag_descriptor,
        &[],
        VERSION_CP,
    )
}

/// Encode a frame using CP2 format (version 2) with MSN metadata.
#[must_use]
pub fn encode_frame_cp2(
    payload: &[u8],
    backend: Backend,
    original_size: usize,
    dag_descriptor: &[u8],
    msn_metadata: &[u8],
) -> Vec<u8> {
    encode_frame_with_version(
        payload,
        backend,
        original_size,
        dag_descriptor,
        msn_metadata,
        VERSION_CP2,
    )
}

fn encode_frame_with_version(
    payload: &[u8],
    backend: Backend,
    original_size: usize,
    dag_descriptor: &[u8],
    msn_metadata: &[u8],
    version: u8,
) -> Vec<u8> {
    let orig = original_size as u32;
    let dag_len = dag_descriptor.len() as u16;
    let flags: u16 = 0;

    let total = if version == VERSION_CP2 {
        MIN_HEADER_CP2 + dag_descriptor.len() + msn_metadata.len() + payload.len()
    } else {
        MIN_HEADER_CP + dag_descriptor.len() + payload.len()
    };

    let mut buf = Vec::with_capacity(total);

    buf.extend_from_slice(MAGIC);
    buf.push(version);
    buf.extend_from_slice(&flags.to_le_bytes());
    buf.push(backend.id());
    buf.extend_from_slice(&orig.to_le_bytes());
    buf.extend_from_slice(&dag_len.to_le_bytes());

    if version == VERSION_CP2 {
        let msn_len = msn_metadata.len() as u16;
        buf.extend_from_slice(&msn_len.to_le_bytes());
    }

    buf.extend_from_slice(dag_descriptor);
    if version == VERSION_CP2 {
        buf.extend_from_slice(msn_metadata);
    }
    buf.extend_from_slice(payload);

    buf
}

/// Decode a frame, returning the header and payload slice.
/// Supports both CP (v1) and CP2 (v2) formats.
pub fn decode_frame(data: &[u8]) -> CpacResult<(FrameHeader, &[u8])> {
    // Check minimum size
    if data.len() < MIN_HEADER_CP {
        return Err(CpacError::InvalidFrame(format!(
            "too short: {} < {MIN_HEADER_CP}",
            data.len()
        )));
    }

    if &data[0..2] != MAGIC {
        return Err(CpacError::InvalidFrame("bad magic bytes".into()));
    }

    let version = data[2];
    if version != VERSION_CP && version != VERSION_CP2 {
        return Err(CpacError::InvalidFrame(format!(
            "unsupported version: {version}"
        )));
    }

    let flags = u16::from_le_bytes([data[3], data[4]]);
    let backend = Backend::from_id(data[5])?;
    let original_size = u32::from_le_bytes([data[6], data[7], data[8], data[9]]);
    let dag_len = u16::from_le_bytes([data[10], data[11]]) as usize;

    let (_msn_len, msn_metadata, payload_start) = if version == VERSION_CP2 {
        // CP2: read MSN metadata length
        if data.len() < MIN_HEADER_CP2 {
            return Err(CpacError::InvalidFrame(format!(
                "CP2 frame too short: {} < {MIN_HEADER_CP2}",
                data.len()
            )));
        }
        let msn_len = u16::from_le_bytes([data[12], data[13]]) as usize;
        let dag_end = MIN_HEADER_CP2 + dag_len;
        let msn_end = dag_end + msn_len;

        if data.len() < msn_end {
            return Err(CpacError::InvalidFrame("truncated MSN metadata".into()));
        }

        let msn_metadata = data[dag_end..msn_end].to_vec();
        (msn_len, msn_metadata, msn_end)
    } else {
        // CP: no MSN metadata
        let dag_end = MIN_HEADER_CP + dag_len;
        if data.len() < dag_end {
            return Err(CpacError::InvalidFrame("truncated DAG descriptor".into()));
        }
        (0, Vec::new(), dag_end)
    };

    let dag_descriptor = if version == VERSION_CP2 {
        data[MIN_HEADER_CP2..MIN_HEADER_CP2 + dag_len].to_vec()
    } else {
        data[MIN_HEADER_CP..MIN_HEADER_CP + dag_len].to_vec()
    };

    let payload = &data[payload_start..];

    Ok((
        FrameHeader {
            version,
            flags,
            backend,
            original_size,
            dag_descriptor,
            msn_metadata,
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

        assert_eq!(header.version, VERSION_CP);
        assert_eq!(header.backend, Backend::Zstd);
        assert_eq!(header.original_size, 1234);
        assert!(header.dag_descriptor.is_empty());
        assert!(header.msn_metadata.is_empty());
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
        assert!(header.msn_metadata.is_empty());
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

    #[test]
    fn encode_decode_cp2_with_msn() {
        let payload = b"compressed";
        let dag = b"dag";
        let msn = b"msn-metadata-here";

        let frame = encode_frame_cp2(payload, Backend::Zstd, 100, dag, msn);
        let (header, decoded_payload) = decode_frame(&frame).unwrap();

        assert_eq!(header.version, VERSION_CP2);
        assert_eq!(header.backend, Backend::Zstd);
        assert_eq!(header.original_size, 100);
        assert_eq!(header.dag_descriptor, dag);
        assert_eq!(header.msn_metadata, msn);
        assert_eq!(decoded_payload, payload);
    }

    #[test]
    fn cp_and_cp2_interop() {
        // CP frame should decode with empty MSN
        let cp_frame = encode_frame(b"data", Backend::Raw, 4, b"");
        let (cp_header, _) = decode_frame(&cp_frame).unwrap();
        assert_eq!(cp_header.version, VERSION_CP);
        assert!(cp_header.msn_metadata.is_empty());

        // CP2 frame should decode with MSN
        let cp2_frame = encode_frame_cp2(b"data", Backend::Raw, 4, b"", b"msn");
        let (cp2_header, _) = decode_frame(&cp2_frame).unwrap();
        assert_eq!(cp2_header.version, VERSION_CP2);
        assert_eq!(cp2_header.msn_metadata, b"msn");
    }
}
