// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! HPC (High-Performance Computing) event log domain handler.
//!
//! Format: `<thread_id> <node_id> <subsystem> <event_type> <epoch> <count> <message>`
//!
//! Example:
//!   `134681 node-246 unix.hw state_change.unavailable 1077804742 1 Component State Change: ...`
//!
//! Extraction:
//! - subsystem (field[2]) → `@T{n}` placeholders
//! - event-type (field[3]) → `@V{n}` placeholders
//! - node (field[1]) → `@N{n}` placeholders
//! - epoch (field[4], 10-digit unix) → `@E` placeholder with delta encoding
//! - thread IDs (field[0], 4-7 digit ints) → `@F{n}` placeholders

use crate::domain::{Domain, DomainInfo, ExtractionResult};
use cpac_types::{CpacError, CpacResult};
use std::collections::HashMap;

const MIN_FREQUENCY: usize = 2;
const MIN_TOKEN_LEN: usize = 6;
/// Minimum thread-ID length to bother extracting.
const MIN_TID_LEN: usize = 4;
const MIN_USEFUL_SIZE: usize = 16_384; // 16 KB
const MAX_TOKENS: usize = 32;
const DYN_FREQ_RATIO: f64 = 0.005;
const MIN_SAVINGS_BYTES: usize = 256;
/// Bytes saved per line by replacing the 10-char epoch with `@E` (2 chars).
const EPOCH_SAVINGS_PER_LINE: usize = 8;

pub struct HpcLogDomain;

impl Domain for HpcLogDomain {
    fn info(&self) -> DomainInfo {
        DomainInfo {
            id: "log.hpc",
            name: "HPC Event Log",
            extensions: &[".log"],
            mime_types: &["text/plain"],
            magic_bytes: &[],
        }
    }

    fn detect(&self, data: &[u8], _filename: Option<&str>) -> f64 {
        let text = std::str::from_utf8(data).unwrap_or("");

        let count = text
            .lines()
            .take(10)
            .filter(|line| is_hpc_line(line))
            .count();

        if count > 6 {
            if data.len() >= MIN_USEFUL_SIZE {
                0.80
            } else {
                0.35
            }
        } else {
            0.0
        }
    }

    fn extract(&self, data: &[u8]) -> CpacResult<ExtractionResult> {
        if data.len() > crate::MAX_DOMAIN_EXTRACT_SIZE {
            return Err(CpacError::CompressFailed(
                "HPC log: exceeds extraction size limit".into(),
            ));
        }
        let text = std::str::from_utf8(data)
            .map_err(|e| CpacError::CompressFailed(format!("HPC log decode: {e}")))?;
        extract_hpc(text)
    }

    fn extract_with_fields(
        &self,
        data: &[u8],
        fields: &HashMap<String, serde_json::Value>,
    ) -> CpacResult<ExtractionResult> {
        if data.len() > crate::MAX_DOMAIN_EXTRACT_SIZE {
            return Err(CpacError::CompressFailed(
                "HPC log: exceeds extraction size limit".into(),
            ));
        }
        let text = std::str::from_utf8(data)
            .map_err(|e| CpacError::CompressFailed(format!("HPC log decode: {e}")))?;

        let subsystems = get_str_vec(fields, "subsystems").unwrap_or_default();
        let events = get_str_vec(fields, "events").unwrap_or_default();
        let nodes = get_str_vec(fields, "nodes").unwrap_or_default();
        let tids = get_str_vec(fields, "tids").unwrap_or_default();
        let has_epochs = fields
            .get("epoch_deltas")
            .and_then(|v| v.as_array())
            .is_some_and(|a| !a.is_empty());

        if subsystems.is_empty()
            && events.is_empty()
            && nodes.is_empty()
            && tids.is_empty()
            && !has_epochs
        {
            return extract_hpc(text);
        }

        // Recompute epoch deltas for this block.
        let epoch_deltas = extract_hpc_epoch_deltas(text);

        // Apply epoch replacement first (positional, line-by-line).
        let mut compacted = if has_epochs || !epoch_deltas.is_empty() {
            replace_hpc_epochs_in_text(text)
        } else {
            text.to_string()
        };

        // Apply token replacements (global).
        compacted = compact_hpc(&compacted, &subsystems, &events, &nodes, &tids);

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

        Ok(ExtractionResult {
            fields: new_fields,
            residual: compacted.into_bytes(),
            metadata: HashMap::new(),
            domain_id: "log.hpc".to_string(),
        })
    }

