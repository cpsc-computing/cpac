// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! OpenStack structured log domain handler.
//!
//! Handles two common OpenStack log formats:
//!
//! Plain format (nova-api, neutron, etc.):
//!   `YYYY-MM-DD HH:MM:SS.mmm PID LEVEL nova.component.name [-] message`
//!
//! Prefixed format (rotated log files):
//!   `nova-api.log.1.2017-05-16 YYYY-MM-DD HH:MM:SS.mmm PID LEVEL nova.comp.name [...] msg`
//!
//! Extraction:
//! - Timestamp prefix (shared date+hour) → `@TS` placeholder
//! - PIDs (field[2], 4-6 digit process IDs) → `@P{n}` placeholders
//! - Component module names (dotted, e.g. `nova.compute.manager`) → `@K{n}` placeholders
//!
//! This domain scores 0.88 on the plain format, superseding `log.syslog` (0.75 for OpenStack).
//! It scores 0.0 on the prefixed format so syslog.rs continues to handle that variant.

use crate::domain::{Domain, DomainInfo, ExtractionResult};
use cpac_types::{CpacError, CpacResult};
use std::collections::HashMap;

const MIN_FREQUENCY: usize = 2;
const MIN_MODULE_LEN: usize = 10;
const MIN_PID_LEN: usize = 4;
const MIN_USEFUL_SIZE: usize = 16_384; // 16 KB
const MAX_TOKENS: usize = 48;
const DYN_FREQ_RATIO: f64 = 0.005;
const MIN_SAVINGS_BYTES: usize = 256;

pub struct OpenStackLogDomain;

impl Domain for OpenStackLogDomain {
    fn info(&self) -> DomainInfo {
        DomainInfo {
            id: "log.openstack",
            name: "OpenStack Log",
            extensions: &[".log"],
            mime_types: &[],
            magic_bytes: &[],
        }
    }

    fn detect(&self, data: &[u8], _filename: Option<&str>) -> f64 {
        let text = std::str::from_utf8(data).unwrap_or("");

        let count = text
            .lines()
            .take(10)
            .filter(|line| is_openstack_plain_line(line))
            .count();

        if count > 6 {
            if data.len() >= MIN_USEFUL_SIZE {
                0.88
            } else {
                0.40
            }
        } else {
            0.0
        }
    }

    fn extract(&self, data: &[u8]) -> CpacResult<ExtractionResult> {
        let text = std::str::from_utf8(data)
            .map_err(|e| CpacError::CompressFailed(format!("OpenStack log decode: {e}")))?;
        extract_openstack(text)
    }

    fn extract_with_fields(
        &self,
        data: &[u8],
        fields: &HashMap<String, serde_json::Value>,
    ) -> CpacResult<ExtractionResult> {
        let text = std::str::from_utf8(data)
            .map_err(|e| CpacError::CompressFailed(format!("OpenStack log decode: {e}")))?;

        let pids = get_str_vec(fields, "pids").unwrap_or_default();
        let modules = get_str_vec(fields, "modules").unwrap_or_default();
        let ts_prefix: Option<&str> = fields.get("ts_prefix").and_then(|v| v.as_str());

        if pids.is_empty() && modules.is_empty() && ts_prefix.is_none() {
            return extract_openstack(text);
        }

        let compacted = compact_openstack(text, ts_prefix, &pids, &modules);

        Ok(ExtractionResult {
            fields: fields.clone(),
            residual: compacted.into_bytes(),
            metadata: HashMap::new(),
            domain_id: "log.openstack".to_string(),
        })
    }

