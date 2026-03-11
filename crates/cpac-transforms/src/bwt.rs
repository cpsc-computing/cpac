// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Burrows-Wheeler Transform (BWT) using suffix array construction.

use cpac_types::{CpacError, CpacResult};

/// Maximum input size for BWT encode (64 MiB).  SA-IS is O(n) time and
/// O(n) space so this is a memory guard, not an algorithmic limit.
pub const BWT_MAX_SIZE: usize = 64 << 20;

/// Burrows-Wheeler Transform forward using SA-IS (O(n) time, O(n) space).
///
/// Returns (`transformed_data`, `original_index`).
pub fn bwt_encode(data: &[u8]) -> CpacResult<(Vec<u8>, usize)> {
    if data.is_empty() {
        return Ok((Vec::new(), 0));
    }
    let n = data.len();
    if n > BWT_MAX_SIZE {
        return Err(CpacError::Transform(format!(
            "bwt: input size {n} exceeds limit {BWT_MAX_SIZE}"
        )));
    }

    // Build suffix array via SA-IS (Nong/Zhang/Chan 2009)
    let sa = sa_is(data);

    // Find original position (suffix starting at index 0)
    let original_idx = sa.iter().position(|&i| i == 0).unwrap_or(0);

    // Build BWT output (last column of rotation matrix)
    let mut output = Vec::with_capacity(n);
    for &idx in &sa {
        let prev = if idx == 0 { n - 1 } else { idx - 1 };
        output.push(data[prev]);
    }

    Ok((output, original_idx))
}

// ---------------------------------------------------------------------------
// SA-IS: Suffix Array by Induced Sorting (Nong, Zhang, Chan 2009)
// ---------------------------------------------------------------------------

/// Classify suffixes as S-type (smaller) or L-type (larger).
/// Returns a bitvec where `true` = S-type.
fn classify_sl(text: &[usize], n: usize) -> Vec<bool> {
    let mut types = vec![false; n];
    if n == 0 {
        return types;
    }
    types[n - 1] = true; // sentinel is S-type
    for i in (0..n.saturating_sub(1)).rev() {
        types[i] = if text[i] < text[i + 1] {
            true
        } else if text[i] > text[i + 1] {
            false
        } else {
            types[i + 1]
        };
    }
    types
}

/// Check if position `i` is a Left-Most S-type (LMS) character.
#[inline]
fn is_lms(types: &[bool], i: usize) -> bool {
    i > 0 && types[i] && !types[i - 1]
}

/// Get bucket heads or tails for each character.
fn get_buckets(text: &[usize], n: usize, alpha_size: usize, end: bool) -> Vec<usize> {
    let mut counts = vec![0usize; alpha_size];
    for &c in &text[..n] {
        counts[c] += 1;
    }
    let mut buckets = vec![0usize; alpha_size];
    let mut sum = 0;
    for i in 0..alpha_size {
        sum += counts[i];
        // Empty buckets (counts[i] == 0) use wrapping_sub to avoid underflow;
        // their indices are never accessed during sorting.
        buckets[i] = if end {
            sum.wrapping_sub(1)
        } else {
            sum - counts[i]
        };
    }
    buckets
}

/// Core SA-IS on integer alphabet `[0, alpha_size)`.
fn sa_is_int(text: &[usize], n: usize, alpha_size: usize) -> Vec<usize> {
    const EMPTY: usize = usize::MAX;
    let types = classify_sl(text, n);
    let mut sa = vec![EMPTY; n];

    // Step 1: place LMS suffixes into their bucket tails
    let mut tails = get_buckets(text, n, alpha_size, true);
    for i in (0..n).rev() {
        if is_lms(&types, i) {
            sa[tails[text[i]]] = i;
            tails[text[i]] = tails[text[i]].wrapping_sub(1);
        }
    }

    // Step 2: induce L-type suffixes from bucket heads
    let mut heads = get_buckets(text, n, alpha_size, false);
    for i in 0..n {
        let j = sa[i];
        if j != EMPTY && j > 0 && !types[j - 1] {
            let c = text[j - 1];
            sa[heads[c]] = j - 1;
            heads[c] += 1;
        }
    }

    // Step 3: induce S-type suffixes from bucket tails
    tails = get_buckets(text, n, alpha_size, true);
    for i in (0..n).rev() {
        let j = sa[i];
        if j != EMPTY && j > 0 && types[j - 1] {
            let c = text[j - 1];
            sa[tails[c]] = j - 1;
            tails[c] = tails[c].wrapping_sub(1);
        }
    }

    // Step 4: compact LMS suffixes, check if all unique
    let mut lms_positions: Vec<usize> = Vec::new();
    for i in 0..n {
        if is_lms(&types, i) {
            lms_positions.push(i);
        }
    }
    let lms_count = lms_positions.len();
    if lms_count <= 1 {
        return sa;
    }

    // Name LMS substrings
    let mut names = vec![EMPTY; n];
    let mut current_name = 0usize;
    let mut prev_lms = EMPTY;
    for i in 0..n {
        if sa[i] == EMPTY || !is_lms(&types, sa[i]) {
            continue;
        }
        if prev_lms != EMPTY {
            // Compare LMS substrings
            let mut diff = false;
            for d in 0..n {
                let a = sa[i] + d;
                let b = prev_lms + d;
                if a >= n || b >= n || text[a] != text[b] || types[a] != types[b] {
                    diff = true;
                    break;
                }
                // End of LMS substring (found next LMS or sentinel)
                if d > 0 && (is_lms(&types, a) || is_lms(&types, b)) {
                    break;
                }
            }
            if diff {
                current_name += 1;
            }
        } else {
            current_name += 1; // first LMS always gets name 1
        }
        names[sa[i]] = current_name - 1;
        prev_lms = sa[i];
    }

    // Build reduced string from LMS names (in original order)
    let reduced: Vec<usize> = lms_positions.iter().map(|&p| names[p]).collect();
    let unique_names = current_name;

    // Recurse if names are not all unique
    let sorted_lms_indices = if unique_names < lms_count {
        sa_is_int(&reduced, lms_count, unique_names)
    } else {
        // All unique — inverse is trivial
        let mut inv = vec![0usize; lms_count];
        for (i, &name) in reduced.iter().enumerate() {
            inv[name] = i;
        }
        inv
    };

    // Step 5: place sorted LMS suffixes and re-induce
    sa.fill(EMPTY);
    tails = get_buckets(text, n, alpha_size, true);
    for i in (0..lms_count).rev() {
        let pos = lms_positions[sorted_lms_indices[i]];
        sa[tails[text[pos]]] = pos;
        tails[text[pos]] = tails[text[pos]].wrapping_sub(1);
    }

    // Re-induce L-type
    heads = get_buckets(text, n, alpha_size, false);
    for i in 0..n {
        let j = sa[i];
        if j != EMPTY && j > 0 && !types[j - 1] {
            let c = text[j - 1];
            sa[heads[c]] = j - 1;
            heads[c] += 1;
        }
    }

    // Re-induce S-type
    tails = get_buckets(text, n, alpha_size, true);
    for i in (0..n).rev() {
        let j = sa[i];
        if j != EMPTY && j > 0 && types[j - 1] {
            let c = text[j - 1];
            sa[tails[c]] = j - 1;
            tails[c] = tails[c].wrapping_sub(1);
        }
    }

    sa
}