    fn reconstruct(&self, result: &ExtractionResult) -> CpacResult<Vec<u8>> {
        let subsystems = get_str_vec(&result.fields, "subsystems").unwrap_or_default();
        let events = get_str_vec(&result.fields, "events").unwrap_or_default();
        let nodes = get_str_vec(&result.fields, "nodes").unwrap_or_default();
        let tids = get_str_vec(&result.fields, "tids").unwrap_or_default();
        let epoch_deltas: Vec<i64> = result
            .fields
            .get("epoch_deltas")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect())
            .unwrap_or_default();

        let mut reconstructed = std::str::from_utf8(&result.residual)
            .map_err(|e| CpacError::DecompressFailed(format!("UTF-8 decode: {e}")))?
            .to_string();

        // Restore epochs first (positional replacement).
        if !epoch_deltas.is_empty() {
            reconstructed = restore_hpc_epochs_in_text(&reconstructed, &epoch_deltas)?;
        }

        // Expand token placeholders in reverse index order.
        for (idx, sub) in subsystems.iter().enumerate().rev() {
            reconstructed = reconstructed.replace(&format!("@T{idx}"), sub);
        }
        for (idx, ev) in events.iter().enumerate().rev() {
            reconstructed = reconstructed.replace(&format!("@V{idx}"), ev);
        }
        for (idx, node) in nodes.iter().enumerate().rev() {
            reconstructed = reconstructed.replace(&format!("@N{idx}"), node);
        }
        for (idx, tid) in tids.iter().enumerate().rev() {
            reconstructed = reconstructed.replace(&format!("@F{idx}"), tid);
        }