    fn reconstruct(&self, result: &ExtractionResult) -> CpacResult<Vec<u8>> {
        let pids = get_str_vec(&result.fields, "pids").unwrap_or_default();
        let modules = get_str_vec(&result.fields, "modules").unwrap_or_default();
        let ts_prefix: Option<String> = result
            .fields
            .get("ts_prefix")
            .and_then(|v| v.as_str().map(String::from));

        let mut reconstructed = std::str::from_utf8(&result.residual)
            .map_err(|e| CpacError::DecompressFailed(format!("UTF-8 decode: {e}")))?
            .to_string();

        // Expand in reverse index order to avoid @K10 matching before @K1.
        for (idx, module) in modules.iter().enumerate().rev() {
            reconstructed = reconstructed.replace(&format!("@K{idx}"), module);
        }
        for (idx, pid) in pids.iter().enumerate().rev() {
            reconstructed = reconstructed.replace(&format!("@P{idx}"), pid);
        }
        // Expand timestamp prefix last (line-by-line).
        if let Some(pfx) = ts_prefix {
            let mut result_str = String::with_capacity(reconstructed.len());
            let trailing_nl = reconstructed.ends_with('\n');
            for line in reconstructed.lines() {
                if let Some(rest) = line.strip_prefix("@TS") {
                    result_str.push_str(&pfx);
                    result_str.push_str(rest);
                } else {
                    result_str.push_str(line);
                }
                result_str.push('\n');
            }
            if !trailing_nl && result_str.ends_with('\n') {
                result_str.pop();
            }
            reconstructed = result_str;
        }

        Ok(reconstructed.into_bytes())
    }
}

// ---------------------------------------------------------------------------
// Detection helper
// ---------------------------------------------------------------------------

/// Returns true when `line` is an OpenStack plain-format log line:
/// `YYYY-MM-DD HH:MM:SS.mmm PID LEVEL module.name ...`
fn is_openstack_plain_line(line: &str) -> bool {
    let parts: Vec<&str> = line.splitn(6, ' ').collect();
    if parts.len() < 5 {
        return false;
    }
    // field[0]: YYYY-MM-DD (10 chars, starts with digit, contains dashes)
    let date = parts[0].as_bytes();
    if date.len() != 10 || !date[0].is_ascii_digit() {
        return false;
    }
    if date[4] != b'-' || date[7] != b'-' {
        return false;
    }
    // field[1]: HH:MM:SS or HH:MM:SS.mmm (8+ chars, starts with digit, contains colon)
    let time = parts[1].as_bytes();
    if time.len() < 8 || !time[0].is_ascii_digit() || time[2] != b':' {
        return false;
    }
    // field[2]: PID (all digits)
    if !parts[2].bytes().all(|b| b.is_ascii_digit()) || parts[2].is_empty() {
        return false;
    }
    // field[3]: log level keyword
    matches!(
        parts[3],
        "INFO" | "WARNING" | "ERROR" | "DEBUG" | "TRACE" | "CRITICAL" | "AUDIT" | "WARN"
    )
}

// ---------------------------------------------------------------------------
// Extraction helpers
// ---------------------------------------------------------------------------

