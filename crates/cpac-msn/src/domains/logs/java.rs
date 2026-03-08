// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Java / Log4j / HDFS log domain handler.
//!
//! Handles Java-style log lines where a fully-qualified class name (FQCN) or
//! dotted module token appears immediately before the `: ` message separator.
//!
//! Covered formats:
//! - Hadoop / MapReduce: `YYYY-MM-DD HH:MM:SS,ms LEVEL [thread] pkg.Class: msg`
//! - Spark:              `YY/MM/DD HH:MM:SS LEVEL pkg.Class: msg`
//! - HDFS compact:       `YYMMDD HHMMSS tid LEVEL dfs.Module$Inner: msg`
//! - Generic Log4j:      any line with ` LEVEL ` keyword and a dotted token before `:`
//!
//! Extraction: repeated FQCN tokens (≥ 10 chars, freq ≥ 3) are replaced with
//! `@C{n}` placeholders, and repeated thread names (the `[name]` bracketed token
//! immediately after the log level) are replaced with `[@T{n}]` placeholders,
//! reducing residual size by 10–50 bytes per token per line.

use crate::domain::{Domain, DomainInfo, ExtractionResult};
use cpac_types::{CpacError, CpacResult};
use std::collections::HashMap;

/// Minimum dotted-token length to consider for extraction.
const MIN_TOKEN_LEN: usize = 10;
/// Minimum thread-name length to consider for extraction.
/// Filters out short names like "main" (4) and "Thread-1" (8).
const MIN_THREAD_LEN: usize = 8;
/// Minimum occurrences for a token to be worth storing in metadata.
const MIN_FREQUENCY: usize = 3;
/// Minimum block size; below this the metadata overhead exceeds savings.
const MIN_USEFUL_SIZE: usize = 32_768; // 32 KB
/// Maximum number of unique tokens to extract.  Caps metadata size.
const MAX_TOKENS: usize = 32;
/// Dynamic frequency scale factor: require a token to appear in at least
/// this fraction of lines, ensuring large diverse files are not over-extracted.
const DYN_FREQ_RATIO: f64 = 0.005;
/// Minimum raw byte savings (post-truncation) before we commit to extraction.
const MIN_SAVINGS_BYTES: usize = 256;
/// Tokens appearing on more than this fraction of lines are excluded: they are
/// already handled efficiently by zstd back-references and MSN extraction of
/// such high-frequency tokens hurts the residual's overall compression ratio.
const MAX_FREQ_FRACTION: f64 = 0.25;

/// Minimum length for a block ID token to be worth extracting.
/// `blk_12345` is 9 chars; real HDFS IDs are 10-24 chars.
const MIN_BLK_LEN: usize = 9;

pub struct JavaLogDomain;

impl Domain for JavaLogDomain {
    fn info(&self) -> DomainInfo {
        DomainInfo {
            id: "log.java",
            name: "Java/Log4j Log",
            extensions: &[".log"],
            mime_types: &["text/plain"],
            magic_bytes: &[],
        }
    }

    fn detect(&self, data: &[u8], _filename: Option<&str>) -> f64 {
        let text = std::str::from_utf8(data).unwrap_or("");

        let java_count = text
            .lines()
            .take(10)
            .filter(|line| is_java_log_line(line))
            .count();

        if java_count > 6 {
            if data.len() >= MIN_USEFUL_SIZE {
                0.82
            } else {
                0.35 // below default 0.5 min_confidence — skip on tiny blocks
            }
        } else {
            0.0
        }
    }

    fn extract(&self, data: &[u8]) -> CpacResult<ExtractionResult> {
        let text = std::str::from_utf8(data)
            .map_err(|e| CpacError::CompressFailed(format!("Java log decode: {e}")))?;
        extract_java(text)
    }