        Ok(reconstructed.into_bytes())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns true when `line` looks like an HPC event-log entry.
fn is_hpc_line(line: &str) -> bool {
    let parts: Vec<&str> = line.splitn(7, ' ').collect();
    if parts.len() < 5 {
        return false;
    }
    // field[0]: numeric thread/job id
    if !parts[0].chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    // field[1]: node id containing '-'
    if !parts[1].contains('-') {
        return false;
    }
    // field[2]: dotted subsystem token (e.g. unix.hw)
    if !parts[2].contains('.') {
        return false;
    }
    // field[3]: dotted event type (e.g. state_change.unavailable)
    if !parts[3].contains('.') && !parts[3].contains('_') {
        return false;
    }
    // field[4]: epoch-like large integer
    parts[4].chars().all(|c| c.is_ascii_digit())
}

fn extract_hpc(text: &str) -> CpacResult<ExtractionResult> {
    let mut sub_freq: HashMap<String, usize> = HashMap::new();
    let mut ev_freq: HashMap<String, usize> = HashMap::new();
    let mut node_freq: HashMap<String, usize> = HashMap::new();
    let mut tid_freq: HashMap<String, usize> = HashMap::new();

    let mut hpc_line_count = 0usize;
    for line in text.lines() {
        if !is_hpc_line(line) {
            continue;
        }
        hpc_line_count += 1;
        let parts: Vec<&str> = line.splitn(7, ' ').collect();
        // field[0]: thread / job id (all digits, e.g. "134681").
        if let Some(tid) = parts.first() {
            if tid.len() >= MIN_TID_LEN && tid.bytes().all(|b| b.is_ascii_digit()) {
                *tid_freq.entry(tid.to_string()).or_insert(0) += 1;
            }
        }
        // field[1]: node id (e.g. "node-246") — varies but repeats across many lines.
        if let Some(node) = parts.get(1) {
            if node.len() >= MIN_TOKEN_LEN {
                *node_freq.entry(node.to_string()).or_insert(0) += 1;
            }
        }
        if let Some(sub) = parts.get(2) {
            if sub.len() >= MIN_TOKEN_LEN {
                *sub_freq.entry(sub.to_string()).or_insert(0) += 1;
            }
        }
        if let Some(ev) = parts.get(3) {
            if ev.len() >= MIN_TOKEN_LEN {
                *ev_freq.entry(ev.to_string()).or_insert(0) += 1;
            }
        }
    }

    let line_count = text.lines().count().max(1);
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation
    )]
    let dyn_min_freq = ((line_count as f64 * DYN_FREQ_RATIO).round() as usize).max(MIN_FREQUENCY);

    let mut repeated_nodes: Vec<(String, usize)> = node_freq
        .into_iter()
        .filter(|(tok, count)| tok.len() >= MIN_TOKEN_LEN && *count >= dyn_min_freq)
        .collect();
    repeated_nodes.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(b.1.cmp(&a.1)));
    repeated_nodes.truncate(MAX_TOKENS);

    let mut repeated_subs: Vec<(String, usize)> = sub_freq
        .into_iter()
        .filter(|(_, count)| *count >= dyn_min_freq)
        .collect();
    repeated_subs.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(b.1.cmp(&a.1)));
    repeated_subs.truncate(MAX_TOKENS);

    let mut repeated_evs: Vec<(String, usize)> = ev_freq
        .into_iter()
        .filter(|(_, count)| *count >= dyn_min_freq)
        .collect();
    repeated_evs.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(b.1.cmp(&a.1)));
    repeated_evs.truncate(MAX_TOKENS);

    let mut repeated_tids: Vec<(String, usize)> = tid_freq
        .into_iter()
        .filter(|(tok, count)| tok.len() >= MIN_TID_LEN && *count >= dyn_min_freq)
        .collect();
    repeated_tids.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(b.1.cmp(&a.1)));
    repeated_tids.truncate(MAX_TOKENS);

    // Epoch delta encoding: each HPC line has a 10-digit unix epoch as field[4].
    let epoch_deltas = extract_hpc_epoch_deltas(text);
    let epoch_savings = if epoch_deltas.len() >= 2 {
        hpc_line_count * EPOCH_SAVINGS_PER_LINE
    } else {
        0
    };

    // Savings gate: nodes + subsystems + events + TIDs + epochs.
    let gross_savings: usize = repeated_nodes
        .iter()
        .map(|(t, c)| t.len().saturating_sub(4) * c)
        .sum::<usize>()
        + repeated_subs
            .iter()
            .map(|(t, c)| t.len().saturating_sub(4) * c)
            .sum::<usize>()
        + repeated_evs
            .iter()
            .map(|(t, c)| t.len().saturating_sub(4) * c)
            .sum::<usize>()
        + repeated_tids
            .iter()
            .map(|(t, c)| t.len().saturating_sub(3) * c)
            .sum::<usize>()
        + epoch_savings;
    if gross_savings < MIN_SAVINGS_BYTES {
        let mut f = HashMap::new();
        f.insert("nodes".to_string(), serde_json::Value::Array(vec![]));
        f.insert("subsystems".to_string(), serde_json::Value::Array(vec![]));
        f.insert("events".to_string(), serde_json::Value::Array(vec![]));
        f.insert("tids".to_string(), serde_json::Value::Array(vec![]));
        f.insert("epoch_deltas".to_string(), serde_json::Value::Array(vec![]));
        return Ok(ExtractionResult {
            fields: f,
            residual: text.as_bytes().to_vec(),
            metadata: HashMap::new(),
            domain_id: "log.hpc".to_string(),
        });
    }

    let node_names: Vec<String> = repeated_nodes.iter().map(|(n, _)| n.clone()).collect();
    let sub_names: Vec<String> = repeated_subs.iter().map(|(s, _)| s.clone()).collect();
    let ev_names: Vec<String> = repeated_evs.iter().map(|(e, _)| e.clone()).collect();
    let tid_names: Vec<String> = repeated_tids.iter().map(|(t, _)| t.clone()).collect();

    // Apply epoch replacement first (positional), then global token replacements.
    let epoch_replaced = if !epoch_deltas.is_empty() {
        replace_hpc_epochs_in_text(text)
    } else {
        text.to_string()
    };
    let compacted = compact_hpc(
        &epoch_replaced,
        &sub_names,
        &ev_names,
        &node_names,
        &tid_names,
    );

    let mut fields = HashMap::new();
    fields.insert(
        "nodes".to_string(),
        serde_json::Value::Array(
            node_names
                .iter()
                .map(|n| serde_json::Value::String(n.clone()))
                .collect(),
        ),
    );
    fields.insert(
        "subsystems".to_string(),
        serde_json::Value::Array(
            sub_names
                .iter()
                .map(|s| serde_json::Value::String(s.clone()))
                .collect(),
        ),
    );
    fields.insert(
        "events".to_string(),
        serde_json::Value::Array(
            ev_names
                .iter()
                .map(|e| serde_json::Value::String(e.clone()))
                .collect(),
        ),
    );
    fields.insert(
        "tids".to_string(),
        serde_json::Value::Array(
            tid_names
                .iter()
                .map(|t| serde_json::Value::String(t.clone()))
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

    Ok(ExtractionResult {
        fields,
        residual: compacted.into_bytes(),
        metadata: HashMap::new(),
        domain_id: "log.hpc".to_string(),
    })
}

fn compact_hpc(
    text: &str,
    subsystems: &[String],
    events: &[String],
    nodes: &[String],
    tids: &[String],
) -> String {
    let mut compacted = text.to_string();
    // Replace longest tokens first to prevent partial matches.
    // Nodes before subsystems/events (node IDs can be longer).
    // TIDs last (shortest, least risk of partial matches with longer tokens).
    let mut all_tokens: Vec<(String, String)> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.clone(), format!("@N{i}")))
        .chain(
            subsystems
                .iter()
                .enumerate()
                .map(|(i, s)| (s.clone(), format!("@T{i}"))),
        )
        .chain(
            events
                .iter()
                .enumerate()
                .map(|(i, e)| (e.clone(), format!("@V{i}"))),
        )
        .chain(
            tids.iter()
                .enumerate()
                .map(|(i, t)| (t.clone(), format!("@F{i}"))),
        )
        .collect();
    all_tokens.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
    for (orig, placeholder) in &all_tokens {
        compacted = compacted.replace(orig.as_str(), placeholder);
    }
    compacted
}

