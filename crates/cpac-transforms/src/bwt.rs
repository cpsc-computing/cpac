// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Burrows-Wheeler Transform (BWT) using suffix array construction.

use cpac_types::{CpacError, CpacResult};

/// Burrows-Wheeler Transform forward.
///
/// Returns (transformed_data, original_index).
pub fn bwt_encode(data: &[u8]) -> CpacResult<(Vec<u8>, usize)> {
    if data.is_empty() {
        return Ok((Vec::new(), 0));
    }

    let n = data.len();
    
    // Build suffix array using simple O(n^2 log n) algorithm
    // For production, use SA-IS or divsufsort
    let mut suffixes: Vec<usize> = (0..n).collect();
    suffixes.sort_by(|&a, &b| {
        data[a..].cmp(&data[b..])
    });

    // Find original position
    let original_idx = suffixes.iter().position(|&i| i == 0).unwrap();

    // Build BWT output (last column of rotation matrix)
    let mut output = Vec::with_capacity(n);
    for &idx in &suffixes {
        let prev = if idx == 0 { n - 1 } else { idx - 1 };
        output.push(data[prev]);
    }

    Ok((output, original_idx))
}

/// Burrows-Wheeler Transform inverse.
pub fn bwt_decode(data: &[u8], original_idx: usize) -> CpacResult<Vec<u8>> {
    if data.is_empty() {
        return Ok(Vec::new());
    }

    let n = data.len();
    if original_idx >= n {
        return Err(CpacError::Transform(format!(
            "invalid original index: {original_idx} >= {n}"
        )));
    }

    // Build first column (sorted)
    let mut first_col = data.to_vec();
    first_col.sort_unstable();

    // Build transformation vector
    let mut count = vec![0usize; 256];
    let mut transform = vec![0usize; n];

    for &byte in data {
        count[byte as usize] += 1;
    }

    let mut cumulative = vec![0usize; 256];
    for i in 1..256 {
        cumulative[i] = cumulative[i - 1] + count[i - 1];
    }

    for i in 0..n {
        let byte = data[i];
        transform[cumulative[byte as usize]] = i;
        cumulative[byte as usize] += 1;
    }

    // Reconstruct original string
    let mut output = Vec::with_capacity(n);
    let mut idx = original_idx;
    for _ in 0..n {
        output.push(first_col[idx]);
        idx = transform[idx];
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bwt_roundtrip_simple() {
        let data = b"banana";
        let (encoded, idx) = bwt_encode(data).unwrap();
        let decoded = bwt_decode(&encoded, idx).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn bwt_roundtrip_repeated() {
        let data = b"aaaabbbbcccc";
        let (encoded, idx) = bwt_encode(data).unwrap();
        let decoded = bwt_decode(&encoded, idx).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn bwt_empty() {
        let (encoded, idx) = bwt_encode(b"").unwrap();
        assert_eq!(encoded, b"");
        assert_eq!(idx, 0);
        let decoded = bwt_decode(&encoded, idx).unwrap();
        assert_eq!(decoded, b"");
    }

    #[test]
    fn bwt_single_byte() {
        let data = b"x";
        let (encoded, idx) = bwt_encode(data).unwrap();
        let decoded = bwt_decode(&encoded, idx).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn bwt_properties() {
        let data = b"the quick brown fox jumps over the lazy dog";
        let (encoded, idx) = bwt_encode(data).unwrap();
        
        // BWT properties: same length, same character frequencies
        assert_eq!(encoded.len(), data.len());
        
        let mut data_sorted = data.to_vec();
        let mut encoded_sorted = encoded.clone();
        data_sorted.sort_unstable();
        encoded_sorted.sort_unstable();
        assert_eq!(data_sorted, encoded_sorted);
        
        let decoded = bwt_decode(&encoded, idx).unwrap();
        assert_eq!(decoded, data);
    }
}
