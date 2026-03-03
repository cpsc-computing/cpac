// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Apache Common/Combined log format handler.

use crate::domain::{Domain, DomainInfo, ExtractionResult};
use cpac_types::{CpacError, CpacResult};
use std::collections::HashMap;

/// Apache log domain handler.
///
/// Extracts repeated IPs, user agents, and referrers.
/// Target compression: 25-40x on Apache logs.
pub struct ApacheDomain;

impl Domain for ApacheDomain {
    fn info(&self) -> DomainInfo {
        DomainInfo {
            id: "log.apache",
            name: "Apache",
            extensions: &[".log", ".access"],
            mime_types: &["text/plain"],
            magic_bytes: &[],
        }
    }

    fn detect(&self, data: &[u8], filename: Option<&str>) -> f64 {
        if let Some(fname) = filename {
            if fname.contains("access") || fname.contains("apache") {
                return 0.7;
            }
        }

        let text = std::str::from_utf8(data).unwrap_or("");
        
        // Look for Apache log patterns: IP - - [timestamp] "METHOD /path HTTP/..."
        let has_apache_pattern = text.lines().take(10).filter(|line| {
            line.contains('[') && line.contains(']') && line.contains("HTTP/") && line.contains('"')
        }).count() > 5;

        if has_apache_pattern {
            return 0.8;
        }

        0.0
    }

    fn extract(&self, data: &[u8]) -> CpacResult<ExtractionResult> {
        let text = std::str::from_utf8(data)
            .map_err(|e| CpacError::CompressFailed(format!("Apache log decode: {}", e)))?;

        let mut ip_freq: HashMap<String, usize> = HashMap::new();
        let mut ua_freq: HashMap<String, usize> = HashMap::new();
        let mut method_freq: HashMap<String, usize> = HashMap::new();

        for line in text.lines() {
            // Parse IP (first field)
            if let Some(ip) = line.split_whitespace().next() {
                *ip_freq.entry(ip.to_string()).or_insert(0) += 1;
            }

            // Parse method (in quotes)
            if let Some(start) = line.find('"') {
                if let Some(end) = line[start+1..].find('"') {
                    let request = &line[start+1..start+1+end];
                    if let Some(method) = request.split_whitespace().next() {
                        *method_freq.entry(method.to_string()).or_insert(0) += 1;
                    }
                }
            }

            // Parse user agent (last quoted string in Combined format)
            let quote_positions: Vec<_> = line.match_indices('"').collect();
            if quote_positions.len() >= 4 {
                let ua_start = quote_positions[quote_positions.len()-2].0 + 1;
                let ua_end = quote_positions[quote_positions.len()-1].0;
                if ua_end > ua_start {
                    let ua = &line[ua_start..ua_end];
                    *ua_freq.entry(ua.to_string()).or_insert(0) += 1;
                }
            }
        }

        // Extract repeated values
        let mut repeated_ips: Vec<(String, usize)> = ip_freq
            .into_iter()
            .filter(|(_, count)| *count >= 2)
            .collect();
        repeated_ips.sort_by(|a, b| b.1.cmp(&a.1));

        let mut repeated_methods: Vec<(String, usize)> = method_freq
            .into_iter()
            .filter(|(_, count)| *count >= 2)
            .collect();
        repeated_methods.sort_by(|a, b| b.1.cmp(&a.1));

        let mut repeated_uas: Vec<(String, usize)> = ua_freq
            .into_iter()
            .filter(|(_, count)| *count >= 2)
            .collect();
        repeated_uas.sort_by(|a, b| b.1.cmp(&a.1));

        // Build replacement maps
        let mut ip_map: HashMap<String, String> = HashMap::new();
        for (idx, (ip, _)) in repeated_ips.iter().enumerate() {
            ip_map.insert(ip.clone(), format!("@I{}", idx));
        }

        let mut method_map: HashMap<String, String> = HashMap::new();
        for (idx, (method, _)) in repeated_methods.iter().enumerate() {
            method_map.insert(method.clone(), format!("@M{}", idx));
        }

        let mut ua_map: HashMap<String, String> = HashMap::new();
        for (idx, (ua, _)) in repeated_uas.iter().enumerate() {
            ua_map.insert(ua.clone(), format!("@U{}", idx));
        }

        // Compact log
        let mut compacted = text.to_string();
        for (orig, replacement) in &ip_map {
            compacted = compacted.replace(orig, replacement);
        }
        for (orig, replacement) in &method_map {
            compacted = compacted.replace(orig, replacement);
        }
        for (orig, replacement) in &ua_map {
            compacted = compacted.replace(orig, replacement);
        }

        let mut fields = HashMap::new();
        fields.insert("ips".to_string(), serde_json::Value::Array(
            repeated_ips.iter().map(|(i, _)| serde_json::Value::String(i.clone())).collect()
        ));
        fields.insert("methods".to_string(), serde_json::Value::Array(
            repeated_methods.iter().map(|(m, _)| serde_json::Value::String(m.clone())).collect()
        ));
        fields.insert("user_agents".to_string(), serde_json::Value::Array(
            repeated_uas.iter().map(|(u, _)| serde_json::Value::String(u.clone())).collect()
        ));

        Ok(ExtractionResult {
            fields,
            residual: compacted.into_bytes(),
            metadata: HashMap::new(),
            domain_id: "log.apache".to_string(),
        })
    }

    fn reconstruct(&self, result: &ExtractionResult) -> CpacResult<Vec<u8>> {
        let ips_value = result.fields.get("ips")
            .ok_or_else(|| CpacError::DecompressFailed("Missing ips".into()))?;
        let methods_value = result.fields.get("methods")
            .ok_or_else(|| CpacError::DecompressFailed("Missing methods".into()))?;
        let uas_value = result.fields.get("user_agents")
            .ok_or_else(|| CpacError::DecompressFailed("Missing user_agents".into()))?;

        let ips: Vec<String> = if let serde_json::Value::Array(arr) = ips_value {
            arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
        } else {
            return Err(CpacError::DecompressFailed("Invalid ips format".into()));
        };

        let methods: Vec<String> = if let serde_json::Value::Array(arr) = methods_value {
            arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
        } else {
            return Err(CpacError::DecompressFailed("Invalid methods format".into()));
        };

        let uas: Vec<String> = if let serde_json::Value::Array(arr) = uas_value {
            arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
        } else {
            return Err(CpacError::DecompressFailed("Invalid user_agents format".into()));
        };

        let mut reconstructed = std::str::from_utf8(&result.residual)
            .map_err(|e| CpacError::DecompressFailed(format!("UTF-8 decode: {}", e)))?
            .to_string();

        // Expand placeholders
        for (idx, ip) in ips.iter().enumerate() {
            reconstructed = reconstructed.replace(&format!("@I{}", idx), ip);
        }
        for (idx, method) in methods.iter().enumerate() {
            reconstructed = reconstructed.replace(&format!("@M{}", idx), method);
        }
        for (idx, ua) in uas.iter().enumerate() {
            reconstructed = reconstructed.replace(&format!("@U{}", idx), ua);
        }

        Ok(reconstructed.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apache_domain_roundtrip() {
        let domain = ApacheDomain;
        let data = b"192.168.1.1 - - [01/Jan/2021:12:00:00 +0000] \"GET /index.html HTTP/1.1\" 200 1234\n192.168.1.1 - - [01/Jan/2021:12:00:01 +0000] \"GET /about.html HTTP/1.1\" 200 5678";

        let result = domain.extract(data).unwrap();
        let reconstructed = domain.reconstruct(&result).unwrap();

        assert_eq!(data.as_slice(), reconstructed.as_slice());
    }
}
