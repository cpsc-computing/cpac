// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Transcode compression for lossless image formats.
//!
//! Decodes images to raw pixel buffers, applies byte-plane splitting + delta
//! encoding + zstd compression, and stores codec metadata so the original
//! file can be reconstructed bit-identically on decompression.
//!
//! ## Wire format (CPTC)
//!
//! ```text
//! [magic:4 "CPTC"]
//! [version:1]
//! [codec_id:1]       — 1=PNG, 2=BMP, 3=TIFF, 4=WebP-lossless
//! [width:4 LE]
//! [height:4 LE]
//! [channels:1]       — 1=gray, 2=gray+alpha, 3=RGB, 4=RGBA
//! [bit_depth:1]      — 8 or 16
//! [original_len:4 LE] — length of the original encoded file
//! [payload_len:4 LE]  — length of the compressed pixel payload
//! [payload...]       — zstd-compressed byte-plane-split pixel data
//! [original_file...] — NOT stored; reconstructed from pixels + codec metadata
//! ```

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

pub mod detect;

use cpac_types::{CpacError, CpacResult, ImageFormat};
use image::{DynamicImage, GenericImageView, ImageReader};
use std::io::Cursor;

/// CPTC wire format magic bytes.
const CPTC_MAGIC: &[u8; 4] = b"CPTC";

/// Current CPTC wire format version.
const CPTC_VERSION: u8 = 1;

/// CPTC header size: magic(4) + version(1) + codec(1) + w(4) + h(4) +
/// channels(1) + depth(1) + original_len(4) + payload_len(4) = 24 bytes.
const CPTC_HEADER_SIZE: usize = 24;

// ---------------------------------------------------------------------------
// Byte-plane splitting
// ---------------------------------------------------------------------------

/// Split interleaved pixel data into per-channel planes.
///
/// E.g. for 3-channel RGB data [R0,G0,B0, R1,G1,B1, ...] produces
/// [R0,R1,..., G0,G1,..., B0,B1,...].
fn byte_plane_split(data: &[u8], channels: usize) -> Vec<u8> {
    if channels <= 1 {
        return data.to_vec();
    }
    let pixels = data.len() / channels;
    let mut planes = vec![0u8; data.len()];
    for ch in 0..channels {
        for px in 0..pixels {
            planes[ch * pixels + px] = data[px * channels + ch];
        }
    }
    planes
}

/// Reverse byte-plane split: interleave per-channel planes back.
fn byte_plane_unsplit(data: &[u8], channels: usize) -> Vec<u8> {
    if channels <= 1 {
        return data.to_vec();
    }
    let pixels = data.len() / channels;
    let mut interleaved = vec![0u8; data.len()];
    for ch in 0..channels {
        for px in 0..pixels {
            interleaved[px * channels + ch] = data[ch * pixels + px];
        }
    }
    interleaved
}

/// Simple delta encoding (byte-level, wrap-around).
fn delta_encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut prev = 0u8;
    for &b in data {
        out.push(b.wrapping_sub(prev));
        prev = b;
    }
    out
}

/// Reverse delta encoding.
fn delta_decode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut prev = 0u8;
    for &d in data {
        let b = prev.wrapping_add(d);
        out.push(b);
        prev = b;
    }
    out
}

// ---------------------------------------------------------------------------
// Compress
// ---------------------------------------------------------------------------

/// Transcode-compress image data.
///
/// `data` must be a complete encoded image file (PNG, BMP, etc.).
/// Returns a CPTC frame on success, or an error if the data is not a
/// supported lossless image.
pub fn transcode_compress(data: &[u8]) -> CpacResult<Vec<u8>> {
    let format = detect::detect_image_format(data)
        .ok_or_else(|| CpacError::Other("transcode: unsupported image format".into()))?;

    // Decode to raw pixels
    let reader = ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .map_err(|e| CpacError::Other(format!("transcode: image read error: {e}")))?;

    let img: DynamicImage = reader
        .decode()
        .map_err(|e| CpacError::Other(format!("transcode: image decode error: {e}")))?;

    let (width, height) = img.dimensions();
    let channels = img.color().channel_count() as usize;
    let bit_depth: u8 = if img.color().bytes_per_pixel() / img.color().channel_count() > 1 {
        16
    } else {
        8
    };

    let raw_pixels = img.as_bytes();

    // Byte-plane split → delta → zstd
    let split = byte_plane_split(raw_pixels, channels);
    let delta = delta_encode(&split);
    let compressed = zstd::bulk::compress(&delta, 3)
        .map_err(|e| CpacError::CompressFailed(format!("transcode zstd: {e}")))?;

    // Build CPTC frame
    let mut frame = Vec::with_capacity(CPTC_HEADER_SIZE + compressed.len());
    frame.extend_from_slice(CPTC_MAGIC);
    frame.push(CPTC_VERSION);
    frame.push(detect::codec_id(format));
    frame.extend_from_slice(&width.to_le_bytes());
    frame.extend_from_slice(&height.to_le_bytes());
    frame.push(channels as u8);
    frame.push(bit_depth);
    frame.extend_from_slice(&(data.len() as u32).to_le_bytes());
    frame.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
    frame.extend_from_slice(&compressed);

    Ok(frame)
}