    fn extract_with_fields(
        &self,
        data: &[u8],
        fields: &HashMap<String, serde_json::Value>,
    ) -> CpacResult<ExtractionResult> {
        let text = std::str::from_utf8(data)
            .map_err(|e| CpacError::CompressFailed(format!("Java log decode: {e}")))?;

        let classes = get_str_vec(fields, "classes").unwrap_or_default();
        let threads = get_str_vec(fields, "threads").unwrap_or_default();
        let blk_ids = get_str_vec(fields, "blk_ids").unwrap_or_default();
        let ts_prefix: Option<&str> = fields.get("ts_prefix").and_then(|v| v.as_str());

        if classes.is_empty() && threads.is_empty() && ts_prefix.is_none() && blk_ids.is_empty() {
            return extract_java(text);
        }

        // Build replacement maps from detection-phase lists.
        // Sort longest first to avoid partial matches.
        let mut sorted_classes: Vec<(usize, &str)> = classes
            .iter()
            .enumerate()
            .map(|(i, s)| (i, s.as_str()))
            .collect();
        sorted_classes.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

        let mut sorted_threads: Vec<(usize, &str)> = threads
            .iter()
            .enumerate()
            .map(|(i, s)| (i, s.as_str()))
            .collect();
        sorted_threads.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

        let mut compacted = text.to_string();
        // Apply ts_prefix replacement first.
        if let Some(pfx) = ts_prefix {
            let mut result = String::with_capacity(compacted.len());
            for line in compacted.lines() {
                if let Some(rest) = line.strip_prefix(pfx) {
                    result.push_str("@TS");
                    result.push_str(rest);
                } else {
                    result.push_str(line);
                }
                result.push('\n');
            }
            if !compacted.ends_with('\n') && result.ends_with('\n') {
                result.pop();
            }
            compacted = result;
        }
        for (idx, class) in &sorted_classes {
            compacted = compacted.replace(class, &format!("@C{idx}"));
        }
        for (idx, thread) in &sorted_threads {
            let from = format!("[{thread}]");
            let to = format!("[@T{idx}]");
            compacted = compacted.replace(&from, &to);
        }
        // Apply block ID substitutions (longest first to prevent prefix matches).
        let mut sorted_blks: Vec<(usize, &str)> = blk_ids
            .iter()
            .enumerate()
            .map(|(i, s)| (i, s.as_str()))
            .collect();
        sorted_blks.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
        for (idx, blk) in &sorted_blks {
            compacted = compacted.replace(blk, &format!("@B{idx}"));
        }

        Ok(ExtractionResult {
            fields: fields.clone(),
            residual: compacted.into_bytes(),
            metadata: HashMap::new(),
            domain_id: "log.java".to_string(),
        })
    }

