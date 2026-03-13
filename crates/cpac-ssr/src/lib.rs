// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Structural Summary Record (SSR) analysis.
//!
//! Computes entropy, ASCII ratio, domain hints and determines the
//! compression track (Track 1 domain-aware vs Track 2 generic).

#![allow(
    clippy::cast_precision_loss,
    clippy::naive_bytecount,
    clippy::inline_always
)]

mod simd;

use cpac_types::{DomainHint, ImageFormat, Track};

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

/// P9: Convert from the lightweight `CachedSsr` in cpac-types.
impl From<cpac_types::CachedSsr> for SSRResult {
    fn from(c: cpac_types::CachedSsr) -> Self {
        Self {
            entropy_estimate: c.entropy_estimate,
            ascii_ratio: c.ascii_ratio,
            data_size: c.data_size,
            viability_score: c.viability_score,
            track: c.track,
            domain_hint: c.domain_hint,
        }
    }
}

/// P9: Convert to the lightweight `CachedSsr` in cpac-types.
impl From<&SSRResult> for cpac_types::CachedSsr {
    fn from(s: &SSRResult) -> Self {
        Self {
            entropy_estimate: s.entropy_estimate,
            ascii_ratio: s.ascii_ratio,
            data_size: s.data_size,
            viability_score: s.viability_score,
            track: s.track,
            domain_hint: s.domain_hint.clone(),
        }
    }
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

    // Viability: higher for low entropy + high ascii ratio + domain detection.
    // Binary domain hint gets a *penalty* to force Track 2 (BWT/MSN hurt on binary).
    let domain_bonus = match &domain_hint {
        Some(DomainHint::Binary) => -0.5,
        Some(_) => 0.2,
        None => 0.0,
    };
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
///
/// Uses 4× unrolled byte histogram for ILP on modern CPUs.
fn shannon_entropy(data: &[u8]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }

    let counts = simd::byte_histogram(data);

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
///
/// Uses SIMD-accelerated scanning on x86_64 (AVX2/SSE2) and aarch64 (NEON)
/// with automatic runtime dispatch.
fn compute_ascii_ratio(data: &[u8]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }

    let ascii_count = simd::count_ascii_bytes(data);
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

    // --- Lossless image detection (before generic binary) ---
    // PNG: 89 50 4E 47 0D 0A 1A 0A
    if data.len() >= 8 && data[..8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A] {
        return Some(DomainHint::Image(ImageFormat::Png));
    }
    // BMP: 42 4D
    if data.len() >= 2 && data[..2] == [0x42, 0x4D] {
        return Some(DomainHint::Image(ImageFormat::Bmp));
    }
    // TIFF: 49 49 2A 00 (little-endian) or 4D 4D 00 2A (big-endian)
    if data.len() >= 4
        && (data[..4] == [0x49, 0x49, 0x2A, 0x00] || data[..4] == [0x4D, 0x4D, 0x00, 0x2A])
    {
        return Some(DomainHint::Image(ImageFormat::Tiff));
    }
    // WebP: RIFF....WEBP + VP8L (lossless)
    if data.len() >= 16
        && data[..4] == [0x52, 0x49, 0x46, 0x46]
        && data[8..12] == [0x57, 0x45, 0x42, 0x50]
        && data[12..16] == [0x56, 0x50, 0x38, 0x4C]
    {
        return Some(DomainHint::Image(ImageFormat::WebPLossless));
    }

    // --- Binary format detection (before text formats) ---
    // OLE2 Compound Document (XLS, DOC, PPT): magic 0xD0CF11E0A1B11AE1
    if data.len() >= 8 && data[..8] == [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1] {
        return Some(DomainHint::Binary);
    }
    // CCITT Group 3/4 fax (ITU T.4/T.6): high-entropy binary with specific byte patterns.
    // Detect by: low ASCII ratio + specific EOL patterns (0x00 0x01 or 0x80 sequences).
    if data.len() >= 64 {
        let ascii = compute_ascii_ratio(data);
        if ascii < 0.15 {
            // Check for fax-like pattern: high byte diversity with 0x00 sequences
            let zero_pairs = data
                .windows(2)
                .take(512)
                .filter(|w| w[0] == 0 && w[1] < 0x04)
                .count();
            if zero_pairs > 10 {
                return Some(DomainHint::Binary);
            }
        }
    }
    // ELF binary
    if data.len() >= 4 && data[..4] == [0x7F, b'E', b'L', b'F'] {
        return Some(DomainHint::Binary);
    }
    // PE/COFF (Windows executable)
    if data.len() >= 2 && data[..2] == [b'M', b'Z'] {
        return Some(DomainHint::Binary);
    }

