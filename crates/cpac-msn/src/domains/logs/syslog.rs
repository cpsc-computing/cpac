// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Syslog domain handler (RFC 5424).

use crate::domain::{Domain, DomainInfo, ExtractionResult};
use cpac_types::{CpacError, CpacResult};
use std::collections::HashMap;

/// Syslog domain handler.
///
/// Extracts repeated hostnames, app names, and structured data keys.
/// Target compression: 20-30x on syslog data.
pub struct SyslogDomain;

impl Domain for SyslogDomain {
    fn info(&self) -> DomainInfo {
        DomainInfo {
            id: "log.syslog",
            name: "Syslog",
            extensions: &[".log", ".syslog"],
            mime_types: &["text/plain"],
            magic_bytes: &[],
        }
    }

    fn detect(&self, data: &[u8], filename: Option<&str>) -> f64 {
        if let Some(fname) = filename {
            if fname.contains("syslog") || fname.ends_with(".log") {
                return 0.6;
            }
        }

        let text = std::str::from_utf8(data).unwrap_or("");
        
        // Look for RFC 5424 priority pattern: <NUMBER>
        let has_priority = text.lines().take(10).filter(|line| {
            line.starts_with('<') && line.chars().nth(1).map_or(false, |c| c.is_ascii_digit())
        }).count() > 5;

        if has_priority {
            return 0.8;
        }

        // Look for common syslog patterns
        let has_timestamp = text.lines().take(10).filter(|line| {
            line.contains("T") && line.contains(":") && line.contains("-")
        }).count() > 5;

        if has_timestamp {
            return 0.5;
        }

        0.0
    }

    fn extract(&self, data: &[u8]) -> CpacResult<ExtractionResult> {
        let text = std::str::from_utf8(data)
            .map_err(|e| CpacError::CompressFailed(format!("Syslog decode: {}", e)))?;

        let mut hostname_freq: HashMap<String, usize> = HashMap::new();
        let mut appname_freq: HashMap<String, usize> = HashMap::new();

        for line in text.lines() {
            // Parse RFC 5424: <PRI>VERSION TIMESTAMP HOSTNAME APP-NAME ...
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 5 {
                // Extract hostname (usually 4th field)
                if let Some(hostname) = parts.get(3) {
                    *hostname_freq.entry(hostname.to_string()).or_insert(0) += 1;
                }
                // Extract app-name (usually 5th field)
                if let Some(appname) = parts.get(4) {
                    *appname_freq.entry(appname.to_string()).or_insert(0) += 1;
                }
            }
        }

        // Extract repeated hostnames
        let mut repeated_hostnames: Vec<(String, usize)> = hostname_freq
            .into_iter()
            .filter(|(_, count)| *count >= 2)
            .collect();
        repeated_hostnames.sort_by(|a, b| b.1.cmp(&a.1));

        // Extract repeated app names
        let mut repeated_appnames: Vec<(String, usize)> = appname_freq
            .into_iter()
            .filter(|(_, count)| *count >= 2)
            .collect();
        repeated_appnames.sort_by(|a, b| b.1.cmp(&a.1));

        // Build replacement maps
        let mut hostname_map: HashMap<String, String> = HashMap::new();
        for (idx, (hostname, _)) in repeated_hostnames.iter().enumerate() {
            hostname_map.insert(hostname.clone(), format!("@H{}", idx));
        }

        let mut appname_map: HashMap<String, String> = HashMap::new();
        for (idx, (appname, _)) in repeated_appnames.iter().enumerate() {
            appname_map.insert(appname.clone(), format!("@A{}", idx));
        }

        // Compact log by replacing repeated values
        let mut compacted = text.to_string();
        for (orig, replacement) in &hostname_map {
            compacted = compacted.replace(orig, replacement);
        }
        for (orig, replacement) in &appname_map {
            compacted = compacted.replace(orig, replacement);
        }

        let mut fields = HashMap::new();
        fields.insert("hostnames".to_string(), serde_json::Value::Array(
            repeated_hostnames.iter().map(|(h, _)| serde_json::Value::String(h.clone())).collect()
        ));
        fields.insert("appnames".to_string(), serde_json::Value::Array(
            repeated_appnames.iter().map(|(a, _)| serde_json::Value::String(a.clone())).collect()
        ));

        Ok(ExtractionResult {
            fields,
            residual: compacted.into_bytes(),
            metadata: HashMap::new(),
            domain_id: "log.syslog".to_string(),
        })
    }

    fn reconstruct(&self, result: &ExtractionResult) -> CpacResult<Vec<u8>> {
        let hostnames_value = result.fields.get("hostnames")
            .ok_or_else(|| CpacError::DecompressFailed("Missing hostnames".into()))?;
        let appnames_value = result.fields.get("appnames")
            .ok_or_else(|| CpacError::DecompressFailed("Missing appnames".into()))?;

        let hostnames: Vec<String> = if let serde_json::Value::Array(arr) = hostnames_value {
            arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
        } else {
            return Err(CpacError::DecompressFailed("Invalid hostnames format".into()));
        };

        let appnames: Vec<String> = if let serde_json::Value::Array(arr) = appnames_value {
            arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
        } else {
            return Err(CpacError::DecompressFailed("Invalid appnames format".into()));
        };

        let mut reconstructed = std::str::from_utf8(&result.residual)
            .map_err(|e| CpacError::DecompressFailed(format!("UTF-8 decode: {}", e)))?
            .to_string();

        // Expand placeholders
        for (idx, hostname) in hostnames.iter().enumerate() {
            reconstructed = reconstructed.replace(&format!("@H{}", idx), hostname);
        }
        for (idx, appname) in appnames.iter().enumerate() {
            reconstructed = reconstructed.replace(&format!("@A{}", idx), appname);
        }

        Ok(reconstructed.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syslog_domain_roundtrip() {
        let domain = SyslogDomain;
        let data = b"<134>1 2021-03-01T12:00:00Z server1 app1 - - msg1\n<134>1 2021-03-01T12:00:01Z server1 app1 - - msg2";

        let result = domain.extract(data).unwrap();
        let reconstructed = domain.reconstruct(&result).unwrap();

        assert_eq!(data.as_slice(), reconstructed.as_slice());
    }
}