/// Build suffix array for a byte string using SA-IS.
///
/// Returns a `Vec<usize>` of length `data.len()` where `sa[i]` is the
/// starting position of the i-th lexicographically smallest **cyclic rotation**.
///
/// We double the input (`data ++ data ++ sentinel`) so that suffixes starting
/// in `[0, n)` compare identically to the corresponding cyclic rotations.
/// A plain suffix SA (single copy + sentinel) would mis-order positions whose
/// suffixes are identical up to the end-of-data boundary; for highly periodic
/// data this corrupts the BWT.
fn sa_is(data: &[u8]) -> Vec<usize> {
    let n = data.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![0];
    }
    // Double the data and append sentinel.
    // Suffix of `data++data++sentinel` starting at position i (< n) has
    // length 2n+1-i ≥ n+1, so the first n characters match the cyclic
    // rotation of `data` starting at i.  Ties are broken by the second
    // copy + sentinel, giving a total order consistent with cyclic BWT.
    let mut text: Vec<usize> = Vec::with_capacity(2 * n + 1);
    for &b in data.iter().chain(data.iter()) {
        text.push(b as usize + 1);
    }
    text.push(0); // sentinel
    let full_sa = sa_is_int(&text, 2 * n + 1, 258);
    // Keep only positions in [0, n) — one per cyclic rotation.
    full_sa.into_iter().filter(|&i| i < n).collect()
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

    for (i, &byte) in data.iter().enumerate().take(n) {
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
    fn bwt_roundtrip_4mb_text() {
        let sentence = b"The quick brown fox jumps over the lazy dog. ";
        let data: Vec<u8> = sentence.iter().copied().cycle().take(4 << 20).collect();
        let (encoded, idx) = bwt_encode(&data).unwrap();
        assert_eq!(encoded.len(), data.len());
        let decoded = bwt_decode(&encoded, idx).unwrap();
        assert_eq!(decoded.len(), data.len());
        if decoded != data {
            let pos = decoded
                .iter()
                .zip(data.iter())
                .position(|(a, b)| a != b)
                .unwrap_or(0);
            panic!(
                "BWT roundtrip 4MB mismatch at byte {}: got {} expected {} (idx={})",
                pos, decoded[pos], data[pos], idx
            );
        }
    }

    #[test]
    fn bwt_sa_cyclic_validity_4mb() {
        // Verify the cyclic-rotation SA is correct at 4 MiB by
        // spot-checking adjacent SA entries.
        let sentence = b"The quick brown fox jumps over the lazy dog. ";
        let data: Vec<u8> = sentence.iter().copied().cycle().take(4 << 20).collect();
        let sa = sa_is(&data);
        let n = data.len();
        assert_eq!(sa.len(), n);

        // Helper: compare cyclic rotations
        let rot_lt = |a: usize, b: usize| -> bool {
            for k in 0..n {
                let ca = data[(a + k) % n];
                let cb = data[(b + k) % n];
                if ca < cb {
                    return true;
                }
                if ca > cb {
                    return false;
                }
            }
            false // equal
        };

        // Check first 200 adjacent pairs
        for i in 0..199.min(n - 1) {
            assert!(
                rot_lt(sa[i], sa[i + 1]),
                "cyclic SA invalid at {i},{}: rot({}) >= rot({})",
                i + 1,
                sa[i],
                sa[i + 1]
            );
        }
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
