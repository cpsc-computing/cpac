// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Magic-byte detection for lossless image formats.

use cpac_types::ImageFormat;

/// Detect a lossless image format from the first bytes of `data`.
///
/// Returns `None` if the data does not match any supported format.
#[must_use]
pub fn detect_image_format(data: &[u8]) -> Option<ImageFormat> {
    if data.len() >= 8 && data[..8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A] {
        return Some(ImageFormat::Png);
    }
    if data.len() >= 2 && data[..2] == [0x42, 0x4D] {
        return Some(ImageFormat::Bmp);
    }
    if data.len() >= 4
        && (data[..4] == [0x49, 0x49, 0x2A, 0x00] || data[..4] == [0x4D, 0x4D, 0x00, 0x2A])
    {
        return Some(ImageFormat::Tiff);
    }
    if data.len() >= 16
        && data[..4] == [0x52, 0x49, 0x46, 0x46]
        && data[8..12] == [0x57, 0x45, 0x42, 0x50]
        && data[12..16] == [0x56, 0x50, 0x38, 0x4C]
    {
        return Some(ImageFormat::WebPLossless);
    }
    None
}

/// Wire ID for the image codec stored in the CPTC frame header.
#[must_use]
pub fn codec_id(fmt: ImageFormat) -> u8 {
    match fmt {
        ImageFormat::Png => 1,
        ImageFormat::Bmp => 2,
        ImageFormat::Tiff => 3,
        ImageFormat::WebPLossless => 4,
    }
}

/// Reverse-map a wire codec ID to an [`ImageFormat`].
pub fn format_from_id(id: u8) -> Option<ImageFormat> {
    match id {
        1 => Some(ImageFormat::Png),
        2 => Some(ImageFormat::Bmp),
        3 => Some(ImageFormat::Tiff),
        4 => Some(ImageFormat::WebPLossless),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_png() {
        let header = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00];
        assert_eq!(detect_image_format(&header), Some(ImageFormat::Png));
    }

    #[test]
    fn detect_bmp() {
        let header = [0x42, 0x4D, 0x00, 0x00];
        assert_eq!(detect_image_format(&header), Some(ImageFormat::Bmp));
    }

    #[test]
    fn detect_tiff_le() {
        let header = [0x49, 0x49, 0x2A, 0x00, 0x00];
        assert_eq!(detect_image_format(&header), Some(ImageFormat::Tiff));
    }

    #[test]
    fn detect_tiff_be() {
        let header = [0x4D, 0x4D, 0x00, 0x2A, 0x00];
        assert_eq!(detect_image_format(&header), Some(ImageFormat::Tiff));
    }

    #[test]
    fn detect_none() {
        assert_eq!(detect_image_format(b"hello world"), None);
    }

    #[test]
    fn codec_id_roundtrip() {
        for fmt in [ImageFormat::Png, ImageFormat::Bmp, ImageFormat::Tiff, ImageFormat::WebPLossless] {
            assert_eq!(format_from_id(codec_id(fmt)), Some(fmt));
        }
    }
}