    // JSON detection: must check before log (some logs start with '[')
    if trimmed[0] == b'{' {
        return Some(DomainHint::Json);
    }

    // XML detection
    if trimmed.starts_with(b"<?xml") || trimmed.starts_with(b"<!") {
        return Some(DomainHint::Xml);
    }

    // CSV detection: check first line for commas
    if let Some(first_line_end) = data.iter().position(|&b| b == b'\n') {
        let first_line = &data[..first_line_end];
        let comma_count = first_line.iter().filter(|&&b| b == b',').count();
        if comma_count >= 2 {
            return Some(DomainHint::Csv);
        }
    }

    // Log detection: BSD syslog, Apache error log, or structured log with levels
    if detect_log(data) {
        return Some(DomainHint::Log);
    }

    // JSON array or XML element — checked after log to avoid misclassifying
    // Apache error logs ('[Mon ...') as JSON arrays.
    if trimmed[0] == b'[' {
        return Some(DomainHint::Json);
    }
    if trimmed[0] == b'<' {
        return Some(DomainHint::Xml);
    }

    None
}

const BSD_MONTHS_SSR: &[&str] = &[
    "Jan ", "Feb ", "Mar ", "Apr ", "May ", "Jun ", "Jul ", "Aug ", "Sep ", "Oct ", "Nov ", "Dec ",
];
const WEEKDAYS_SSR: &[&str] = &[
    "[Mon ", "[Tue ", "[Wed ", "[Thu ", "[Fri ", "[Sat ", "[Sun ",
];
const LOG_LEVELS_SSR: &[&str] = &[" INFO ", " ERROR ", " WARNING ", " DEBUG ", " CRITICAL "];

