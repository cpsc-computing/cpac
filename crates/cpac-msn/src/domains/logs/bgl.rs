// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! BGL / Thunderbird supercomputer log domain handler.
//!
//! Handles BlueGene/L and similar epoch-prefixed log formats:
//!
//! BGL:
//!   `- <epoch> YYYY.MM.DD <node_id> <datetime> <node_id> RAS <subsystem> <level> <msg>`
//!
//! Thunderbird (syslog wrapped with BGL epoch prefix):
//!   `- <epoch> YYYY.MM.DD <hostname> <bsd_syslog_line>`
//!
//! Extraction: node/hostname identifiers (field[3], and field[5] for BGL) and
//! stable category tokens (RAS, KERNEL, etc.) are replaced with `@N{n}` and
//! `@S{n}` placeholders.  Node IDs like `R02-M1-N0-C:J12-U11` (19 chars) appear
//! twice per BGL line, saving ~30 bytes per occurrence.

use crate::domain::{Domain, DomainInfo, ExtractionResult};
use cpac_types::{CpacError, CpacResult};
use std::collections::HashMap;

const MIN_TOKEN_LEN: usize = 8;
const MIN_FREQUENCY: usize = 2;
const MIN_USEFUL_SIZE: usize = 32_768; // 32 KB
const MAX_TOKENS: usize = 32;
const DYN_FREQ_RATIO: f64 = 0.005;
const MIN_SAVINGS_BYTES: usize = 256;
/// Bytes saved per line by replacing the 10-char epoch with `@E` (2 chars).
const EPOCH_SAVINGS_PER_LINE: usize = 8;
/// Bytes saved per line by replacing the 26-char BGL datetime with `@D` (2 chars).
const DATETIME_SAVINGS_PER_LINE: usize = 24;

pub struct BglLogDomain;

impl Domain for BglLogDomain {
    fn info(&self) -> DomainInfo {
        DomainInfo {
            id: "log.bgl",
            name: "BGL/Thunderbird Supercomputer Log",
            extensions: &[".log"],
            mime_types: &["text/plain"],
            magic_bytes: &[],
        }
    }

    fn detect(&self, data: &[u8], _filename: Option<&str>) -> f64 {
        let text = std::str::from_utf8(data).unwrap_or("");

        // BGL/Thunderbird: lines start with `- <long_epoch> YYYY.MM.DD `
        let bgl_count = text
            .lines()
            .take(10)
            .filter(|line| is_bgl_line(line))
            .count();

        if bgl_count > 6 {
            if data.len() >= MIN_USEFUL_SIZE {
                0.82
            } else {
                0.35
            }
        } else {
            0.0
        }
    }

    fn extract(&self, data: &[u8]) -> CpacResult<ExtractionResult> {
        let text = std::str::from_utf8(data)
            .map_err(|e| CpacError::CompressFailed(format!("BGL log decode: {e}")))?;
        extract_bgl(text)
    }