    fn reconstruct(&self, result: &ExtractionResult) -> CpacResult<Vec<u8>> {
        let classes = get_str_vec(&result.fields, "classes")?;
        let threads = get_str_vec(&result.fields, "threads")?;
        let blk_ids = get_str_vec(&result.fields, "blk_ids")?;
        let ts_prefix: Option<String> = result
            .fields
            .get("ts_prefix")
            .and_then(|v| v.as_str().map(String::from));

        let mut reconstructed = std::str::from_utf8(&result.residual)
            .map_err(|e| CpacError::DecompressFailed(format!("UTF-8 decode: {e}")))?
            .to_string();

        // Expand block ID placeholders first (@B{idx} → blk_NNNN).
        // Reverse index order so @B10 expands before @B1.
        for (idx, blk) in blk_ids.iter().enumerate().rev() {
            reconstructed = reconstructed.replace(&format!("@B{idx}"), blk);
        }

        // Expand thread placeholders ([@T{idx}] → [thread_name]).
        // Reverse index order so [@T10] expands before [@T1].
        for (idx, thread) in threads.iter().enumerate().rev() {
            reconstructed = reconstructed.replace(&format!("[@T{idx}]"), &format!("[{thread}]"));
        }

        // Expand class placeholders (@C{idx} → class_fqcn).
        for (idx, class) in classes.iter().enumerate().rev() {
            reconstructed = reconstructed.replace(&format!("@C{idx}"), class);
        }

        // Expand timestamp prefix (@TS → shared prefix).
        // Must be last: class tokens may contain digits that look like dates.
        if let Some(pfx) = ts_prefix {
            let mut result_str = String::with_capacity(reconstructed.len());
            for line in reconstructed.lines() {
                if let Some(rest) = line.strip_prefix("@TS") {
                    result_str.push_str(&pfx);
                    result_str.push_str(rest);
                } else {
                    result_str.push_str(line);
                }
                result_str.push('\n');
            }
            // Preserve original trailing-newline semantics.
            if !reconstructed.ends_with('\n') && result_str.ends_with('\n') {
                result_str.pop();
            }
            reconstructed = result_str;
        }

        Ok(reconstructed.into_bytes())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns true when `line` matches a Java-style log line: contains a known
/// level keyword AND has a dotted class token immediately before `": "`.
fn is_java_log_line(line: &str) -> bool {
    if line.len() < 20 {
        return false;
    }
    let has_level = line.contains(" INFO ")
        || line.contains(" WARN ")
        || line.contains(" ERROR ")
        || line.contains(" DEBUG ")
        || line.contains(" TRACE ")
        || line.contains(" FATAL ");
    has_level && extract_class_token(line).is_some()
}

/// Scan `line` left-to-right for the first occurrence of `": "` (or `":<EOL>"`)
/// where the word immediately preceding `:` is a dotted token of at least
/// `MIN_TOKEN_LEN` characters.  Returns a slice into `line`.
fn extract_class_token(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    for i in 1..bytes.len() {
        if bytes[i] != b':' {
            continue;
        }
        let after = if i + 1 < bytes.len() {
            bytes[i + 1]
        } else {
            b'\n'
        };
        // Only match ": " (colon + space) or colon at end-of-line.
        if after != b' ' && after != b'\n' && after != b'\r' && after != 0 {
            continue;
        }
        // Find word start: scan backwards for space or ']'.
        let word_start = line[..i].rfind([' ', ']']).map_or(0, |p| p + 1);
        let token = &line[word_start..i];
        // Valid class token: contains '.', long enough, no embedded ':' or '/'.
        if token.len() >= MIN_TOKEN_LEN
            && token.contains('.')
            && !token.contains(':')
            && !token.contains('/')
        {
            return Some(token);
        }
        // Not valid — keep scanning (message body may have other ": " pairs).
    }
    None
}

/// Core extraction: build FQCN + thread-name + block-ID frequency tables and compact the text.
fn extract_java(text: &str) -> CpacResult<ExtractionResult> {
    let mut class_freq: HashMap<String, usize> = HashMap::new();
    let mut thread_freq: HashMap<String, usize> = HashMap::new();
    let mut blk_freq: HashMap<String, usize> = HashMap::new();

    for line in text.lines() {
        if let Some(token) = extract_class_token(line) {
            *class_freq.entry(token.to_string()).or_insert(0) += 1;
        }
        if let Some(thread) = extract_thread_name(line) {
            *thread_freq.entry(thread.to_string()).or_insert(0) += 1;
        }
        // Scan for HDFS block ID tokens: `blk_` followed by optional `-` and digits.
        for blk in extract_blk_ids(line) {
            *blk_freq.entry(blk.to_string()).or_insert(0) += 1;
        }
    }

    // Dynamic minimum frequency: scales with file size so large, diverse files
    // (e.g. Hadoop with many unique classes appearing only a few times each)
    // do not produce metadata that outweighs the compression benefit.
    let line_count = text.lines().count().max(1);
    // Minimum number of timestamped lines required before we extract a ts_prefix.
    // Use the same dyn_min_freq floor so tiny blocks don’t get a spurious prefix.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation
    )]
    let ts_min_lines = ((line_count as f64 * DYN_FREQ_RATIO).round() as usize).max(MIN_FREQUENCY);
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation
    )]
    let dyn_min_freq = ((line_count as f64 * DYN_FREQ_RATIO).round() as usize).max(MIN_FREQUENCY);

    // -- Class token selection --
    // Keep tokens that pass both length and frequency thresholds.
    let mut repeated_classes: Vec<(String, usize)> = class_freq
        .into_iter()
        .filter(|(tok, count)| tok.len() >= MIN_TOKEN_LEN && *count >= dyn_min_freq)
        .collect();
    // Sort longest first so replacements don't clobber sub-tokens.
    repeated_classes.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(b.1.cmp(&a.1)));
    // Cap to MAX_TOKENS to bound metadata size.
    repeated_classes.truncate(MAX_TOKENS);
    // Exclude tokens that appear on > MAX_FREQ_FRACTION of lines: zstd already
    // compresses such high-frequency, consecutive tokens via cheap back-references.
    #[allow(clippy::cast_precision_loss)]
    repeated_classes.retain(|(_, count)| *count as f64 / line_count as f64 <= MAX_FREQ_FRACTION);

    // -- Thread name selection --
    let mut repeated_threads: Vec<(String, usize)> = thread_freq
        .into_iter()
        .filter(|(tok, count)| tok.len() >= MIN_THREAD_LEN && *count >= dyn_min_freq)
        .collect();
    repeated_threads.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(b.1.cmp(&a.1)));
    repeated_threads.truncate(MAX_TOKENS);
    #[allow(clippy::cast_precision_loss)]
    repeated_threads.retain(|(_, count)| *count as f64 / line_count as f64 <= MAX_FREQ_FRACTION);

    // Combined savings gate: if replacing all tokens saves fewer than MIN_SAVINGS_BYTES
    // raw bytes, the metadata overhead exceeds any benefit.
    // For classes: replacement `@C{n}` averages ~4 chars → savings = len - 4.
    // For threads: replacement `[@T{n}]` averages ~6 chars, replaces `[thread]` (len+2)
    //              → savings = (len + 2) - 6 = len - 4.  Same formula.
    // Block IDs (HDFS): `blk_NNNN` tokens in message bodies.
    let mut repeated_blks: Vec<(String, usize)> = blk_freq
        .into_iter()
        .filter(|(tok, count)| tok.len() >= MIN_BLK_LEN && *count >= dyn_min_freq)
        .collect();
    repeated_blks.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(b.1.cmp(&a.1)));
    repeated_blks.truncate(MAX_TOKENS);

    let class_savings: usize = repeated_classes
        .iter()
        .map(|(tok, count)| tok.len().saturating_sub(4) * count)
        .sum();
    let thread_savings: usize = repeated_threads
        .iter()
        .map(|(tok, count)| tok.len().saturating_sub(4) * count)
        .sum();
    let blk_savings: usize = repeated_blks
        .iter()
        .map(|(tok, count)| tok.len().saturating_sub(4) * count)
        .sum();
    // Timestamp prefix: find the longest common prefix of all leading timestamps.
    // Unlike class/thread tokens, a timestamp prefix appears on EVERY timestamped line
    // (freq ≈ 1.0) — we skip MAX_FREQ_FRACTION for it.
    // Savings gate: prefix_len must exceed the replacement token "@TS" (3 chars)
    // by enough margin to cover metadata cost.
    let ts_prefix = extract_timestamp_prefix(text, ts_min_lines);
    #[allow(clippy::cast_precision_loss)]
    let ts_savings = ts_prefix
        .as_ref()
        .map(|p| p.len().saturating_sub(3) * line_count)
        .unwrap_or(0);

    if class_savings + thread_savings + ts_savings + blk_savings < MIN_SAVINGS_BYTES {
        let mut f = HashMap::new();
        f.insert("classes".to_string(), serde_json::Value::Array(vec![]));
        f.insert("threads".to_string(), serde_json::Value::Array(vec![]));
        f.insert("ts_prefix".to_string(), serde_json::Value::Null);
        f.insert("blk_ids".to_string(), serde_json::Value::Array(vec![]));
        return Ok(ExtractionResult {
            fields: f,
            residual: text.as_bytes().to_vec(),
            metadata: HashMap::new(),
            domain_id: "log.java".to_string(),
        });
    }

    // Apply replacements:
    // 1. Timestamp prefix first (always at line start — no ambiguity).
    // 2. Class FQCNs.
    // 3. Thread names (inside brackets).
    let mut compacted = text.to_string();
    if let Some(ref pfx) = ts_prefix {
        // Only replace at line boundaries to avoid mangling message bodies.
        // A simple newline-split replace is safe because the prefix is only
        // expected at the very start of each line.
        let mut result = String::with_capacity(compacted.len());
        for line in compacted.lines() {
            if line.starts_with(pfx.as_str()) {
                result.push_str("@TS");
                result.push_str(&line[pfx.len()..]);
            } else {
                result.push_str(line);
            }
            result.push('\n');
        }
        // Preserve trailing newline behaviour: if original didn’t end with \n, pop the last one.
        if !compacted.ends_with('\n') && result.ends_with('\n') {
            result.pop();
        }
        compacted = result;
    }
    for (idx, (class, _)) in repeated_classes.iter().enumerate() {
        compacted = compacted.replace(class.as_str(), &format!("@C{idx}"));
    }
    for (idx, (thread, _)) in repeated_threads.iter().enumerate() {
        // Replace `[thread_name]` (with surrounding brackets) with `[@T{idx}]` to
        // prevent accidental replacement of the same text in the message body.
        let from = format!("[{thread}]");
        let to = format!("[@T{idx}]");
        compacted = compacted.replace(&from, &to);
    }
    // Replace block IDs (longest first to avoid prefix collisions).
    let mut sorted_blks: Vec<(usize, &str)> = repeated_blks
        .iter()
        .enumerate()
        .map(|(i, (s, _))| (i, s.as_str()))
        .collect();
    sorted_blks.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    for (idx, blk) in &sorted_blks {
        compacted = compacted.replace(blk, &format!("@B{idx}"));
    }

    let mut fields = HashMap::new();
    fields.insert(
        "classes".to_string(),
        serde_json::Value::Array(
            repeated_classes
                .iter()
                .map(|(c, _)| serde_json::Value::String(c.clone()))
                .collect(),
        ),
    );
    fields.insert(
        "threads".to_string(),
        serde_json::Value::Array(
            repeated_threads
                .iter()
                .map(|(t, _)| serde_json::Value::String(t.clone()))
                .collect(),
        ),
    );
    fields.insert(
        "ts_prefix".to_string(),
        match ts_prefix {
            Some(ref p) => serde_json::Value::String(p.clone()),
            None => serde_json::Value::Null,
        },
    );
    fields.insert(
        "blk_ids".to_string(),
        serde_json::Value::Array(
            repeated_blks
                .iter()
                .map(|(b, _)| serde_json::Value::String(b.clone()))
                .collect(),
        ),
    );

    Ok(ExtractionResult {
        fields,
        residual: compacted.into_bytes(),
        metadata: HashMap::new(),
        domain_id: "log.java".to_string(),
    })
}

