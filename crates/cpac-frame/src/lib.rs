// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
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
//! | original_size (4B LE) | dag_descriptor_len (2B LE) | msn_metadata_len (4B LE)
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

/// Flag bit: MSN metadata is embedded (inline) inside the compressed payload
/// rather than stored as a raw byte blob between the frame header and payload.
/// When set, `msn_metadata_len` in the CP2 header records the *uncompressed*
/// byte length of the metadata so the decompressor can split the decompressed
/// buffer at that offset.
pub const FLAG_MSN_INLINE: u16 = 0x0001;

/// Minimum header size for CP (magic + version + flags + backend + `orig_size` + `dag_len`).
const MIN_HEADER_CP: usize = 2 + 1 + 2 + 1 + 4 + 2; // 12 bytes

/// Minimum header size for CP2 (adds `msn_metadata_len` as u32).
const MIN_HEADER_CP2: usize = MIN_HEADER_CP + 4; // 16 bytes

/// Frame header parsed from wire format.
#[derive(Clone, Debug)]
pub struct FrameHeader {
    pub version: u8,
    pub flags: u16,
    pub backend: Backend,
    pub original_size: u32,
    pub dag_descriptor: Vec<u8>,
    /// MSN metadata bytes (CP2 legacy format only — empty when `FLAG_MSN_INLINE` is set).
    pub msn_metadata: Vec<u8>,
    /// Uncompressed length of MSN metadata.
    /// - Legacy CP2 (flags & FLAG_MSN_INLINE == 0): equals `msn_metadata.len()`.
    /// - Inline CP2 (flags & FLAG_MSN_INLINE != 0): split point inside the decompressed
    ///   payload where MSN metadata ends and the TP-framed residual begins.
    /// - CP v1: always 0.
    pub msn_meta_len: usize,
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
///
/// The metadata bytes are stored **uncompressed** between the header and the
/// compressed payload (legacy layout, `flags = 0`).
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

/// Encode a frame using CP2 format (version 2) with **inline** MSN metadata.
///
/// Unlike [`encode_frame_cp2`], the metadata is NOT stored as a separate raw
/// blob.  The caller is expected to have prepended the uncompressed metadata to
/// the data before entropy-coding it, so the `payload` already contains the
/// metadata embedded at its start.  `msn_meta_len` records the *uncompressed*
/// byte length of that metadata so the decompressor knows where to split.
///
/// Wire layout (same fixed header as CP2, `FLAG_MSN_INLINE` set in `flags`):
/// ```text
/// "CP"(2) | v2(1) | flags=0x0001(2) | backend(1) | orig_size(4) | dag_len(2)
/// | msn_meta_len(4) | dag_descriptor | compressed_payload
/// ```
#[must_use]
pub fn encode_frame_cp2_inline(
    payload: &[u8],
    backend: Backend,
    original_size: usize,
    dag_descriptor: &[u8],
    msn_meta_len: usize,
) -> Vec<u8> {
    let orig = original_size as u32;
    let dag_len = dag_descriptor.len() as u16;
    let flags = FLAG_MSN_INLINE;

    // Header (16 B) + dag_descriptor + payload; no raw metadata blob.
    let total = MIN_HEADER_CP2 + dag_descriptor.len() + payload.len();
    let mut buf = Vec::with_capacity(total);

    buf.extend_from_slice(MAGIC);
    buf.push(VERSION_CP2);
    buf.extend_from_slice(&flags.to_le_bytes());
    buf.push(backend.id());
    buf.extend_from_slice(&orig.to_le_bytes());
    buf.extend_from_slice(&dag_len.to_le_bytes());
    buf.extend_from_slice(&(msn_meta_len as u32).to_le_bytes());
    buf.extend_from_slice(dag_descriptor);
    // Payload already contains [msn_metadata][tp_framed_residual] compressed together.
    buf.extend_from_slice(payload);

    buf
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
        let msn_len = msn_metadata.len() as u32;
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

    let (msn_metadata, msn_meta_len, payload_start) = if version == VERSION_CP2 {
        // CP2: read MSN metadata length from header
        if data.len() < MIN_HEADER_CP2 {
            return Err(CpacError::InvalidFrame(format!(
                "CP2 frame too short: {} < {MIN_HEADER_CP2}",
                data.len()
            )));
        }
        let msn_len = u32::from_le_bytes([data[12], data[13], data[14], data[15]]) as usize;
        let dag_end = MIN_HEADER_CP2 + dag_len;

        if flags & FLAG_MSN_INLINE != 0 {
            // Inline format: no raw metadata blob between header and payload.
            // The metadata is embedded at the start of the compressed payload;
            // msn_len records its *uncompressed* byte length for the decompressor.
            if data.len() < dag_end {
                return Err(CpacError::InvalidFrame("truncated DAG descriptor (inline)".into()));
            }
            (Vec::new(), msn_len, dag_end)
        } else {
            // Legacy format: raw metadata blob follows dag_descriptor.
            let msn_end = dag_end + msn_len;
            if data.len() < msn_end {
                return Err(CpacError::InvalidFrame("truncated MSN metadata".into()));
            }
            let msn_metadata = data[dag_end..msn_end].to_vec();
            (msn_metadata, msn_len, msn_end)
        }
    } else {
        // CP: no MSN metadata
        let dag_end = MIN_HEADER_CP + dag_len;
        if data.len() < dag_end {
            return Err(CpacError::InvalidFrame("truncated DAG descriptor".into()));
        }
        (Vec::new(), 0, dag_end)
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
            msn_meta_len,
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
    fn encode_decode_cp2_large_msn() {
        // Regression test: msn_metadata_len was u16, silently truncating when > 65535 bytes.
        // 134281 as u16 = 3209; this test ensures the u32 field handles large metadata correctly.
        let payload = b"compressed";
        let msn = vec![0xABu8; 70_000]; // > 65535, previously caused u16 truncation

        let frame = encode_frame_cp2(payload, Backend::Zstd, 70_000, b"", &msn);
        let (header, decoded_payload) = decode_frame(&frame).unwrap();

        assert_eq!(header.version, VERSION_CP2);
        assert_eq!(header.msn_metadata.len(), 70_000, "MSN metadata length truncated");
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
        assert_eq!(cp_header.msn_meta_len, 0);

        // CP2 frame should decode with MSN
        let cp2_frame = encode_frame_cp2(b"data", Backend::Raw, 4, b"", b"msn");
        let (cp2_header, _) = decode_frame(&cp2_frame).unwrap();
        assert_eq!(cp2_header.version, VERSION_CP2);
        assert_eq!(cp2_header.msn_metadata, b"msn");
        assert_eq!(cp2_header.msn_meta_len, 3);
    }

    #[test]
    fn encode_decode_cp2_inline() {
        // Inline format: metadata embedded inside payload, no raw blob in frame.
        let msn_meta = b"meta-bytes-here";
        // Simulate combined payload: [metadata][residual]
        let residual = b"residual-data";
        let mut combined = msn_meta.to_vec();
        combined.extend_from_slice(residual);

        let frame = encode_frame_cp2_inline(&combined, Backend::Zstd, 200, b"", msn_meta.len());
        let (header, decoded_payload) = decode_frame(&frame).unwrap();

        assert_eq!(header.version, VERSION_CP2);
        assert_ne!(header.flags & FLAG_MSN_INLINE, 0);
        assert!(header.msn_metadata.is_empty(), "no raw blob for inline format");
        assert_eq!(header.msn_meta_len, msn_meta.len());
        assert_eq!(decoded_payload, combined.as_slice());

        // Caller can split the payload at msn_meta_len to get metadata + residual.
        let (extracted_meta, extracted_residual) = decoded_payload.split_at(header.msn_meta_len);
        assert_eq!(extracted_meta, msn_meta);
        assert_eq!(extracted_residual, residual);
    }

    #[test]
    fn encode_decode_cp2_inline_with_dag() {
        let msn_meta = b"short-meta";
        let payload = b"the-compressed-data";
        let dag = b"dag-info";
        let mut combined = msn_meta.to_vec();
        combined.extend_from_slice(payload);

        let frame =
            encode_frame_cp2_inline(&combined, Backend::Brotli, 500, dag, msn_meta.len());
        let (header, decoded_payload) = decode_frame(&frame).unwrap();

        assert_eq!(header.backend, Backend::Brotli);
        assert_eq!(header.original_size, 500);
        assert_eq!(header.dag_descriptor, dag);
        assert_ne!(header.flags & FLAG_MSN_INLINE, 0);
        assert_eq!(header.msn_meta_len, msn_meta.len());
        assert_eq!(decoded_payload, combined.as_slice());
    }
}