    fn extract_with_fields(
        &self,
        data: &[u8],
        fields: &HashMap<String, serde_json::Value>,
    ) -> CpacResult<ExtractionResult> {
        let text = std::str::from_utf8(data)
            .map_err(|e| CpacError::CompressFailed(format!("BGL log decode: {e}")))?;

        let nodes = get_str_vec(fields, "nodes").unwrap_or_default();
        let categories = get_str_vec(fields, "categories").unwrap_or_default();
        let has_epochs = fields
            .get("epoch_deltas")
            .and_then(|v| v.as_array())
            .is_some_and(|a| !a.is_empty());
        let has_datetimes = fields
            .get("datetime_micros")
            .and_then(|v| v.as_array())
            .is_some_and(|a| !a.is_empty());

        if nodes.is_empty() && categories.is_empty() && !has_epochs && !has_datetimes {
            return extract_bgl(text);
        }

        // Recompute per-block datetime fields and epoch deltas from the original text.
        let (tz_offset, micros) = extract_bgl_datetime_fields(text);
        let epoch_deltas = extract_epoch_deltas(text);

        let mut compacted = text.to_string();

        // Replace epochs first (positional replacement, line-by-line).
        if has_epochs || !epoch_deltas.is_empty() {
            compacted = replace_epochs_in_text(&compacted);
        }
        // Replace datetimes after epochs (positional replacement).
        if has_datetimes || !micros.is_empty() {
            compacted = replace_datetimes_in_bgl_text(&compacted);
        }

        // Replace longest tokens first to avoid partial matches.
        let mut all_tokens: Vec<(String, String)> = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.clone(), format!("@N{i}")))
            .chain(
                categories
                    .iter()
                    .enumerate()
                    .map(|(i, c)| (c.clone(), format!("@S{i}"))),
            )
            .collect();
        all_tokens.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        for (orig, placeholder) in &all_tokens {
            compacted = compacted.replace(orig.as_str(), placeholder);
        }

        let mut new_fields = fields.clone();
        new_fields.insert(
            "epoch_deltas".to_string(),
            serde_json::Value::Array(
                epoch_deltas
                    .iter()
                    .map(|&d| serde_json::Value::Number(serde_json::Number::from(d)))
                    .collect(),
            ),
        );
        new_fields.insert(
            "datetime_tz".to_string(),
            serde_json::Value::Number(serde_json::Number::from(tz_offset)),
        );
        new_fields.insert(
            "datetime_micros".to_string(),
            serde_json::Value::Array(
                micros
                    .iter()
                    .map(|&m| serde_json::Value::Number(serde_json::Number::from(m)))
                    .collect(),
            ),
        );

        Ok(ExtractionResult {
            fields: new_fields,
            residual: compacted.into_bytes(),
            metadata: HashMap::new(),
            domain_id: "log.bgl".to_string(),
        })
    }

    fn reconstruct(&self, result: &ExtractionResult) -> CpacResult<Vec<u8>> {
        let nodes = get_str_vec(&result.fields, "nodes").unwrap_or_default();
        let categories = get_str_vec(&result.fields, "categories").unwrap_or_default();
        let epoch_deltas: Vec<i64> = result
            .fields
            .get("epoch_deltas")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect())
            .unwrap_or_default();
        let tz_offset: i64 = result
            .fields
            .get("datetime_tz")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let datetime_micros: Vec<u32> = result
            .fields
            .get("datetime_micros")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_u64().map(|n| n as u32)).collect())
            .unwrap_or_default();

        let mut reconstructed = std::str::from_utf8(&result.residual)
            .map_err(|e| CpacError::DecompressFailed(format!("UTF-8 decode: {e}")))?
            .to_string();

        // Restore epochs first (positional) so field[1] has actual values for datetime restore.
        if !epoch_deltas.is_empty() {
            reconstructed = restore_epochs_in_text(&reconstructed, &epoch_deltas)?;
        }
        // Restore datetimes (uses epoch from field[1] + tz_offset + per-line microseconds).
        if !datetime_micros.is_empty() {
            reconstructed =
                restore_datetimes_in_bgl_text(&reconstructed, tz_offset, &datetime_micros)?;
        }

        // Expand token placeholders in reverse index order so @N10 expands before @N1.
        for (idx, node) in nodes.iter().enumerate().rev() {
            reconstructed = reconstructed.replace(&format!("@N{idx}"), node);
        }
        for (idx, cat) in categories.iter().enumerate().rev() {
            reconstructed = reconstructed.replace(&format!("@S{idx}"), cat);
        }

        Ok(reconstructed.into_bytes())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_bgl_line(line: &str) -> bool {
    let bytes = line.as_bytes();
    if bytes.len() < 20 {
        return false;
    }
    // Must start with "- " then a sequence of digits (epoch), then space, then date YYYY.MM.DD
    if bytes[0] != b'-' || bytes[1] != b' ' {
        return false;
    }
    let rest = &line[2..];
    // Next token must be all digits (epoch)
    let epoch_end = rest.find(' ').unwrap_or(0);
    if epoch_end == 0 {
        return false;
    }
    let epoch_tok = &rest[..epoch_end];
    if !epoch_tok.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    // After epoch, next token must look like YYYY.MM.DD (contains two dots)
    let after_epoch = &rest[epoch_end + 1..];
    let date_end = after_epoch.find(' ').unwrap_or(0);
    if date_end < 8 {
        return false;
    }
    let date_tok = &after_epoch[..date_end];
    date_tok.contains('.') && date_tok.chars().filter(|c| c.is_ascii_digit()).count() >= 6
}