fn extract_openstack(text: &str) -> CpacResult<ExtractionResult> {
    let mut pid_freq: HashMap<String, usize> = HashMap::new();
    let mut module_freq: HashMap<String, usize> = HashMap::new();

    let mut ts_count = 0usize;
    let mut common_ts: Option<Vec<u8>> = None;

    for line in text.lines() {
        if !is_openstack_plain_line(line) {
            continue;
        }
        let parts: Vec<&str> = line.splitn(6, ' ').collect();
        if parts.len() < 5 {
            continue;
        }
        // Timestamp prefix: combine field[0] (date) + " " + field[1] (time) = "YYYY-MM-DD HH:MM:SS.mmm"
        let ts_str = format!("{} {}", parts[0], parts[1]);
        let ts_bytes = ts_str.as_bytes();
        ts_count += 1;
        common_ts = Some(match common_ts {
            None => ts_bytes.to_vec(),
            Some(prev) => {
                let lcp = prev
                    .iter()
                    .zip(ts_bytes.iter())
                    .take_while(|(a, b)| a == b)
                    .count();
                prev[..lcp].to_vec()
            }
        });
        // field[2]: PID
        let pid = parts[2];
        if pid.len() >= MIN_PID_LEN {
            *pid_freq.entry(pid.to_string()).or_insert(0) += 1;
        }
        // field[4]: module name (dotted, e.g. nova.compute.manager)
        if let Some(module) = parts.get(4) {
            if module.len() >= MIN_MODULE_LEN && module.contains('.') && !module.contains('/') {
                *module_freq.entry(module.to_string()).or_insert(0) += 1;
            }
        }
    }

    let line_count = text.lines().count().max(1);
    #[allow(clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let dyn_min_freq =
        ((line_count as f64 * DYN_FREQ_RATIO).round() as usize).max(MIN_FREQUENCY);

    // Timestamp prefix: trim to a clean separator boundary, require >= 7 chars.
    let ts_prefix: Option<String> = if ts_count >= dyn_min_freq {
        common_ts.and_then(|pfx| {
            if pfx.len() < 7 {
                return None;
            }
            let mut trim = pfx.len();
            while trim > 7 {
                let last = pfx[trim - 1];
                if last == b':' || last == b'-' || last == b' ' {
                    break;
                }
                trim -= 1;
            }
            if trim < 7 {
                return None;
            }
            String::from_utf8(pfx[..trim].to_vec()).ok()
        })
    } else {
        None
    };

    // PIDs: keep repeated values.
    let mut repeated_pids: Vec<(String, usize)> = pid_freq
        .into_iter()
        .filter(|(_, count)| *count >= dyn_min_freq)
        .collect();
    repeated_pids.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(b.1.cmp(&a.1)));
    repeated_pids.truncate(MAX_TOKENS);

    // Modules: keep repeated dotted names.  No MAX_FREQ_FRACTION cap — a single
    // OpenStack service emits the same module name on every line and that's fine.
    let mut repeated_modules: Vec<(String, usize)> = module_freq
        .into_iter()
        .filter(|(_, count)| *count >= dyn_min_freq)
        .collect();
    repeated_modules.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(b.1.cmp(&a.1)));
    repeated_modules.truncate(MAX_TOKENS);

    // Savings gate.
    #[allow(clippy::cast_precision_loss)]
    let ts_savings = ts_prefix
        .as_ref()
        .map(|p| p.len().saturating_sub(3) * ts_count)
        .unwrap_or(0);
    let pid_savings: usize = repeated_pids
        .iter()
        .map(|(t, c)| t.len().saturating_sub(3) * c)
        .sum();
    let module_savings: usize = repeated_modules
        .iter()
        .map(|(t, c)| t.len().saturating_sub(4) * c)
        .sum();

    if ts_savings + pid_savings + module_savings < MIN_SAVINGS_BYTES {
        let mut f = HashMap::new();
        f.insert("ts_prefix".to_string(), serde_json::Value::Null);
        f.insert("pids".to_string(), serde_json::Value::Array(vec![]));
        f.insert("modules".to_string(), serde_json::Value::Array(vec![]));
        return Ok(ExtractionResult {
            fields: f,
            residual: text.as_bytes().to_vec(),
            metadata: HashMap::new(),
            domain_id: "log.openstack".to_string(),
        });
    }

    let pid_names: Vec<String> = repeated_pids.iter().map(|(p, _)| p.clone()).collect();
    let module_names: Vec<String> = repeated_modules.iter().map(|(m, _)| m.clone()).collect();

    let ts_ref = ts_prefix.as_deref();
    let compacted = compact_openstack(text, ts_ref, &pid_names, &module_names);

    let mut fields = HashMap::new();
    fields.insert(
        "ts_prefix".to_string(),
        match ts_prefix {
            Some(ref p) => serde_json::Value::String(p.clone()),
            None => serde_json::Value::Null,
        },
    );
    fields.insert(
        "pids".to_string(),
        serde_json::Value::Array(
            pid_names
                .iter()
                .map(|p| serde_json::Value::String(p.clone()))
                .collect(),
        ),
    );
    fields.insert(
        "modules".to_string(),
        serde_json::Value::Array(
            module_names
                .iter()
                .map(|m| serde_json::Value::String(m.clone()))
                .collect(),
        ),
    );

    Ok(ExtractionResult {
        fields,
        residual: compacted.into_bytes(),
        metadata: HashMap::new(),
        domain_id: "log.openstack".to_string(),
    })
}

