// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! HealthApp pipe-delimited mobile log domain handler.
//!
//! Format: `YYYYMMDD-HH:MM:SS:ms|Component|SessionID|message`
//!
//! Extraction: component names (field[1]) and session IDs (field[2]) are
//! replaced with `@K{n}` and `@D{n}` placeholders respectively.
//! Component tokens like `Step_StandStepCounter` (21 chars) appear on every
//! line, saving ~18 bytes per occurrence.

use crate::domain::{Domain, DomainInfo, ExtractionResult};
use cpac_types::{CpacError, CpacResult};
use std::collections::HashMap;

const MIN_FREQUENCY: usize = 2;
const MIN_USEFUL_SIZE: usize = 16_384; // 16 KB
const MAX_TOKENS: usize = 32;
const DYN_FREQ_RATIO: f64 = 0.005;
const MIN_SAVINGS_BYTES: usize = 256;

pub struct HealthAppDomain;

impl Domain for HealthAppDomain {
    fn info(&self) -> DomainInfo {
        DomainInfo {
            id: "log.healthapp",
            name: "HealthApp Mobile Log",
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
            .filter(|line| is_healthapp_line(line))
            .count();

        if count > 6 {
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
            .map_err(|e| CpacError::CompressFailed(format!("HealthApp decode: {e}")))?;
        extract_healthapp(text)
    }

    fn extract_with_fields(
        &self,
        data: &[u8],
        fields: &HashMap<String, serde_json::Value>,
    ) -> CpacResult<ExtractionResult> {
        let text = std::str::from_utf8(data)
            .map_err(|e| CpacError::CompressFailed(format!("HealthApp decode: {e}")))?;

        let components = get_str_vec(fields, "components").unwrap_or_default();
        let sessions = get_str_vec(fields, "sessions").unwrap_or_default();

        if components.is_empty() && sessions.is_empty() {
            return extract_healthapp(text);
        }

        let compacted = compact_healthapp(text, &components, &sessions);

        Ok(ExtractionResult {
            fields: fields.clone(),
            residual: compacted.into_bytes(),
            metadata: HashMap::new(),
            domain_id: "log.healthapp".to_string(),
        })
    }

    fn reconstruct(&self, result: &ExtractionResult) -> CpacResult<Vec<u8>> {
        let components = get_str_vec(&result.fields, "components").unwrap_or_default();
        let sessions = get_str_vec(&result.fields, "sessions").unwrap_or_default();

        let mut reconstructed = std::str::from_utf8(&result.residual)
            .map_err(|e| CpacError::DecompressFailed(format!("UTF-8 decode: {e}")))?
            .to_string();

        // Expand in reverse index order so @K10 expands before @K1.
        for (idx, comp) in components.iter().enumerate().rev() {
            reconstructed = reconstructed.replace(&format!("@K{idx}"), comp);
        }
        for (idx, sess) in sessions.iter().enumerate().rev() {
            reconstructed = reconstructed.replace(&format!("@D{idx}"), sess);
        }

        Ok(reconstructed.into_bytes())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns true if the line matches the HealthApp timestamp|field|field pattern.
fn is_healthapp_line(line: &str) -> bool {
    // Expected: YYYYMMDD-HH:MM:SS:ms|...
    let bytes = line.as_bytes();
    if bytes.len() < 18 {
        return false;
    }
    // Quick check: first 8 chars are digits, then '-', then digits and colons up to first '|'
    let all_digits_start = bytes[..8].iter().all(|b| b.is_ascii_digit());
    if !all_digits_start || bytes[8] != b'-' {
        return false;
    }
    // Must contain at least two pipe separators
    line.matches('|').count() >= 2
}

fn extract_healthapp(text: &str) -> CpacResult<ExtractionResult> {
    let mut comp_freq: HashMap<String, usize> = HashMap::new();
    let mut sess_freq: HashMap<String, usize> = HashMap::new();

    for line in text.lines() {
        if !is_healthapp_line(line) {
            continue;
        }
        let parts: Vec<&str> = line.splitn(4, '|').collect();
        if parts.len() >= 3 {
            // field[1] = component
            let comp = parts[1];
            if !comp.is_empty() {
                *comp_freq.entry(comp.to_string()).or_insert(0) += 1;
            }
            // field[2] = session ID
            let sess = parts[2];
            if !sess.is_empty() {
                *sess_freq.entry(sess.to_string()).or_insert(0) += 1;
            }
        }
    }

    let line_count = text.lines().count().max(1);
    #[allow(clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let dyn_min_freq =
        ((line_count as f64 * DYN_FREQ_RATIO).round() as usize).max(MIN_FREQUENCY);

    let mut repeated_comps: Vec<(String, usize)> = comp_freq
        .into_iter()
        .filter(|(_, count)| *count >= dyn_min_freq)
        .collect();
    repeated_comps.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(b.1.cmp(&a.1)));
    repeated_comps.truncate(MAX_TOKENS);

    let mut repeated_sess: Vec<(String, usize)> = sess_freq
        .into_iter()
        .filter(|(_, count)| *count >= dyn_min_freq)
        .collect();
    repeated_sess.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(b.1.cmp(&a.1)));
    repeated_sess.truncate(MAX_TOKENS);

    // Savings gate.
    let gross_savings: usize = repeated_comps
        .iter()
        .map(|(t, c)| t.len().saturating_sub(4) * c)
        .sum::<usize>()
        + repeated_sess
            .iter()
            .map(|(t, c)| t.len().saturating_sub(4) * c)
            .sum::<usize>();
    if gross_savings < MIN_SAVINGS_BYTES {
        let mut f = HashMap::new();
        f.insert("components".to_string(), serde_json::Value::Array(vec![]));
        f.insert("sessions".to_string(), serde_json::Value::Array(vec![]));
        return Ok(ExtractionResult {
            fields: f,
            residual: text.as_bytes().to_vec(),
            metadata: HashMap::new(),
            domain_id: "log.healthapp".to_string(),
        });
    }

    let comp_names: Vec<String> = repeated_comps.iter().map(|(c, _)| c.clone()).collect();
    let sess_names: Vec<String> = repeated_sess.iter().map(|(s, _)| s.clone()).collect();

    let compacted = compact_healthapp(text, &comp_names, &sess_names);

    let mut fields = HashMap::new();
    fields.insert(
        "components".to_string(),
        serde_json::Value::Array(
            comp_names
                .iter()
                .map(|c| serde_json::Value::String(c.clone()))
                .collect(),
        ),
    );
    fields.insert(
        "sessions".to_string(),
        serde_json::Value::Array(
            sess_names
                .iter()
                .map(|s| serde_json::Value::String(s.clone()))
                .collect(),
        ),
    );

    Ok(ExtractionResult {
        fields,
        residual: compacted.into_bytes(),
        metadata: HashMap::new(),
        domain_id: "log.healthapp".to_string(),
    })
}

/// Apply component and session token compaction line-by-line.
/// We target only the specific pipe-delimited fields to avoid false positives
/// from matching inside message bodies.
fn compact_healthapp(text: &str, components: &[String], sessions: &[String]) -> String {
    // Build lookup maps.
    let comp_map: HashMap<&str, String> = components
        .iter()
        .enumerate()
        .map(|(i, c)| (c.as_str(), format!("@K{i}")))
        .collect();
    let sess_map: HashMap<&str, String> = sessions
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), format!("@D{i}")))
        .collect();

    let mut out = String::with_capacity(text.len());
    for line in text.split('\n') {
        if is_healthapp_line(line) {
            // splitn(4,'|') gives [timestamp, component, session, rest_of_line]
            let parts: Vec<&str> = line.splitn(4, '|').collect();
            if parts.len() == 4 {
                let comp_out = comp_map.get(parts[1]).map_or(parts[1], |s| s.as_str());
                let sess_out = sess_map.get(parts[2]).map_or(parts[2], |s| s.as_str());
                out.push_str(parts[0]);
                out.push('|');
                out.push_str(comp_out);
                out.push('|');
                out.push_str(sess_out);
                out.push('|');
                out.push_str(parts[3]);
                out.push('\n');
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    // `split('\n')` always appends '\n' for each element (including the trailing
    // empty element produced when text ends with '\n').  Pop the extra '\n' once;
    // what remains is exactly the trailing state of the original text.
    if out.ends_with('\n') {
        out.pop();
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &[u8] = b"20171223-22:15:29:606|Step_LSC|30002312|onStandStepChanged 3579\n\
20171223-22:15:29:615|Step_LSC|30002312|onExtend:1514038530000 14 0 4\n\
20171223-22:15:29:633|Step_StandReportReceiver|30002312|onReceive action: SCREEN_ON\n\
20171223-22:15:29:635|Step_LSC|30002312|processHandleBroadcastAction action:SCREEN_ON\n\
20171223-22:15:29:635|Step_StandStepCounter|30002312|flush sensor data\n";

    #[test]
    fn healthapp_detect() {
        let domain = HealthAppDomain;
        let data: Vec<u8> = SAMPLE.iter().copied().cycle().take(20_000).collect();
        let conf = domain.detect(&data, None);
        assert!(conf >= 0.8, "HealthApp confidence={conf}");
    }

    #[test]
    fn healthapp_roundtrip() {
        let domain = HealthAppDomain;
        let result = domain.extract(SAMPLE).unwrap();
        let reconstructed = domain.reconstruct(&result).unwrap();
        assert_eq!(SAMPLE, reconstructed.as_slice());
    }

    #[test]
    fn healthapp_residual_smaller() {
        let domain = HealthAppDomain;
        // Use a larger block so tokens appear frequently enough.
        let data: Vec<u8> = SAMPLE.iter().copied().cycle().take(20_000).collect();
        let result = domain.extract(&data).unwrap();
        assert!(result.residual.len() < data.len(), "Residual should be smaller");
    }
}