fn extract_bgl(text: &str) -> CpacResult<ExtractionResult> {
    let mut node_freq: HashMap<String, usize> = HashMap::new();
    let mut cat_freq: HashMap<String, usize> = HashMap::new();

    let mut bgl_line_count = 0usize;
    for line in text.lines() {
        if !is_bgl_line(line) {
            continue;
        }
        bgl_line_count += 1;
        let parts: Vec<&str> = line.split_whitespace().collect();
        // fields: [0]="-" [1]=epoch [2]=YYYY.MM.DD [3]=node/host [4]=datetime [5]=node/app ...
        if let Some(node) = parts.get(3) {
            if node.len() >= MIN_TOKEN_LEN {
                *node_freq.entry(node.to_string()).or_insert(0) += 1;
            }
        }
        // For BGL, field[5] is the same node ID again.
        if let Some(node2) = parts.get(5) {
            if node2.len() >= MIN_TOKEN_LEN
                && parts.get(3).is_some_and(|n| *n != *node2)
            {
                // Different from field[3] — treat as separate node entry.
                *node_freq.entry(node2.to_string()).or_insert(0) += 1;
            }
            // If field[5] == field[3], the global replace will handle both occurrences per line.
        }
        // Category tokens: field[6] onward (e.g. "RAS", "KERNEL") — short but repeated on all lines.
        for part in parts.iter().skip(6).take(3) {
            if part.len() >= MIN_TOKEN_LEN
                && part.chars().all(|c| c.is_ascii_uppercase() || c == '_')
            {
                *cat_freq.entry(part.to_string()).or_insert(0) += 1;
            }
        }
    }

    let line_count = text.lines().count().max(1);
    #[allow(clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let dyn_min_freq =
        ((line_count as f64 * DYN_FREQ_RATIO).round() as usize).max(MIN_FREQUENCY);

    let mut repeated_nodes: Vec<(String, usize)> = node_freq
        .into_iter()
        .filter(|(_, count)| *count >= dyn_min_freq)
        .collect();
    repeated_nodes.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(b.1.cmp(&a.1)));
    repeated_nodes.truncate(MAX_TOKENS);

    let mut repeated_cats: Vec<(String, usize)> = cat_freq
        .into_iter()
        .filter(|(_, count)| *count >= dyn_min_freq)
        .collect();
    repeated_cats.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(b.1.cmp(&a.1)));
    repeated_cats.truncate(MAX_TOKENS);

    // Epoch delta encoding: each BGL line has a 10-digit unix epoch as field[1].
    // Replace with @E placeholder; store base + deltas so they can be restored.
    let epoch_deltas = extract_epoch_deltas(text);
    let epoch_savings = if epoch_deltas.len() >= 2 {
        bgl_line_count * EPOCH_SAVINGS_PER_LINE
    } else {
        0
    };

    // Datetime extraction: field[4] = `YYYY-MM-DD-HH.MM.SS.UUUUUU` (26 chars).
    // Reconstruct from epoch + per-TZ-offset + per-line microseconds.
    let (tz_offset, dt_micros) = extract_bgl_datetime_fields(text);
    let datetime_savings = if dt_micros.len() >= 2 {
        bgl_line_count * DATETIME_SAVINGS_PER_LINE
    } else {
        0
    };

    // Savings gate: sum of nodes + categories + epoch savings + datetime savings.
    let gross_savings: usize = repeated_nodes
        .iter()
        .map(|(t, c)| t.len().saturating_sub(4) * c)
        .sum::<usize>()
        + repeated_cats
            .iter()
            .map(|(t, c)| t.len().saturating_sub(4) * c)
            .sum::<usize>()
        + epoch_savings
        + datetime_savings;
    if gross_savings < MIN_SAVINGS_BYTES {
        let mut f = HashMap::new();
        f.insert("nodes".to_string(), serde_json::Value::Array(vec![]));
        f.insert("categories".to_string(), serde_json::Value::Array(vec![]));
        f.insert("epoch_deltas".to_string(), serde_json::Value::Array(vec![]));
        f.insert("datetime_tz".to_string(), serde_json::Value::Number(serde_json::Number::from(0i64)));
        f.insert("datetime_micros".to_string(), serde_json::Value::Array(vec![]));
        return Ok(ExtractionResult {
            fields: f,
            residual: text.as_bytes().to_vec(),
            metadata: HashMap::new(),
            domain_id: "log.bgl".to_string(),
        });
    }

    let mut compacted = text.to_string();

    // Epoch replacement first (line-by-line, positional — must precede global token replaces).
    if !epoch_deltas.is_empty() {
        compacted = replace_epochs_in_text(&compacted);
    }
    // Datetime replacement after epochs (also positional).
    if !dt_micros.is_empty() {
        compacted = replace_datetimes_in_bgl_text(&compacted);
    }
    // Replace nodes (longest first).
    for (idx, (node, _)) in repeated_nodes.iter().enumerate() {
        compacted = compacted.replace(node.as_str(), &format!("@N{idx}"));
    }
    // Replace categories.
    for (idx, (cat, _)) in repeated_cats.iter().enumerate() {
        compacted = compacted.replace(cat.as_str(), &format!("@S{idx}"));
    }

    let mut fields = HashMap::new();
    fields.insert(
        "nodes".to_string(),
        serde_json::Value::Array(
            repeated_nodes
                .iter()
                .map(|(n, _)| serde_json::Value::String(n.clone()))
                .collect(),
        ),
    );
    fields.insert(
        "categories".to_string(),
        serde_json::Value::Array(
            repeated_cats
                .iter()
                .map(|(c, _)| serde_json::Value::String(c.clone()))
                .collect(),
        ),
    );
    fields.insert(
        "epoch_deltas".to_string(),
        serde_json::Value::Array(
            epoch_deltas
                .iter()
                .map(|&d| serde_json::Value::Number(serde_json::Number::from(d)))
                .collect(),
        ),
    );
    fields.insert(
        "datetime_tz".to_string(),
        serde_json::Value::Number(serde_json::Number::from(tz_offset)),
    );
    fields.insert(
        "datetime_micros".to_string(),
        serde_json::Value::Array(
            dt_micros
                .iter()
                .map(|&m| serde_json::Value::Number(serde_json::Number::from(m)))
                .collect(),
        ),
    );

    Ok(ExtractionResult {
        fields,
        residual: compacted.into_bytes(),
        metadata: HashMap::new(),
        domain_id: "log.bgl".to_string(),
    })
}

