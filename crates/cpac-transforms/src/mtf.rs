// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Move-to-Front (MTF) transform for improving compression after BWT.

use cpac_types::CpacResult;

/// Move-to-Front encode.
///
/// Transforms data by maintaining a list of symbols and outputting
/// the index of each symbol, moving it to the front after each occurrence.
pub fn mtf_encode(data: &[u8]) -> CpacResult<Vec<u8>> {
    if data.is_empty() {
        return Ok(Vec::new());
    }

    // Initialize symbol list (0..255)
    let mut list: Vec<u8> = (0..=255).collect();
    let mut output = Vec::with_capacity(data.len());

    for &byte in data {
        // Find position of byte in list
        let pos = list.iter().position(|&x| x == byte).unwrap();
        output.push(pos as u8);

        // Move to front
        if pos > 0 {
            list.remove(pos);
            list.insert(0, byte);
        }
    }

    Ok(output)
}

/// Move-to-Front decode.
pub fn mtf_decode(data: &[u8]) -> CpacResult<Vec<u8>> {
    if data.is_empty() {
        return Ok(Vec::new());
    }

    // Initialize symbol list (0..255)
    let mut list: Vec<u8> = (0..=255).collect();
    let mut output = Vec::with_capacity(data.len());

    for &idx in data {
        let byte = list[idx as usize];
        output.push(byte);

        // Move to front
        if idx > 0 {
            list.remove(idx as usize);
            list.insert(0, byte);
        }
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mtf_roundtrip_simple() {
        let data = b"banana";
        let encoded = mtf_encode(data).unwrap();
        let decoded = mtf_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn mtf_roundtrip_repeated() {
        let data = b"aaaabbbbccccdddd";
        let encoded = mtf_encode(data).unwrap();
        let decoded = mtf_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn mtf_empty() {
        let encoded = mtf_encode(b"").unwrap();
        assert_eq!(encoded, b"");
        let decoded = mtf_decode(&encoded).unwrap();
        assert_eq!(decoded, b"");
    }

    #[test]
    fn mtf_property_repeated_chars() {
        // Repeated characters should produce many zeros
        let data = b"aaaaaaaaaa";
        let encoded = mtf_encode(data).unwrap();
        // First occurrence at position 'a' (97), rest at position 0
        assert_eq!(encoded[0], 97);
        for &val in &encoded[1..] {
            assert_eq!(val, 0);
        }
    }

    #[test]
    fn mtf_all_symbols() {
        let data: Vec<u8> = (0..=255).collect();
        let encoded = mtf_encode(&data).unwrap();
        let decoded = mtf_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }
}