/// Extract thread name from between `[` and `]` immediately following a level keyword.
/// Returns the thread name string WITHOUT brackets, or `None` if not present or too short.
fn extract_thread_name(line: &str) -> Option<&str> {
    const LEVEL_PREFIXES: &[&str] = &[
        " INFO [", " WARN [", " ERROR [", " DEBUG [", " TRACE [", " FATAL [",
    ];
    for level_prefix in LEVEL_PREFIXES {
        if let Some(pos) = line.find(level_prefix) {
            let after_open = pos + level_prefix.len(); // points to char after '['
            if let Some(close_off) = line[after_open..].find(']') {
                let thread_name = &line[after_open..after_open + close_off];
                if thread_name.len() >= MIN_THREAD_LEN {
                    return Some(thread_name);
                }
            }
            break; // found level but thread name too short or no ']'
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Timestamp prefix helpers
// ---------------------------------------------------------------------------

/// Return the byte length of the leading ISO timestamp on `line`, or 0 if none.
///
/// Matches the prefix that looks like a date/time value:
/// - `YYYY-MM-DD HH:MM:SS`  (Hadoop/Log4j): length 19
/// - `YY/MM/DD HH:MM:SS`    (Spark):        length 17
/// - `YYMMDD HHMMSS`        (HDFS):         length 13
///
/// We only require at least 10 bytes (`YYYY-MM-DD`) and that the first 4
/// chars are digits.
fn timestamp_prefix_len(line: &str) -> usize {
    let b = line.as_bytes();
    if b.len() < 10 {
        return 0;
    }
    // Must start with 4 digits (year or compact date prefix).
    if !b[0].is_ascii_digit()
        || !b[1].is_ascii_digit()
        || !b[2].is_ascii_digit()
        || !b[3].is_ascii_digit()
    {
        return 0;
    }
    // Next char must be a known date separator.
    if b[4] != b'-' && b[4] != b'/' && !b[4].is_ascii_digit() {
        return 0;
    }
    // Walk forward consuming characters that belong to a timestamp.
    // Allow one space or 'T' as the date/time separator.
    let mut end = 0usize;
    let mut date_time_sep_seen = false;
    for (i, &c) in b.iter().enumerate().take(24) {
        if c.is_ascii_digit() || c == b'-' || c == b'/' || c == b':' || c == b'.' || c == b',' {
            end = i + 1;
        } else if (c == b' ' || c == b'T') && !date_time_sep_seen && i >= 6 {
            // One space/T separating date from time is allowed once.
            date_time_sep_seen = true;
            end = i + 1;
        } else {
            break;
        }
    }
    if end >= 10 {
        end
    } else {
        0
    }
}

/// Find the longest common prefix shared by all leading timestamps in `text`.
///
/// Returns `None` if:
/// - Fewer than `min_ts_lines` lines carry a timestamp, or
/// - The common prefix is shorter than 10 bytes.
///
/// The returned string is the raw text prefix (e.g. `"2015-10-17 15:"`).
fn extract_timestamp_prefix(text: &str, min_ts_lines: usize) -> Option<String> {
    let mut common: Option<Vec<u8>> = None;
    let mut ts_count = 0usize;

    for line in text.lines() {
        let ts_len = timestamp_prefix_len(line);
        if ts_len == 0 {
            continue;
        }
        let ts_bytes = &line.as_bytes()[..ts_len];
        ts_count += 1;
        common = Some(match common {
            None => ts_bytes.to_vec(),
            Some(prev) => {
                // Compute longest common prefix
                let lcp_len = prev
                    .iter()
                    .zip(ts_bytes.iter())
                    .take_while(|(a, b)| a == b)
                    .count();
                prev[..lcp_len].to_vec()
            }
        });
    }

    if ts_count < min_ts_lines {
        return None;
    }

    let pfx = common?;
    // Minimum prefix length = 7 to capture compact HDFS timestamps: `YYMMDD ` (date + space).
    // Longer formats (Hadoop ISO, Spark YY/MM/DD) still produce prefixes of 10+ chars.
    if pfx.len() < 7 {
        return None;
    }
    // Trim trailing non-separator chars so we end on a clean boundary.
    // E.g. prefer "2015-10-17 15:" over "2015-10-17 15:3" (partial minute).
    let mut trim = pfx.len();
    while trim > 7 {
        let last = pfx[trim - 1];
        if last == b':' || last == b'-' || last == b' ' || last == b'T' {
            break;
        }
        trim -= 1;
    }
    if trim < 7 {
        return None;
    }
    String::from_utf8(pfx[..trim].to_vec()).ok()
}

/// Scan `line` for all HDFS block ID tokens: `blk_` followed by optional `-` then digits.
/// Returns slices into `line`.
fn extract_blk_ids(line: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i + 4 < bytes.len() {
        // Look for `blk_`
        if bytes[i..i + 4] != *b"blk_" {
            i += 1;
            continue;
        }
        // Scan the number: optional `-` then ASCII digits.
        let start = i;
        let mut j = i + 4;
        if j < bytes.len() && bytes[j] == b'-' {
            j += 1;
        }
        let num_start = j;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        // Must have at least one digit after `blk_` (or `blk_-`).
        if j > num_start {
            let tok = &line[start..j];
            if tok.len() >= MIN_BLK_LEN {
                result.push(tok);
            }
        }
        i = j.max(i + 1);
    }
    result
}

fn get_str_vec(fields: &HashMap<String, serde_json::Value>, key: &str) -> CpacResult<Vec<String>> {
    match fields.get(key) {
        Some(serde_json::Value::Array(arr)) => Ok(arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()),
        None => Ok(Vec::new()),
        _ => Err(CpacError::DecompressFailed(format!("Invalid {key} format"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_detect_hadoop() {
        let domain = JavaLogDomain;
        // Build a block > 32 KB with repeated Hadoop-style lines.
        let line = b"2015-10-18 18:01:47,978 INFO [main] org.apache.hadoop.mapreduce.v2.app.MRAppMaster: Created MRAppMaster\n";
        let data: Vec<u8> = line.iter().copied().cycle().take(40_000).collect();
        let conf = domain.detect(&data, None);
        assert!(conf >= 0.8, "Hadoop detection confidence={conf}");
    }

    #[test]
    fn java_detect_spark() {
        let domain = JavaLogDomain;
        let line =
            b"17/06/09 20:10:40 INFO spark.SecurityManager: Changing view acls to: yarn,curi\n";
        let data: Vec<u8> = line.iter().copied().cycle().take(40_000).collect();
        let conf = domain.detect(&data, None);
        assert!(conf >= 0.8, "Spark detection confidence={conf}");
    }

    #[test]
    fn java_detect_hdfs() {
        let domain = JavaLogDomain;
        let line = b"081109 203615 148 INFO dfs.DataNode$PacketResponder: PacketResponder 1 for block blk_12345 terminating\n";
        let data: Vec<u8> = line.iter().copied().cycle().take(40_000).collect();
        let conf = domain.detect(&data, None);
        assert!(conf >= 0.8, "HDFS detection confidence={conf}");
    }

    #[test]
    fn java_roundtrip_hadoop() {
        // Single dominant class triggers passthrough (> MAX_FREQ_FRACTION).
        // Verify the roundtrip still produces correct output.
        let domain = JavaLogDomain;
        let data = b"2015-10-18 18:01:47,978 INFO [main] org.apache.hadoop.mapreduce.v2.app.MRAppMaster: Created\n\
2015-10-18 18:01:48,963 INFO [main] org.apache.hadoop.mapreduce.v2.app.MRAppMaster: Executing\n\
2015-10-18 18:01:49,228 INFO [main] org.apache.hadoop.mapreduce.v2.app.MRAppMaster: Using\n\
2015-10-18 18:01:50,353 INFO [main] org.apache.hadoop.mapreduce.v2.app.MRAppMaster: Output\n";
        let result = domain.extract(data).unwrap();
        let reconstructed = domain.reconstruct(&result).unwrap();
        assert_eq!(data.as_slice(), reconstructed.as_slice());
    }

    #[test]
    fn java_diverse_classes_compress() {
        // 16 lines with 4 different FQCNs, each appearing 4 times (25% ≤ MAX_FREQ_FRACTION).
        // All 4 classes pass the frequency filter and the savings gate → residual shrinks.
        let domain = JavaLogDomain;
        let data =
            b"2015-10-18 18:01:47 INFO [t] org.apache.hadoop.mapreduce.v2.app.MRAppMaster: msg a\n\
2015-10-18 18:01:48 INFO [t] org.apache.hadoop.yarn.event.AsyncDispatcher: msg b\n\
2015-10-18 18:01:49 INFO [t] org.apache.hadoop.mapreduce.lib.output.FileOutputCommitter: msg c\n\
2015-10-18 18:01:50 INFO [t] org.apache.hadoop.mapreduce.v2.app.JobHistoryEventHandler: msg d\n\
2015-10-18 18:01:51 INFO [t] org.apache.hadoop.mapreduce.v2.app.MRAppMaster: msg e\n\
2015-10-18 18:01:52 INFO [t] org.apache.hadoop.yarn.event.AsyncDispatcher: msg f\n\
2015-10-18 18:01:53 INFO [t] org.apache.hadoop.mapreduce.lib.output.FileOutputCommitter: msg g\n\
2015-10-18 18:01:54 INFO [t] org.apache.hadoop.mapreduce.v2.app.JobHistoryEventHandler: msg h\n\
2015-10-18 18:01:55 INFO [t] org.apache.hadoop.mapreduce.v2.app.MRAppMaster: msg i\n\
2015-10-18 18:01:56 INFO [t] org.apache.hadoop.yarn.event.AsyncDispatcher: msg j\n\
2015-10-18 18:01:57 INFO [t] org.apache.hadoop.mapreduce.lib.output.FileOutputCommitter: msg k\n\
2015-10-18 18:01:58 INFO [t] org.apache.hadoop.mapreduce.v2.app.JobHistoryEventHandler: msg l\n\
2015-10-18 18:01:59 INFO [t] org.apache.hadoop.mapreduce.v2.app.MRAppMaster: msg m\n\
2015-10-18 18:02:00 INFO [t] org.apache.hadoop.yarn.event.AsyncDispatcher: msg n\n\
2015-10-18 18:02:01 INFO [t] org.apache.hadoop.mapreduce.lib.output.FileOutputCommitter: msg o\n\
2015-10-18 18:02:02 INFO [t] org.apache.hadoop.mapreduce.v2.app.JobHistoryEventHandler: msg p\n";
        let result = domain.extract(data).unwrap();
        assert!(
            result.residual.len() < data.len(),
            "Diverse-class extraction should shrink the residual"
        );
        let reconstructed = domain.reconstruct(&result).unwrap();
        assert_eq!(data.as_slice(), reconstructed.as_slice());
    }

    #[test]
    fn java_roundtrip_spark() {
        let domain = JavaLogDomain;
        let data =
            b"17/06/09 20:10:40 INFO spark.SecurityManager: Changing view acls to: yarn,curi\n\
17/06/09 20:10:40 INFO spark.SecurityManager: Changing modify acls to: yarn,curi\n\
17/06/09 20:10:41 INFO spark.SecurityManager: SecurityManager: authentication disabled\n\
17/06/09 20:10:41 INFO spark.SecurityManager: Changing view acls to: yarn,curi\n";
        let result = domain.extract(data).unwrap();
        let reconstructed = domain.reconstruct(&result).unwrap();
        assert_eq!(data.as_slice(), reconstructed.as_slice());
    }

    #[test]
    fn java_extract_with_fields_streaming() {
        let domain = JavaLogDomain;
        let block1 = b"2015-10-18 18:01:47,978 INFO [main] org.apache.hadoop.mapreduce.v2.app.MRAppMaster: msg1\n\
2015-10-18 18:01:48,963 INFO [main] org.apache.hadoop.mapreduce.v2.app.MRAppMaster: msg2\n\
2015-10-18 18:01:49,228 INFO [main] org.apache.hadoop.mapreduce.v2.app.MRAppMaster: msg3\n\
2015-10-18 18:01:50,353 INFO [main] org.apache.hadoop.mapreduce.v2.app.MRAppMaster: msg4\n";
        let result1 = domain.extract(block1).unwrap();
        let block2 = b"2015-10-18 18:02:00,001 INFO [main] org.apache.hadoop.mapreduce.v2.app.MRAppMaster: another msg\n";
        let result2 = domain.extract_with_fields(block2, &result1.fields).unwrap();
        let reconstructed = domain.reconstruct(&result2).unwrap();
        assert_eq!(block2.as_slice(), reconstructed.as_slice());
    }

    #[test]
    fn java_roundtrip_with_thread_names() {
        // 12 lines with 4 different thread names, each appearing exactly 3 times.
        // dyn_min_freq = max(3, round(12*0.005)) = 3 → each thread passes (count=3 >= 3).
        // Frequency ratio = 3/12 = 25% = MAX_FREQ_FRACTION → included (<=).
        let domain = JavaLogDomain;
        let data = concat!(
            "2015-10-18 18:01:47 INFO [AsyncDispatcher event handler] org.apache.hadoop.yarn.event.AsyncDispatcher: Register\n",
            "2015-10-18 18:01:48 INFO [RMCommunicator Allocator] org.apache.hadoop.yarn.server.resourcemanager.RMCommunicator: Allocating\n",
            "2015-10-18 18:01:49 INFO [ContainerLauncher Worker] org.apache.hadoop.mapreduce.v2.app.launcher.ContainerLauncher: Starting\n",
            "2015-10-18 18:01:50 INFO [MRAppMaster event handler] org.apache.hadoop.mapreduce.v2.app.MRAppMaster: Handling\n",
            "2015-10-18 18:01:51 INFO [AsyncDispatcher event handler] org.apache.hadoop.yarn.event.AsyncDispatcher: Dispatch\n",
            "2015-10-18 18:01:52 INFO [RMCommunicator Allocator] org.apache.hadoop.yarn.server.resourcemanager.RMCommunicator: Updating\n",
            "2015-10-18 18:01:53 INFO [ContainerLauncher Worker] org.apache.hadoop.mapreduce.v2.app.launcher.ContainerLauncher: Done\n",
            "2015-10-18 18:01:54 INFO [MRAppMaster event handler] org.apache.hadoop.mapreduce.v2.app.MRAppMaster: Complete\n",
            "2015-10-18 18:01:55 INFO [AsyncDispatcher event handler] org.apache.hadoop.yarn.event.AsyncDispatcher: Flush\n",
            "2015-10-18 18:01:56 INFO [RMCommunicator Allocator] org.apache.hadoop.yarn.server.resourcemanager.RMCommunicator: Heartbeat\n",
            "2015-10-18 18:01:57 INFO [ContainerLauncher Worker] org.apache.hadoop.mapreduce.v2.app.launcher.ContainerLauncher: Cleanup\n",
            "2015-10-18 18:01:58 INFO [MRAppMaster event handler] org.apache.hadoop.mapreduce.v2.app.MRAppMaster: Terminate\n",
        );
        let result = domain.extract(data.as_bytes()).unwrap();
        let thread_count = match result.fields.get("threads") {
            Some(serde_json::Value::Array(arr)) => arr.len(),
            _ => 0,
        };
        assert!(
            thread_count > 0,
            "Expected thread names to be extracted, got 0"
        );
        assert!(
            result.residual.len() < data.len(),
            "Thread extraction should shrink residual: {} >= {}",
            result.residual.len(),
            data.len()
        );
        let reconstructed = domain.reconstruct(&result).unwrap();
        assert_eq!(
            data.as_bytes(),
            reconstructed.as_slice(),
            "Thread roundtrip mismatch"
        );
    }

    #[test]
    fn java_hdfs_blk_id_roundtrip() {
        // HDFS-style lines with block IDs repeated multiple times.
        // dyn_min_freq = max(3, round(N*0.005)); cycle to N≥600 lines so dyn_min_freq=3.
        let domain = JavaLogDomain;
        let line = b"081109 203615 148 INFO dfs.DataNode$PacketResponder: PacketResponder for block blk_38865049064139660 terminating\n";
        let data: Vec<u8> = line.iter().copied().cycle().take(40_000).collect();
        let result = domain.extract(&data).unwrap();
        // Block ID must be extracted.
        let blk_count = result
            .fields
            .get("blk_ids")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        assert!(blk_count > 0, "Expected blk_ids to be extracted");
        // Residual must be smaller than input.
        assert!(
            result.residual.len() < data.len(),
            "blk_id extraction should shrink residual"
        );
        // Roundtrip must be lossless.
        let reconstructed = domain.reconstruct(&result).unwrap();
        assert_eq!(
            data.as_slice(),
            reconstructed.as_slice(),
            "HDFS blk_id roundtrip mismatch"
        );
    }

    #[test]
    fn java_thread_streaming_reuse() {
        // 12-line block1 triggers extraction (3 occ each of 4 threads at 25%).
        // extract_with_fields on block2 reuses detection-phase threads → same replacements.
        let domain = JavaLogDomain;
        let block1 = concat!(
            "2015-10-18 18:01:47 INFO [AsyncDispatcher event handler] org.apache.hadoop.yarn.event.AsyncDispatcher: msg1\n",
            "2015-10-18 18:01:48 INFO [RMCommunicator Allocator] org.apache.hadoop.yarn.server.resourcemanager.RMCommunicator: msg2\n",
            "2015-10-18 18:01:49 INFO [ContainerLauncher Worker] org.apache.hadoop.mapreduce.v2.app.launcher.ContainerLauncher: msg3\n",
            "2015-10-18 18:01:50 INFO [MRAppMaster event handler] org.apache.hadoop.mapreduce.v2.app.MRAppMaster: msg4\n",
            "2015-10-18 18:01:51 INFO [AsyncDispatcher event handler] org.apache.hadoop.yarn.event.AsyncDispatcher: msg5\n",
            "2015-10-18 18:01:52 INFO [RMCommunicator Allocator] org.apache.hadoop.yarn.server.resourcemanager.RMCommunicator: msg6\n",
            "2015-10-18 18:01:53 INFO [ContainerLauncher Worker] org.apache.hadoop.mapreduce.v2.app.launcher.ContainerLauncher: msg7\n",
            "2015-10-18 18:01:54 INFO [MRAppMaster event handler] org.apache.hadoop.mapreduce.v2.app.MRAppMaster: msg8\n",
            "2015-10-18 18:01:55 INFO [AsyncDispatcher event handler] org.apache.hadoop.yarn.event.AsyncDispatcher: msg9\n",
            "2015-10-18 18:01:56 INFO [RMCommunicator Allocator] org.apache.hadoop.yarn.server.resourcemanager.RMCommunicator: msg10\n",
            "2015-10-18 18:01:57 INFO [ContainerLauncher Worker] org.apache.hadoop.mapreduce.v2.app.launcher.ContainerLauncher: msg11\n",
            "2015-10-18 18:01:58 INFO [MRAppMaster event handler] org.apache.hadoop.mapreduce.v2.app.MRAppMaster: msg12\n",
        );
        let result1 = domain.extract(block1.as_bytes()).unwrap();
        // Verify block1 extraction worked
        let thread_count = match result1.fields.get("threads") {
            Some(serde_json::Value::Array(arr)) => arr.len(),
            _ => 0,
        };
        assert!(thread_count > 0, "Block1 should extract threads, got 0");
        // Block 2: same thread names appear
        let block2 = concat!(
            "2015-10-18 18:02:00 INFO [AsyncDispatcher event handler] org.apache.hadoop.yarn.event.AsyncDispatcher: new msg\n",
            "2015-10-18 18:02:01 INFO [RMCommunicator Allocator] org.apache.hadoop.yarn.server.resourcemanager.RMCommunicator: update\n",
        );
        let result2 = domain
            .extract_with_fields(block2.as_bytes(), &result1.fields)
            .unwrap();
        let reconstructed = domain.reconstruct(&result2).unwrap();
        assert_eq!(
            block2.as_bytes(),
            reconstructed.as_slice(),
            "Thread streaming roundtrip mismatch"
        );
    }
}
