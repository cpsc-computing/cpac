// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Proxifier proxy log domain handler.
//!
//! Format: `[MM.DD HH:MM:SS] process.exe - host:port action through proxy host:port PROTO`
//!
//! Extraction: process names and proxy `host:port` endpoints are replaced with
//! `@P{n}` and `@E{n}` placeholders respectively.  Since the proxy endpoint
//! often appears TWICE per line, savings can be ~50 bytes per line.

use crate::domain::{Domain, DomainInfo, ExtractionResult};
use cpac_types::{CpacError, CpacResult};
use std::collections::HashMap;

const MIN_FREQUENCY: usize = 2;
const MIN_USEFUL_SIZE: usize = 32_768; // 32 KB
/// Minimum token length — process names < 4 chars save barely anything.
const MIN_PROCESS_LEN: usize = 4;
/// Minimum endpoint (host:port) length to be worth extracting.
const MIN_ENDPOINT_LEN: usize = 8;
const MAX_TOKENS: usize = 32;
const DYN_FREQ_RATIO: f64 = 0.005;
const MIN_SAVINGS_BYTES: usize = 256;

pub struct ProxifierDomain;

impl Domain for ProxifierDomain {
    fn info(&self) -> DomainInfo {
        DomainInfo {
            id: "log.proxifier",
            name: "Proxifier Proxy Log",
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
            .filter(|line| is_proxifier_line(line))
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
            .map_err(|e| CpacError::CompressFailed(format!("Proxifier decode: {e}")))?;
        extract_proxifier(text)
    }

    fn extract_with_fields(
        &self,
        data: &[u8],
        fields: &HashMap<String, serde_json::Value>,
    ) -> CpacResult<ExtractionResult> {
        let text = std::str::from_utf8(data)
            .map_err(|e| CpacError::CompressFailed(format!("Proxifier decode: {e}")))?;

        let processes = get_str_vec(fields, "processes").unwrap_or_default();
        let endpoints = get_str_vec(fields, "endpoints").unwrap_or_default();

        if processes.is_empty() && endpoints.is_empty() {
            return extract_proxifier(text);
        }

        let compacted = compact_proxifier(text, &processes, &endpoints);
        Ok(ExtractionResult {
            fields: fields.clone(),
            residual: compacted.into_bytes(),
            metadata: HashMap::new(),
            domain_id: "log.proxifier".to_string(),
        })
    }

    fn reconstruct(&self, result: &ExtractionResult) -> CpacResult<Vec<u8>> {
        let processes = get_str_vec(&result.fields, "processes").unwrap_or_default();
        let endpoints = get_str_vec(&result.fields, "endpoints").unwrap_or_default();

        let mut reconstructed = std::str::from_utf8(&result.residual)
            .map_err(|e| CpacError::DecompressFailed(format!("UTF-8 decode: {e}")))?
            .to_string();

        // Expand longest-index-first to handle @P10 before @P1.
        for (idx, proc) in processes.iter().enumerate().rev() {
            reconstructed = reconstructed.replace(&format!("@P{idx}"), proc);
        }
        for (idx, ep) in endpoints.iter().enumerate().rev() {
            reconstructed = reconstructed.replace(&format!("@E{idx}"), ep);
        }

        Ok(reconstructed.into_bytes())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_proxifier_line(line: &str) -> bool {
    // Lines start with "[MM.DD HH:MM:SS]" or "[M.D HH:MM:SS]"
    let bytes = line.as_bytes();
    if bytes.len() < 15 {
        return false;
    }
    if bytes[0] != b'[' {
        return false;
    }
    // Find the closing ']'
    let Some(bracket_end) = line.find(']') else {
        return false;
    };
    let ts = &line[1..bracket_end];
    // Timestamp must contain both '.' and ':'
    ts.contains('.') && ts.contains(':')
}

fn extract_proxifier(text: &str) -> CpacResult<ExtractionResult> {
    let mut proc_freq: HashMap<String, usize> = HashMap::new();
    let mut ep_freq: HashMap<String, usize> = HashMap::new();

    for line in text.lines() {
        if !is_proxifier_line(line) {
            continue;
        }
        // After the "] " bracket, first token is the process name.
        let Some(bracket_end) = line.find("] ") else {
            continue;
        };
        let after = &line[bracket_end + 2..];
        // Process name: first token before " -"
        if let Some(dash_pos) = after.find(" - ") {
            let proc_name = &after[..dash_pos];
            if proc_name.len() >= MIN_PROCESS_LEN && !proc_name.contains(' ') {
                *proc_freq.entry(proc_name.to_string()).or_insert(0) += 1;
            }
            // Endpoints: look for host:port patterns (token containing ':' with digits after)
            let rest = &after[dash_pos + 3..];
            for token in rest.split_whitespace() {
                if token.len() >= MIN_ENDPOINT_LEN && looks_like_endpoint(token) {
                    *ep_freq.entry(token.to_string()).or_insert(0) += 1;
                }
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

    let mut repeated_procs: Vec<(String, usize)> = proc_freq
        .into_iter()
        .filter(|(_, count)| *count >= dyn_min_freq)
        .collect();
    repeated_procs.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(b.1.cmp(&a.1)));
    repeated_procs.truncate(MAX_TOKENS);

    let mut repeated_eps: Vec<(String, usize)> = ep_freq
        .into_iter()
        .filter(|(_, count)| *count >= dyn_min_freq)
        .collect();
    repeated_eps.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(b.1.cmp(&a.1)));
    repeated_eps.truncate(MAX_TOKENS);

    // Savings gate.
    let gross_savings: usize = repeated_procs
        .iter()
        .map(|(t, c)| t.len().saturating_sub(4) * c)
        .sum::<usize>()
        + repeated_eps
            .iter()
            .map(|(t, c)| t.len().saturating_sub(4) * c)
            .sum::<usize>();
    if gross_savings < MIN_SAVINGS_BYTES {
        let mut f = HashMap::new();
        f.insert("processes".to_string(), serde_json::Value::Array(vec![]));
        f.insert("endpoints".to_string(), serde_json::Value::Array(vec![]));
        return Ok(ExtractionResult {
            fields: f,
            residual: text.as_bytes().to_vec(),
            metadata: HashMap::new(),
            domain_id: "log.proxifier".to_string(),
        });
    }

    let proc_names: Vec<String> = repeated_procs.iter().map(|(p, _)| p.clone()).collect();
    let ep_names: Vec<String> = repeated_eps.iter().map(|(e, _)| e.clone()).collect();

    let compacted = compact_proxifier(text, &proc_names, &ep_names);

    let mut fields = HashMap::new();
    fields.insert(
        "processes".to_string(),
        serde_json::Value::Array(
            proc_names
                .iter()
                .map(|p| serde_json::Value::String(p.clone()))
                .collect(),
        ),
    );
    fields.insert(
        "endpoints".to_string(),
        serde_json::Value::Array(
            ep_names
                .iter()
                .map(|e| serde_json::Value::String(e.clone()))
                .collect(),
        ),
    );

    Ok(ExtractionResult {
        fields,
        residual: compacted.into_bytes(),
        metadata: HashMap::new(),
        domain_id: "log.proxifier".to_string(),
    })
}

/// Returns true if `token` looks like a `host:port` endpoint.
fn looks_like_endpoint(token: &str) -> bool {
    if let Some(colon) = token.rfind(':') {
        let port_part = &token[colon + 1..];
        // Port must be all digits and non-empty; host must be non-empty and contain dot or alpha.
        let host_part = &token[..colon];
        !host_part.is_empty()
            && !port_part.is_empty()
            && port_part.chars().all(|c| c.is_ascii_digit())
            && (host_part.contains('.') || host_part.chars().any(|c| c.is_ascii_alphabetic()))
    } else {
        false
    }
}

/// Global token replacement, longest-first for processes then endpoints.
fn compact_proxifier(text: &str, processes: &[String], endpoints: &[String]) -> String {
    let mut compacted = text.to_string();
    // Processes
    for (idx, proc) in processes.iter().enumerate() {
        compacted = compacted.replace(proc.as_str(), &format!("@P{idx}"));
    }
    // Endpoints (longest first — already sorted by caller)
    for (idx, ep) in endpoints.iter().enumerate() {
        compacted = compacted.replace(ep.as_str(), &format!("@E{idx}"));
    }
    compacted
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

    const SAMPLE: &[u8] = b"[10.30 16:49:06] chrome.exe - proxy.cse.cuhk.edu.hk:5070 open through proxy proxy.cse.cuhk.edu.hk:5070 HTTPS\n\
[10.30 16:49:06] chrome.exe - proxy.cse.cuhk.edu.hk:5070 open through proxy proxy.cse.cuhk.edu.hk:5070 HTTPS\n\
[10.30 16:49:07] chrome.exe - proxy.cse.cuhk.edu.hk:5070 close, 0 bytes sent, 0 bytes received, lifetime 00:01\n\
[10.30 16:49:08] chrome.exe - proxy.cse.cuhk.edu.hk:5070 open through proxy proxy.cse.cuhk.edu.hk:5070 HTTPS\n";

    #[test]
    fn proxifier_detect() {
        let domain = ProxifierDomain;
        let data: Vec<u8> = SAMPLE.iter().copied().cycle().take(40_000).collect();
        let conf = domain.detect(&data, None);
        assert!(conf >= 0.8, "Proxifier confidence={conf}");
    }

    #[test]
    fn proxifier_roundtrip() {
        let domain = ProxifierDomain;
        let result = domain.extract(SAMPLE).unwrap();
        let reconstructed = domain.reconstruct(&result).unwrap();
        assert_eq!(SAMPLE, reconstructed.as_slice());
    }

    #[test]
    fn proxifier_residual_smaller() {
        let domain = ProxifierDomain;
        let data: Vec<u8> = SAMPLE.iter().copied().cycle().take(40_000).collect();
        let result = domain.extract(&data).unwrap();
        assert!(
            result.residual.len() < data.len(),
            "Residual should be smaller"
        );
    }
}