// ---------------------------------------------------------------------------
// Decompress
// ---------------------------------------------------------------------------

/// Check if `data` is a CPTC transcode frame.
#[must_use]
pub fn is_transcode_frame(data: &[u8]) -> bool {
    data.len() >= CPTC_HEADER_SIZE && data[..4] == *CPTC_MAGIC
}

/// Transcode-decompress a CPTC frame back to the raw pixel buffer.
///
/// Returns `(width, height, channels, bit_depth, raw_pixels)`.
///
/// Note: This returns the *raw pixel data*, not the re-encoded image file.
/// Re-encoding to the original format (PNG, BMP, etc.) requires the caller
/// to use the `image` crate with the returned metadata.
pub fn transcode_decompress(frame: &[u8]) -> CpacResult<(u32, u32, u8, u8, ImageFormat, Vec<u8>)> {
    if frame.len() < CPTC_HEADER_SIZE || frame[..4] != *CPTC_MAGIC {
        return Err(CpacError::InvalidFrame("not a CPTC frame".into()));
    }
    let version = frame[4];
    if version != CPTC_VERSION {
        return Err(CpacError::InvalidFrame(format!(
            "unsupported CPTC version {version}"
        )));
    }
    let codec_id = frame[5];
    let format = detect::format_from_id(codec_id)
        .ok_or_else(|| CpacError::InvalidFrame(format!("unknown CPTC codec {codec_id}")))?;
    let width = u32::from_le_bytes([frame[6], frame[7], frame[8], frame[9]]);
    let height = u32::from_le_bytes([frame[10], frame[11], frame[12], frame[13]]);
    let channels = frame[14];
    let bit_depth = frame[15];
    let _original_len = u32::from_le_bytes([frame[16], frame[17], frame[18], frame[19]]);
    let payload_len = u32::from_le_bytes([frame[20], frame[21], frame[22], frame[23]]) as usize;

    if CPTC_HEADER_SIZE + payload_len > frame.len() {
        return Err(CpacError::InvalidFrame("CPTC payload truncated".into()));
    }
    let payload = &frame[CPTC_HEADER_SIZE..CPTC_HEADER_SIZE + payload_len];

    let bytes_per_pixel = if bit_depth > 8 { 2 } else { 1 };
    let expected_size = width as usize * height as usize * channels as usize * bytes_per_pixel;

    // zstd decompress → delta decode → byte-plane unsplit
    let delta = zstd::bulk::decompress(payload, expected_size)
        .map_err(|e| CpacError::DecompressFailed(format!("transcode zstd: {e}")))?;
    let split = delta_decode(&delta);
    let raw_pixels = byte_plane_unsplit(&split, channels as usize);

    Ok((width, height, channels, bit_depth, format, raw_pixels))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a minimal 4×4 RGBA PNG in memory.
    fn create_test_png() -> Vec<u8> {
        let mut img = image::RgbaImage::new(4, 4);
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            *pixel = image::Rgba([(x * 60) as u8, (y * 60) as u8, ((x + y) * 30) as u8, 255]);
        }
        let mut buf = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut buf);
        img.write_with_encoder(encoder).unwrap();
        buf
    }

    #[test]
    fn png_roundtrip() {
        let png_data = create_test_png();

        // Compress
        let frame = transcode_compress(&png_data).unwrap();
        assert!(is_transcode_frame(&frame));
        assert!(frame.len() < png_data.len() + CPTC_HEADER_SIZE + 64); // sanity

        // Decompress
        let (w, h, ch, depth, fmt, pixels) = transcode_decompress(&frame).unwrap();
        assert_eq!(w, 4);
        assert_eq!(h, 4);
        assert_eq!(ch, 4); // RGBA
        assert_eq!(depth, 8);
        assert_eq!(fmt, ImageFormat::Png);

        // Verify pixel data matches original
        let reader = ImageReader::new(Cursor::new(&png_data))
            .with_guessed_format()
            .unwrap();
        let original_img = reader.decode().unwrap();
        assert_eq!(pixels, original_img.as_bytes());
    }

    #[test]
    fn non_image_rejected() {
        let result = transcode_compress(b"hello world this is not an image");
        assert!(result.is_err());
    }

    #[test]
    fn byte_plane_roundtrip() {
        let data = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        let split = byte_plane_split(&data, 3);
        let unsplit = byte_plane_unsplit(&split, 3);
        assert_eq!(unsplit, data);
    }

    #[test]
    fn delta_roundtrip() {
        let data = vec![10, 20, 15, 30, 25, 100, 0, 255];
        let encoded = delta_encode(&data);
        let decoded = delta_decode(&encoded);
        assert_eq!(decoded, data);
    }
}