fn compact_openstack(
    text: &str,
    ts_prefix: Option<&str>,
    pids: &[String],
    modules: &[String],
) -> String {
    // Apply timestamp prefix replacement first (line-by-line at line start).
    let mut compacted = if let Some(pfx) = ts_prefix {
        let mut out = String::with_capacity(text.len());
        let trailing_nl = text.ends_with('\n');
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix(pfx) {
                out.push_str("@TS");
                out.push_str(rest);
            } else {
                out.push_str(line);
            }
            out.push('\n');
        }
        if !trailing_nl && out.ends_with('\n') {
            out.pop();
        }
        out
    } else {
        text.to_string()
    };

    // Apply module replacements before PIDs (modules are longer → fewer false hits).
    let mut all_tokens: Vec<(String, String)> = modules
        .iter()
        .enumerate()
        .map(|(i, m)| (m.clone(), format!("@K{i}")))
        .chain(
            pids.iter()
                .enumerate()
                .map(|(i, p)| (p.clone(), format!("@P{i}"))),
        )
        .collect();
    all_tokens.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
    for (orig, placeholder) in &all_tokens {
        compacted = compacted.replace(orig.as_str(), placeholder);
    }

    compacted
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

    const SAMPLE: &[u8] = b"2015-10-25 12:34:56.789 12345 INFO nova.compute.manager [-] Starting instance build\n\
2015-10-25 12:34:57.001 12345 DEBUG nova.compute.manager [-] Checking instance state\n\
2015-10-25 12:34:57.234 12346 WARNING nova.scheduler.manager [-] No host found\n\
2015-10-25 12:34:57.456 12345 INFO nova.compute.manager [-] Instance spawned successfully\n\
2015-10-25 12:34:57.678 12346 ERROR nova.scheduler.manager [-] Scheduler failed\n\
2015-10-25 12:34:57.890 12345 INFO nova.compute.manager [-] Sending update to conductor\n\
2015-10-25 12:34:58.012 12345 DEBUG nova.compute.manager [-] Cleanup complete\n";

    #[test]
    fn openstack_detect() {
        let domain = OpenStackLogDomain;
        let data: Vec<u8> = SAMPLE.iter().copied().cycle().take(20_000).collect();
        let conf = domain.detect(&data, None);
        assert!(conf >= 0.85, "OpenStack detection confidence={conf}");
    }

    #[test]
    fn openstack_roundtrip() {
        let domain = OpenStackLogDomain;
        let data: Vec<u8> = SAMPLE.iter().copied().cycle().take(20_000).collect();
        let result = domain.extract(&data).unwrap();
        let reconstructed = domain.reconstruct(&result).unwrap();
        assert_eq!(data.as_slice(), reconstructed.as_slice(), "OpenStack roundtrip mismatch");
    }

    #[test]
    fn openstack_extracts_modules_and_pids() {
        let domain = OpenStackLogDomain;
        let data: Vec<u8> = SAMPLE.iter().copied().cycle().take(20_000).collect();
        let result = domain.extract(&data).unwrap();
        let module_count = result.fields.get("modules")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        assert!(module_count > 0, "Expected modules to be extracted, got 0");
        let pid_count = result.fields.get("pids")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        assert!(pid_count > 0, "Expected PIDs to be extracted, got 0");
    }

    #[test]
    fn openstack_residual_smaller() {
        let domain = OpenStackLogDomain;
        let data: Vec<u8> = SAMPLE.iter().copied().cycle().take(20_000).collect();
        let result = domain.extract(&data).unwrap();
        assert!(
            result.residual.len() < data.len(),
            "Residual {} should be smaller than original {}",
            result.residual.len(),
            data.len()
        );
    }

    #[test]
    fn openstack_streaming_reuse() {
        let domain = OpenStackLogDomain;
        let block1: Vec<u8> = SAMPLE.iter().copied().cycle().take(20_000).collect();
        let result1 = domain.extract(&block1).unwrap();
        let block2 = b"2015-10-25 12:35:00.000 12345 INFO nova.compute.manager [-] New block\n\
2015-10-25 12:35:00.001 12346 WARNING nova.scheduler.manager [-] Retry\n";
        let result2 = domain
            .extract_with_fields(block2, &result1.fields)
            .unwrap();
        let reconstructed = domain.reconstruct(&result2).unwrap();
        assert_eq!(
            block2.as_slice(),
            reconstructed.as_slice(),
            "OpenStack streaming roundtrip mismatch"
        );
    }
}