// ---------------------------------------------------------------------------
// Epoch delta encoding helpers
// ---------------------------------------------------------------------------

/// Extract HPC epoch (field[4]) from each HPC line, delta-encode the array.
/// Returns `[base, delta1, delta2, ...]`.
fn extract_hpc_epoch_deltas(text: &str) -> Vec<i64> {
    let mut epochs: Vec<i64> = Vec::new();
    for line in text.lines() {
        if !is_hpc_line(line) {
            continue;
        }
        // Use splitn to protect the message field from splitting.
        let parts: Vec<&str> = line.splitn(6, ' ').collect();
        if let Some(tok) = parts.get(4) {
            if tok.len() >= 8 && tok.bytes().all(|b| b.is_ascii_digit()) {
                if let Ok(epoch) = tok.parse::<i64>() {
                    epochs.push(epoch);
                }
            }
        }
    }
    if epochs.is_empty() {
        return Vec::new();
    }
    let mut deltas = Vec::with_capacity(epochs.len());
    deltas.push(epochs[0]);
    for i in 1..epochs.len() {
        deltas.push(epochs[i] - epochs[i - 1]);
    }
    deltas
}

/// Replace field[4] (unix epoch) in each HPC line with the `@E` placeholder.
///
/// **Guard**: only processes lines that pass `is_hpc_line`, matching exactly the
/// set of lines that `extract_hpc_epoch_deltas` records epochs for.  Without this
/// guard, non-HPC lines with a large integer at field[4] would produce `@E`
/// placeholders with no corresponding stored delta, causing a reconstruction error
/// that bypasses MSN entirely.
fn replace_hpc_epochs_in_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let trailing_newline = text.ends_with('\n');
    for line in text.lines() {
        if !is_hpc_line(line) {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let parts: Vec<&str> = line.splitn(6, ' ').collect();
        if parts.len() >= 6 {
            let tok = parts[4];
            if tok.len() >= 8 && tok.bytes().all(|b| b.is_ascii_digit()) {
                out.push_str(parts[0]);
                out.push(' ');
                out.push_str(parts[1]);
                out.push(' ');
                out.push_str(parts[2]);
                out.push(' ');
                out.push_str(parts[3]);
                out.push_str(" @E ");
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

/// Restore `@E` epoch placeholders from the delta-encoded array.
fn restore_hpc_epochs_in_text(text: &str, deltas: &[i64]) -> CpacResult<String> {
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
        let parts: Vec<&str> = line.splitn(6, ' ').collect();
        if parts.len() >= 6 && parts[4] == "@E" {
            let epoch = epoch_iter.next().ok_or_else(|| {
                CpacError::DecompressFailed("HPC: not enough epoch_deltas to reconstruct".into())
            })?;
            out.push_str(parts[0]);
            out.push(' ');
            out.push_str(parts[1]);
            out.push(' ');
            out.push_str(parts[2]);
            out.push(' ');
            out.push_str(parts[3]);
            out.push(' ');
            out.push_str(&epoch.to_string());
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

fn get_str_vec(fields: &HashMap<String, serde_json::Value>, key: &str) -> Option<Vec<String>> {
    fields.get(key).and_then(|v| v.as_array()).map(|arr| {
        arr.iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &[u8] = b"134681 node-246 unix.hw state_change.unavailable 1077804742 1 Component State Change: Component is unavailable\n\
350766 node-109 unix.hw state_change.unavailable 1084680778 1 Component State Change: Component is unavailable\n\
344518 node-246 unix.hw state_change.unavailable 1084270955 1 Component State Change: Component is unavailable\n\
344448 node-153 unix.hw state_change.unavailable 1084270952 1 Component State Change: Component is unavailable\n";

    #[test]
    fn hpc_detect() {
        let domain = HpcLogDomain;
        let data: Vec<u8> = SAMPLE.iter().copied().cycle().take(20_000).collect();
        let conf = domain.detect(&data, None);
        assert!(conf >= 0.75, "HPC confidence={conf}");
    }

    #[test]
    fn hpc_roundtrip() {
        let domain = HpcLogDomain;
        let result = domain.extract(SAMPLE).unwrap();
        let reconstructed = domain.reconstruct(&result).unwrap();
        assert_eq!(SAMPLE, reconstructed.as_slice());
    }

    #[test]
    fn hpc_residual_smaller() {
        let domain = HpcLogDomain;
        let data: Vec<u8> = SAMPLE.iter().copied().cycle().take(20_000).collect();
        let result = domain.extract(&data).unwrap();
        assert!(
            result.residual.len() < data.len(),
            "Residual should be smaller"
        );
    }

    #[test]
    fn hpc_epoch_delta_roundtrip() {
        let domain = HpcLogDomain;
        let data: Vec<u8> = SAMPLE.iter().copied().cycle().take(20_000).collect();
        let result = domain.extract(&data).unwrap();
        let deltas = result
            .fields
            .get("epoch_deltas")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        assert!(deltas > 0, "Expected epoch_deltas to be stored, got 0");
        let reconstructed = domain.reconstruct(&result).unwrap();
        assert_eq!(
            data.as_slice(),
            reconstructed.as_slice(),
            "HPC epoch roundtrip mismatch"
        );
    }

    #[test]
    fn hpc_tid_extraction_roundtrip() {
        let domain = HpcLogDomain;
        // 20k of the sample gives enough repetition for tid extraction.
        let data: Vec<u8> = SAMPLE.iter().copied().cycle().take(20_000).collect();
        let result = domain.extract(&data).unwrap();
        let tid_count = result
            .fields
            .get("tids")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        // At least some TIDs should be extracted from the repeated sample.
        assert!(tid_count > 0, "Expected TIDs to be extracted, got 0");
        let reconstructed = domain.reconstruct(&result).unwrap();
        assert_eq!(
            data.as_slice(),
            reconstructed.as_slice(),
            "HPC TID roundtrip mismatch"
        );
    }

    #[test]
    fn hpc_streaming_reuse() {
        let domain = HpcLogDomain;
        let block1: Vec<u8> = SAMPLE.iter().copied().cycle().take(20_000).collect();
        let result1 = domain.extract(&block1).unwrap();
        // block2: same structure, different epoch values
        let block2 =
            b"134681 node-246 unix.hw state_change.unavailable 1090000000 1 another event\n\
350766 node-109 unix.hw state_change.unavailable 1090000010 1 another event\n";
        let result2 = domain.extract_with_fields(block2, &result1.fields).unwrap();
        let reconstructed = domain.reconstruct(&result2).unwrap();
        assert_eq!(
            block2.as_slice(),
            reconstructed.as_slice(),
            "HPC streaming roundtrip mismatch"
        );
    }
}