/// Extract the unix epoch (field[1]) from each BGL line.
/// Returns a delta-encoded array: element[0] is the absolute base epoch,
/// subsequent elements are deltas from the previous value.
fn extract_epoch_deltas(text: &str) -> Vec<i64> {
    let mut epochs: Vec<i64> = Vec::new();
    for line in text.lines() {
        let bytes = line.as_bytes();
        // BGL line starts: "- <digits> ..."
        if bytes.len() < 4 || bytes[0] != b'-' || bytes[1] != b' ' {
            continue;
        }
        let rest = &line[2..];
        let space = rest.find(' ').unwrap_or(0);
        if space == 0 {
            continue;
        }
        let tok = &rest[..space];
        if tok.len() >= 8 && tok.bytes().all(|b| b.is_ascii_digit()) {
            if let Ok(epoch) = tok.parse::<i64>() {
                epochs.push(epoch);
            }
        }
    }
    if epochs.is_empty() {
        return Vec::new();
    }
    // Delta-encode: first element = absolute base, rest = delta from previous.
    let mut deltas = Vec::with_capacity(epochs.len());
    deltas.push(epochs[0]);
    for i in 1..epochs.len() {
        deltas.push(epochs[i] - epochs[i - 1]);
    }
    deltas
}

/// Replace the epoch field in every BGL line with the `@E` placeholder.
fn replace_epochs_in_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let trailing_newline = text.ends_with('\n');
    for line in text.lines() {
        let bytes = line.as_bytes();
        if bytes.len() >= 4 && bytes[0] == b'-' && bytes[1] == b' ' {
            let rest = &line[2..];
            if let Some(space) = rest.find(' ') {
                let tok = &rest[..space];
                if tok.len() >= 8 && tok.bytes().all(|b| b.is_ascii_digit()) {
                    out.push_str("- @E ");
                    out.push_str(&rest[space + 1..]);
                    out.push('\n');
                    continue;
                }
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    if !trailing_newline && out.ends_with('\n') {
        out.pop();
    }
    out
}

/// Restore epoch values from delta-encoded array into `@E` placeholders.
fn restore_epochs_in_text(text: &str, deltas: &[i64]) -> CpacResult<String> {
    // Reconstruct absolute epochs from base + deltas.
    let mut epochs: Vec<i64> = Vec::with_capacity(deltas.len());
    let mut running = 0i64;
    for (i, &d) in deltas.iter().enumerate() {
        running = if i == 0 { d } else { running + d };
        epochs.push(running);
    }

    let mut epoch_iter = epochs.iter();
    let mut out = String::with_capacity(text.len() + deltas.len() * 4);
    let trailing_newline = text.ends_with('\n');
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("- @E ") {
            let epoch = epoch_iter.next().ok_or_else(|| {
                CpacError::DecompressFailed("BGL: not enough epoch_deltas to reconstruct".into())
            })?;
            out.push_str("- ");
            out.push_str(&epoch.to_string());
            out.push(' ');
            out.push_str(rest);
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    if !trailing_newline && out.ends_with('\n') {
        out.pop();
    }
    Ok(out)
}

fn get_str_vec(
    fields: &HashMap<String, serde_json::Value>,
    key: &str,
) -> Option<Vec<String>> {
    fields.get(key).and_then(|v| v.as_array()).map(|arr| {
        arr.iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()
    })
}

// ---------------------------------------------------------------------------
// BGL datetime (field[4]) extraction and civil-calendar helpers
// ---------------------------------------------------------------------------

/// Days since Unix epoch (1970-01-01) for the given proleptic Gregorian date.
/// Uses the civil calendar algorithm by Howard Hinnant (public domain).
fn days_since_unix_epoch(y: i64, m: i64, d: i64) -> i64 {
    let (y, m) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * m + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Unix seconds → (year, month, day, hour, minute, second).
fn unix_secs_to_datetime(secs: i64) -> (i64, i64, i64, i64, i64, i64) {
    let day = secs.div_euclid(86400);
    let sec_of_day = secs.rem_euclid(86400);
    let z = day + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y_era = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let y = y_era + if m <= 2 { 1 } else { 0 };
    (
        y,
        m,
        d,
        sec_of_day / 3600,
        (sec_of_day % 3600) / 60,
        sec_of_day % 60,
    )
}

/// Parse a BGL datetime string `YYYY-MM-DD-HH.MM.SS.UUUUUU` into (unix_seconds, microseconds).
/// Returns `None` if the format doesn't match.
fn parse_bgl_datetime(s: &str) -> Option<(i64, u32)> {
    let b = s.as_bytes();
    // Minimum: YYYY-MM-DD-HH.MM.SS (19 chars). Full: +.UUUUUU (26 chars).
    if b.len() < 19 {
        return None;
    }
    // Validate separators: YYYY-MM-DD-HH.MM.SS
    if b[4] != b'-' || b[7] != b'-' || b[10] != b'-' || b[13] != b'.' || b[16] != b'.' {
        return None;
    }
    // Validate that date/time fields are digits.
    for &idx in &[0usize, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18] {
        if !b[idx].is_ascii_digit() {
            return None;
        }
    }
    let year: i64 = fast_parse_u64(&b[0..4])? as i64;
    let month: i64 = fast_parse_u64(&b[5..7])? as i64;
    let day: i64 = fast_parse_u64(&b[8..10])? as i64;
    let hour: i64 = fast_parse_u64(&b[11..13])? as i64;
    let min: i64 = fast_parse_u64(&b[14..16])? as i64;
    let sec: i64 = fast_parse_u64(&b[17..19])? as i64;
    let micro: u32 = if b.len() >= 26 && b[19] == b'.' {
        fast_parse_u64(&b[20..26])? as u32
    } else {
        0
    };
    let unix_secs = days_since_unix_epoch(year, month, day) * 86400
        + hour * 3600
        + min * 60
        + sec;
    Some((unix_secs, micro))
}

/// Format unix seconds + microseconds as a BGL datetime string `YYYY-MM-DD-HH.MM.SS.UUUUUU`.
fn format_bgl_datetime(unix_secs: i64, micros: u32) -> String {
    let (y, m, d, h, mn, s) = unix_secs_to_datetime(unix_secs);
    format!("{y:04}-{m:02}-{d:02}-{h:02}.{mn:02}.{s:02}.{micros:06}")
}

/// Parse a sequence of ASCII digit bytes into a `u64`.
/// Returns `None` if any byte is not a digit.
#[inline]
fn fast_parse_u64(b: &[u8]) -> Option<u64> {
    let mut n = 0u64;
    for &c in b {
        if !c.is_ascii_digit() {
            return None;
        }
        n = n * 10 + (c - b'0') as u64;
    }
    Some(n)
}

/// Scan each BGL line for field[4] (the human-readable datetime), compute the
/// timezone offset from the first line, and collect per-line microseconds.
///
/// Returns `(tz_offset_seconds, microseconds_per_bgl_line)`.
/// `tz_offset` = `datetime_unix_secs - epoch` for the first valid line.
fn extract_bgl_datetime_fields(text: &str) -> (i64, Vec<u32>) {
    let mut tz_offset: Option<i64> = None;
    let mut micros: Vec<u32> = Vec::new();

    for line in text.lines() {
        if !is_bgl_line(line) {
            continue;
        }
        // Use splitn(6) so field[5+] (message) is not further split.
        let parts: Vec<&str> = line.splitn(6, ' ').collect();
        // parts: [0]="-" [1]=epoch [2]=YYYY.MM.DD [3]=node [4]=datetime [5]=rest
        let Some(epoch_tok) = parts.get(1) else {
            continue;
        };
        let Some(dt_tok) = parts.get(4) else {
            continue;
        };
        let Ok(epoch) = epoch_tok.parse::<i64>() else {
            continue;
        };
        let Some((dt_unix, micro)) = parse_bgl_datetime(dt_tok) else {
            continue;
        };
        micros.push(micro);
        if tz_offset.is_none() {
            tz_offset = Some(dt_unix - epoch);
        }
    }

    (tz_offset.unwrap_or(0), micros)
}

/// Replace field[4] (BGL datetime) in each BGL line with the `@D` placeholder.
/// Epochs will already have been replaced with `@E` when this is called.
///
/// **Guard**: only processes lines that structurally look like a BGL line after
/// epoch replacement (field[0]="-", field[1]="@E" or digits, field[2]=YYYY.MM.DD),
/// matching exactly the set of lines that `extract_bgl_datetime_fields` records
/// microseconds for.  Without this guard, non-BGL lines whose epoch was also
/// replaced would produce `@D` placeholders with no corresponding stored microsecond,
/// causing a roundtrip length mismatch that bypasses MSN entirely.
fn replace_datetimes_in_bgl_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let trailing_newline = text.ends_with('\n');
    for line in text.lines() {
        // Use splitn(6) to protect the message from further splitting.
        let parts: Vec<&str> = line.splitn(6, ' ').collect();
        if parts.len() >= 6 {
            // Structural BGL check (adapted for post-epoch-replacement text where
            // field[1] is "@E" instead of the original digit epoch).
            let is_bgl_shaped = parts[0] == "-"
                && (parts[1] == "@E" || parts[1].bytes().all(|b| b.is_ascii_digit()))
                && {
                    let d = parts[2];
                    d.len() >= 8
                        && d.contains('.')
                        && d.chars().filter(|c| c.is_ascii_digit()).count() >= 6
                };
            let dt_tok = parts[4];
            if is_bgl_shaped && parse_bgl_datetime(dt_tok).is_some() {
                out.push_str(parts[0]);
                out.push(' ');
                out.push_str(parts[1]);
                out.push(' ');
                out.push_str(parts[2]);
                out.push(' ');
                out.push_str(parts[3]);
                out.push_str(" @D ");
                out.push_str(parts[5]);
                out.push('\n');
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    if !trailing_newline && out.ends_with('\n') {
        out.pop();
    }
    out
}

/// Restore `@D` datetime placeholders using the epoch already present in field[1]
/// (epochs must be restored before calling this), the stored TZ offset, and
/// per-line microseconds.
fn restore_datetimes_in_bgl_text(
    text: &str,
    tz_offset: i64,
    micros: &[u32],
) -> CpacResult<String> {
    let mut micro_iter = micros.iter();
    let mut out = String::with_capacity(text.len() + micros.len() * 8);
    let trailing_newline = text.ends_with('\n');
    for line in text.lines() {
        let parts: Vec<&str> = line.splitn(6, ' ').collect();
        // Check: line looks like a BGL line with @D at field[4].
        if parts.len() >= 6 && parts[0] == "-" && parts[4] == "@D" {
            let epoch: i64 =
                parts[1].parse().map_err(|_| {
                    CpacError::DecompressFailed(
                        "BGL: cannot parse epoch for datetime reconstruction".into(),
                    )
                })?;
            let micro = micro_iter.next().ok_or_else(|| {
                CpacError::DecompressFailed(
                    "BGL: not enough datetime_micros to reconstruct".into(),
                )
            })?;
            let datetime = format_bgl_datetime(epoch + tz_offset, *micro);
            out.push_str(parts[0]);
            out.push(' ');
            out.push_str(parts[1]);
            out.push(' ');
            out.push_str(parts[2]);
            out.push(' ');
            out.push_str(parts[3]);
            out.push(' ');
            out.push_str(&datetime);
            out.push(' ');
            out.push_str(parts[5]);
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    if !trailing_newline && out.ends_with('\n') {
        out.pop();
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BGL_LINE: &[u8] = b"- 1117838570 2005.06.03 R02-M1-N0-C:J12-U11 2005-06-03-15.42.50.675872 R02-M1-N0-C:J12-U11 RAS KERNEL INFO instruction cache parity error corrected\n";

    #[test]
    fn bgl_detect() {
        let domain = BglLogDomain;
        let data: Vec<u8> = BGL_LINE.iter().copied().cycle().take(40_000).collect();
        let conf = domain.detect(&data, None);
        assert!(conf >= 0.8, "BGL detection confidence={conf}");
    }

    #[test]
    fn bgl_roundtrip() {
        let domain = BglLogDomain;
        // Need >= MIN_FREQUENCY=2 occurrences of the node ID.
        let data: Vec<u8> = BGL_LINE.iter().copied().cycle().take(5000).collect();
        let result = domain.extract(&data).unwrap();
        let reconstructed = domain.reconstruct(&result).unwrap();
        assert_eq!(data.as_slice(), reconstructed.as_slice());
    }

    #[test]
    fn bgl_residual_smaller() {
        let domain = BglLogDomain;
        let data: Vec<u8> = BGL_LINE.iter().copied().cycle().take(40_000).collect();
        let result = domain.extract(&data).unwrap();
        assert!(
            result.residual.len() < data.len(),
            "Residual should be smaller than original"
        );
    }

    #[test]
    fn bgl_datetime_roundtrip() {
        let domain = BglLogDomain;
        // Cycle enough to pass savings gate.
        let data: Vec<u8> = BGL_LINE.iter().copied().cycle().take(40_000).collect();
        let result = domain.extract(&data).unwrap();
        // Microseconds should be extracted.
        let micro_count = result.fields.get("datetime_micros")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        assert!(micro_count > 0, "Expected datetime_micros to be stored, got 0");
        // Roundtrip must be lossless.
        let reconstructed = domain.reconstruct(&result).unwrap();
        assert_eq!(data.as_slice(), reconstructed.as_slice(), "BGL datetime roundtrip mismatch");
    }

    #[test]
    fn bgl_epoch_delta_roundtrip() {
        let domain = BglLogDomain;
        // Three lines with increasing epochs so delta encoding fires.
        let data = b"- 1117838570 2005.06.03 R02-M1-N0-C:J12-U11 2005-06-03-15.42.50.675872 R02-M1-N0-C:J12-U11 RAS KERNEL INFO msg a\n\
- 1117838572 2005.06.03 R02-M1-N0-C:J12-U11 2005-06-03-15.42.52.000000 R02-M1-N0-C:J12-U11 RAS KERNEL INFO msg b\n\
- 1117838575 2005.06.03 R02-M1-N0-C:J12-U11 2005-06-03-15.42.55.000000 R02-M1-N0-C:J12-U11 RAS KERNEL INFO msg c\n";
        // Cycle to exceed MIN_SAVINGS_BYTES threshold.
        let big: Vec<u8> = data.iter().copied().cycle().take(40_000).collect();
        let result = domain.extract(&big).unwrap();
        // Epoch deltas should be stored.
        let deltas = result.fields.get("epoch_deltas")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        assert!(deltas > 0, "Expected epoch_deltas to be stored");
        // Roundtrip must be lossless.
        let reconstructed = domain.reconstruct(&result).unwrap();
        assert_eq!(big.as_slice(), reconstructed.as_slice(), "BGL epoch roundtrip mismatch");
    }
}
