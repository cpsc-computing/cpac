// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Structural Summary Record (SSR) analysis.
//!
//! Computes entropy, ASCII ratio, domain hints and determines the
//! compression track (Track 1 domain-aware vs Track 2 generic).

#![allow(clippy::cast_precision_loss, clippy::naive_bytecount)]

use cpac_types::{DomainHint, Track};

/// Default viability threshold for Track 1.
pub const DEFAULT_VIABILITY_THRESHOLD: f64 = 0.3;

/// Result of SSR analysis.
#[derive(Clone, Debug)]
pub struct SSRResult {
    /// Shannon entropy estimate (bits per byte, 0.0–8.0).
    pub entropy_estimate: f64,
    /// Fraction of bytes that are printable ASCII (0.0–1.0).
    pub ascii_ratio: f64,
    /// Data size in bytes.
    pub data_size: usize,
    /// Viability score (higher = more compressible via Track 1).
    pub viability_score: f64,
    /// Selected track.
    pub track: Track,
    /// Domain hint (if detected).
    pub domain_hint: Option<DomainHint>,
}

/// Analyze data and produce an SSR result.
///
/// This function computes the Shannon entropy, ASCII ratio, and domain hints
/// to determine the optimal compression track.
///
/// # Examples
///
/// ```
/// use cpac_ssr::analyze;
/// use cpac_types::Track;
///
/// // Low-entropy ASCII data selects Track 1
/// let result = analyze(b"hello world hello world");
/// assert_eq!(result.track, Track::Track1);
/// assert!(result.ascii_ratio > 0.9);
///
/// // High-entropy data selects Track 2
/// let random: Vec<u8> = (0..256).map(|i| i as u8).cycle().take(1024).collect();
/// let result = analyze(&random);
/// assert_eq!(result.track, Track::Track2);
/// ```
#[must_use] 
pub fn analyze(data: &[u8]) -> SSRResult {
    let data_size = data.len();

    if data_size == 0 {
        return SSRResult {
            entropy_estimate: 0.0,
            ascii_ratio: 0.0,
            data_size: 0,
            viability_score: 0.0,
            track: Track::Track2,
            domain_hint: None,
        };
    }

    let entropy_estimate = shannon_entropy(data);
    let ascii_ratio = compute_ascii_ratio(data);
    let domain_hint = detect_domain(data);

    // Viability: higher for low entropy + high ascii ratio + domain detection
    let domain_bonus = if domain_hint.is_some() { 0.2 } else { 0.0 };
    let viability_score = (1.0 - entropy_estimate / 8.0) * 0.4 + ascii_ratio * 0.4 + domain_bonus;

    let track = if viability_score >= DEFAULT_VIABILITY_THRESHOLD {
        Track::Track1
    } else {
        Track::Track2
    };

    SSRResult {
        entropy_estimate,
        ascii_ratio,
        data_size,
        viability_score,
        track,
        domain_hint,
    }
}

/// Compute Shannon entropy in bits per byte.
fn shannon_entropy(data: &[u8]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }

    let mut counts = [0u64; 256];
    for &b in data {
        counts[b as usize] += 1;
    }

    let len = data.len() as f64;
    let mut entropy = 0.0;

    for &count in &counts {
        if count > 0 {
            let p = count as f64 / len;
            entropy -= p * p.log2();
        }
    }

    entropy
}

/// Fraction of bytes that are printable ASCII (0x20–0x7E) or common whitespace.
fn compute_ascii_ratio(data: &[u8]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }

    let ascii_count = data
        .iter()
        .filter(|&&b| b.is_ascii_graphic() || b == b' ' || b == b'\n' || b == b'\r' || b == b'\t')
        .count();

    ascii_count as f64 / data.len() as f64
}

/// Simple domain detection from content heuristics.
fn detect_domain(data: &[u8]) -> Option<DomainHint> {
    if data.is_empty() {
        return None;
    }

    // Check first non-whitespace bytes
    let trimmed = data
        .iter()
        .skip_while(|b| b.is_ascii_whitespace())
        .copied()
        .take(64)
        .collect::<Vec<_>>();

    if trimmed.is_empty() {
        return None;
    }

    // JSON detection
    if trimmed[0] == b'{' || trimmed[0] == b'[' {
        return Some(DomainHint::Json);
    }

    // XML detection
    if trimmed.starts_with(b"<?xml") || trimmed.starts_with(b"<") {
        return Some(DomainHint::Xml);
    }

    // CSV detection: check first line for commas or tabs
    if let Some(first_line_end) = data.iter().position(|&b| b == b'\n') {
        let first_line = &data[..first_line_end];
        let comma_count = first_line.iter().filter(|&&b| b == b',').count();
        if comma_count >= 2 {
            return Some(DomainHint::Csv);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_data() {
        let r = analyze(b"");
        assert_eq!(r.data_size, 0);
        assert_eq!(r.entropy_estimate, 0.0);
        assert_eq!(r.track, Track::Track2);
    }

    #[test]
    fn low_entropy_ascii() {
        let data = b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let r = analyze(data);
        assert!(r.entropy_estimate < 0.1);
        assert!(r.ascii_ratio > 0.99);
        assert_eq!(r.track, Track::Track1);
    }

    #[test]
    fn high_entropy() {
        // Pseudo-random bytes
        let data: Vec<u8> = (0..256).map(|i| i as u8).cycle().take(1024).collect();
        let r = analyze(&data);
        assert!(r.entropy_estimate > 7.5);
    }

    #[test]
    fn detect_json() {
        let r = analyze(b"{\"key\": \"value\"}");
        assert_eq!(r.domain_hint, Some(DomainHint::Json));
    }

    #[test]
    fn detect_csv() {
        let r = analyze(b"name,age,city\nAlice,30,NYC\n");
        assert_eq!(r.domain_hint, Some(DomainHint::Csv));
    }
}