/// Content-based log format detection.
///
/// Recognises:
/// - BSD syslog:    `Mon DD HH:MM:SS hostname app[pid]: msg`
/// - Apache error:  `[Day Mon DD HH:MM:SS YYYY] [level] msg`
/// - Apache/NCSA access log: `IP - - [timestamp] "METHOD /path HTTP/x.x" status bytes`
/// - Structured:    `ISO-date` + `log-level` keyword (`OpenStack`, `Nova`, etc.)
fn detect_log(data: &[u8]) -> bool {
    // Sample at most 4 KB so this stays O(1) for large files.
    let sample = &data[..data.len().min(4096)];
    let text = std::str::from_utf8(sample).unwrap_or("");
    if text.is_empty() {
        return false;
    }

    let mut bsd = 0usize;
    let mut apache_err = 0usize;
    let mut structured = 0usize;
    let mut access_log = 0usize;

    for line in text.lines().take(10) {
        if BSD_MONTHS_SSR.iter().any(|m| line.starts_with(m)) {
            bsd += 1;
        }
        if WEEKDAYS_SSR.iter().any(|d| line.starts_with(d)) {
            apache_err += 1;
        }
        if line.contains('-')
            && line.contains(':')
            && LOG_LEVELS_SSR.iter().any(|lvl| line.contains(lvl))
        {
            structured += 1;
        }
        // Apache/NCSA access log: `IP - - [timestamp] "METHOD /path HTTP/x.x"`
        // Identified by `] "` (timestamp close + quoted request) + HTTP/ version.
        if line.contains("] \"") && line.contains("HTTP/") {
            access_log += 1;
        }
    }

    bsd > 5 || apache_err > 5 || structured > 5 || access_log > 5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_ole2_binary() {
        // OLE2 magic + padding
        let mut data = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
        data.extend(vec![0u8; 256]);
        let r = analyze(&data);
        assert_eq!(r.domain_hint, Some(DomainHint::Binary));
        assert_eq!(r.track, Track::Track2, "OLE2 binary should go to Track 2");
    }

    #[test]
    fn detect_elf_binary() {
        let mut data = vec![0x7F, b'E', b'L', b'F'];
        data.extend(vec![0u8; 256]);
        let r = analyze(&data);
        assert_eq!(r.domain_hint, Some(DomainHint::Binary));
        assert_eq!(r.track, Track::Track2);
    }

    #[test]
    fn detect_pe_binary() {
        let mut data = vec![b'M', b'Z'];
        data.extend(vec![0u8; 256]);
        let r = analyze(&data);
        assert_eq!(r.domain_hint, Some(DomainHint::Binary));
        assert_eq!(r.track, Track::Track2);
    }

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

    #[test]
    fn detect_bsd_syslog() {
        // Minimal 10-line BSD syslog sample (>5 lines must match)
        let data = b"Jun 14 15:16:01 host sshd[1]: msg\n\
Jun 14 15:16:02 host sshd[2]: msg\n\
Jun 14 15:16:03 host sshd[3]: msg\n\
Jun 14 15:16:04 host sshd[4]: msg\n\
Jun 14 15:16:05 host sshd[5]: msg\n\
Jun 14 15:16:06 host sshd[6]: msg\n\
Jun 14 15:16:07 host sshd[7]: msg\n";
        let r = analyze(data);
        assert_eq!(r.domain_hint, Some(DomainHint::Log));
    }

    #[test]
    fn detect_apache_error_log() {
        let data = b"[Sun Dec 04 04:47:44 2005] [notice] msg1\n\
[Sun Dec 04 04:47:45 2005] [error] msg2\n\
[Sun Dec 04 04:47:46 2005] [notice] msg3\n\
[Sun Dec 04 04:47:47 2005] [notice] msg4\n\
[Sun Dec 04 04:47:48 2005] [error] msg5\n\
[Sun Dec 04 04:47:49 2005] [notice] msg6\n\
[Mon Dec 05 04:47:50 2005] [notice] msg7\n";
        let r = analyze(data);
        assert_eq!(r.domain_hint, Some(DomainHint::Log));
    }

    #[test]
    fn detect_structured_log() {
        let data = b"svc.log 2017-05-16 00:00:00.001 123 INFO ns.api: request\n\
svc.log 2017-05-16 00:00:01.002 123 INFO ns.api: response\n\
svc.log 2017-05-16 00:00:02.003 123 ERROR ns.api: fail\n\
svc.log 2017-05-16 00:00:03.004 123 INFO ns.api: request\n\
svc.log 2017-05-16 00:00:04.005 123 INFO ns.api: response\n\
svc.log 2017-05-16 00:00:05.006 123 INFO ns.api: ok\n\
svc.log 2017-05-16 00:00:06.007 123 INFO ns.api: ok\n";
        let r = analyze(data);
        assert_eq!(r.domain_hint, Some(DomainHint::Log));
    }

    #[test]
    fn json_not_misdetected_as_log() {
        // JSON arrays start with '['; must not be classified as log
        let r = analyze(b"[{\"a\": 1}, {\"b\": 2}]");
        assert_eq!(r.domain_hint, Some(DomainHint::Json));
    }

    #[test]
    fn detect_access_log() {
        // Classic Combined Log Format (NASA/Apache access logs)
        let data = b"199.72.81.55 - - [01/Jul/1995:00:00:01 -0400] \"GET /history/apollo/ HTTP/1.0\" 200 6245\n\
205.212.115.106 - - [01/Jul/1995:00:00:02 -0400] \"GET /shuttle/countdown/ HTTP/1.0\" 200 3985\n\
129.94.144.152 - - [01/Jul/1995:00:00:03 -0400] \"GET / HTTP/1.0\" 200 7074\n\
199.166.62.154 - - [01/Jul/1995:00:00:04 -0400] \"GET /images/NASA-logosmall.gif HTTP/1.0\" 200 786\n\
 unicom016.unicom.net - - [01/Jul/1995:00:00:05 -0400] \"GET /shuttle/countdown/ HTTP/1.0\" 200 3985\n\
199.72.81.55 - - [01/Jul/1995:00:00:06 -0400] \"GET /images/NASA-logosmall.gif HTTP/1.0\" 200 786\n\
205.212.115.106 - - [01/Jul/1995:00:00:07 -0400] \"GET /images/KSC-logosmall.gif HTTP/1.0\" 200 1204\n";
        let r = analyze(data);
        assert_eq!(r.domain_hint, Some(DomainHint::Log));
    }
}
