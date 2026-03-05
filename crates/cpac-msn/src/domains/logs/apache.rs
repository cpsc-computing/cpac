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
        // Case-insensitive filename hint: high confidence for explicitly named access/error logs.
        if let Some(fname) = filename {
            let fname_lower = fname.to_ascii_lowercase();
            if fname_lower.contains("access") || fname_lower.contains("apache") {
                return 0.7;
            }
        }

        let text = std::str::from_utf8(data).unwrap_or("");

        // Apache error log pattern: "[Day Mon DD HH:MM:SS YYYY] [level] message"
        // e.g. "[Sun Dec 04 04:47:44 2005] [notice] workerEnv.init()..."
        const WEEKDAYS: &[&str] = &[
            "[Mon ", "[Tue ", "[Wed ", "[Thu ", "[Fri ", "[Sat ", "[Sun ",
        ];
        let error_log_count = text
            .lines()
            .take(10)
            .filter(|line| WEEKDAYS.iter().any(|d| line.starts_with(d)))
            .count();

        if error_log_count > 5 {
            return 0.85;
        }

        // Apache/NCSA access log: IP - - [timestamp] "METHOD /path HTTP/..."
        // Content-only confidence is intentionally low (0.4, below the default min_confidence of
        // 0.5) to prevent regression on large, diverse access logs (e.g. NASA server logs) where
        // IP/UA table overhead exceeds the savings from substitution.  Files with an explicit
        // 'access' or 'apache' filename already get 0.7 confidence from the branch above.
        let has_access_pattern = text
            .lines()
            .take(10)
            .filter(|line| line.contains("] \"") && line.contains("HTTP/"))
            .count()
            > 5;

        if has_access_pattern {
            return 0.4;
        }

        0.0
    }

    fn extract(&self, data: &[u8]) -> CpacResult<ExtractionResult> {
        let text = std::str::from_utf8(data)
            .map_err(|e| CpacError::CompressFailed(format!("Apache log decode: {e}")))?;

        if Self::is_error_log(text) {
            Self::extract_error_log(text)
        } else {
            Self::extract_access_log(text)
        }
    }

    fn reconstruct(&self, result: &ExtractionResult) -> CpacResult<Vec<u8>> {
        let format = result
            .fields
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("access");

        let mut reconstructed = std::str::from_utf8(&result.residual)
            .map_err(|e| CpacError::DecompressFailed(format!("UTF-8 decode: {e}")))?
            .to_string();

        if format == "error" {
            // Apache error log: expand level placeholders
            let levels = Self::get_str_vec(&result.fields, "levels")?;
            for (idx, level) in levels.iter().enumerate() {
                reconstructed = reconstructed.replace(&format!("@L{idx}"), level);
            }
        } else {
            // Apache access log: expand IP, method, user-agent placeholders
            let ips = Self::get_str_vec(&result.fields, "ips")?;
            let methods = Self::get_str_vec(&result.fields, "methods")?;
            let uas = Self::get_str_vec(&result.fields, "user_agents")?;
            for (idx, ip) in ips.iter().enumerate() {
                reconstructed = reconstructed.replace(&format!("@I{idx}"), ip);
            }
            for (idx, method) in methods.iter().enumerate() {
                reconstructed = reconstructed.replace(&format!("@M{idx}"), method);
            }
            for (idx, ua) in uas.iter().enumerate() {
                reconstructed = reconstructed.replace(&format!("@U{idx}"), ua);
            }
        }

        Ok(reconstructed.into_bytes())
    }
}

impl ApacheDomain {
    /// True when `text` looks like an Apache error log (`[Day Mon DD ...]` lines).
    fn is_error_log(text: &str) -> bool {
        const WEEKDAYS: &[&str] = &[
            "[Mon ", "[Tue ", "[Wed ", "[Thu ", "[Fri ", "[Sat ", "[Sun ",
        ];
        text.lines()
            .take(10)
            .filter(|line| WEEKDAYS.iter().any(|d| line.starts_with(d)))
            .count()
            > 5
    }

    /// Extract Apache error-log fields: log levels → `@L{n}` placeholders.
    fn extract_error_log(text: &str) -> CpacResult<ExtractionResult> {
        let mut level_freq: HashMap<String, usize> = HashMap::new();

        for line in text.lines() {
            // Log level is the second bracketed token: [...] [level] ...
            // Find the second '['
            let mut bracket_count = 0usize;
            let mut in_bracket = false;
            let mut level_start = 0usize;
            let mut level_end = 0usize;
            for (i, ch) in line.char_indices() {
                match ch {
                    '[' => {
                        bracket_count += 1;
                        if bracket_count == 2 {
                            in_bracket = true;
                            level_start = i;
                        }
                    }
                    ']' if in_bracket => {
                        level_end = i + 1;
                        break;
                    }
                    _ => {}
                }
            }
            if level_end > level_start {
                let level = &line[level_start..level_end];
                *level_freq.entry(level.to_string()).or_insert(0) += 1;
            }
        }

        let mut repeated_levels: Vec<(String, usize)> = level_freq
            .into_iter()
            .filter(|(_, count)| *count >= 2)
            .collect();
        repeated_levels.sort_by(|a, b| b.1.cmp(&a.1));

        let mut level_map: HashMap<String, String> = HashMap::new();
        for (idx, (level, _)) in repeated_levels.iter().enumerate() {
            level_map.insert(level.clone(), format!("@L{idx}"));
        }

        let mut compacted = text.to_string();
        for (orig, replacement) in &level_map {
            compacted = compacted.replace(orig, replacement);
        }

        let mut fields = HashMap::new();
        fields.insert(
            "format".to_string(),
            serde_json::Value::String("error".to_string()),
        );
        fields.insert(
            "levels".to_string(),
            serde_json::Value::Array(
                repeated_levels
                    .iter()
                    .map(|(l, _)| serde_json::Value::String(l.clone()))
                    .collect(),
            ),
        );

        Ok(ExtractionResult {
            fields,
            residual: compacted.into_bytes(),
            metadata: HashMap::new(),
            domain_id: "log.apache".to_string(),
        })
    }

    /// Extract Apache access-log fields: IPs, methods, user agents → placeholders.
    fn extract_access_log(text: &str) -> CpacResult<ExtractionResult> {
        let mut ip_freq: HashMap<String, usize> = HashMap::new();
        let mut ua_freq: HashMap<String, usize> = HashMap::new();
        let mut method_freq: HashMap<String, usize> = HashMap::new();

        for line in text.lines() {
            if let Some(ip) = line.split_whitespace().next() {
                *ip_freq.entry(ip.to_string()).or_insert(0) += 1;
            }
            if let Some(start) = line.find('"') {
                if let Some(end) = line[start + 1..].find('"') {
                    let request = &line[start + 1..start + 1 + end];
                    if let Some(method) = request.split_whitespace().next() {
                        *method_freq.entry(method.to_string()).or_insert(0) += 1;
                    }
                }
            }
            let quote_positions: Vec<_> = line.match_indices('"').collect();
            if quote_positions.len() >= 4 {
                let ua_start = quote_positions[quote_positions.len() - 2].0 + 1;
                let ua_end = quote_positions[quote_positions.len() - 1].0;
                if ua_end > ua_start {
                    let ua = &line[ua_start..ua_end];
                    *ua_freq.entry(ua.to_string()).or_insert(0) += 1;
                }
            }
        }

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

        let mut ip_map: HashMap<String, String> = HashMap::new();
        for (idx, (ip, _)) in repeated_ips.iter().enumerate() {
            ip_map.insert(ip.clone(), format!("@I{idx}"));
        }
        let mut method_map: HashMap<String, String> = HashMap::new();
        for (idx, (method, _)) in repeated_methods.iter().enumerate() {
            method_map.insert(method.clone(), format!("@M{idx}"));
        }
        let mut ua_map: HashMap<String, String> = HashMap::new();
        for (idx, (ua, _)) in repeated_uas.iter().enumerate() {
            ua_map.insert(ua.clone(), format!("@U{idx}"));
        }

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
        fields.insert(
            "format".to_string(),
            serde_json::Value::String("access".to_string()),
        );
        fields.insert(
            "ips".to_string(),
            serde_json::Value::Array(
                repeated_ips
                    .iter()
                    .map(|(i, _)| serde_json::Value::String(i.clone()))
                    .collect(),
            ),
        );
        fields.insert(
            "methods".to_string(),
            serde_json::Value::Array(
                repeated_methods
                    .iter()
                    .map(|(m, _)| serde_json::Value::String(m.clone()))
                    .collect(),
            ),
        );
        fields.insert(
            "user_agents".to_string(),
            serde_json::Value::Array(
                repeated_uas
                    .iter()
                    .map(|(u, _)| serde_json::Value::String(u.clone()))
                    .collect(),
            ),
        );

        Ok(ExtractionResult {
            fields,
            residual: compacted.into_bytes(),
            metadata: HashMap::new(),
            domain_id: "log.apache".to_string(),
        })
    }

    /// Helper: extract a `Vec<String>` from a named field (returns empty vec if missing/wrong type).
    fn get_str_vec(
        fields: &HashMap<String, serde_json::Value>,
        key: &str,
    ) -> CpacResult<Vec<String>> {
        match fields.get(key) {
            Some(serde_json::Value::Array(arr)) => Ok(arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()),
            None => Ok(Vec::new()), // field absent = nothing to expand
            _ => Err(CpacError::DecompressFailed(format!("Invalid {key} format"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apache_access_log_roundtrip() {
        let domain = ApacheDomain;
        let data = b"*********** - - [01/Jan/2021:12:00:00 +0000] \"GET /index.html HTTP/1.1\" 200 1234\n192.168.1.1 - - [01/Jan/2021:12:00:01 +0000] \"GET /about.html HTTP/1.1\" 200 5678";
        let result = domain.extract(data).unwrap();
        let reconstructed = domain.reconstruct(&result).unwrap();
        assert_eq!(data.as_slice(), reconstructed.as_slice());
    }

    #[test]
    fn apache_error_log_detect() {
        let domain = ApacheDomain;
        let data = b"[Sun Dec 04 04:47:44 2005] [notice] msg1\n\
[Sun Dec 04 04:47:45 2005] [error] msg2\n\
[Sun Dec 04 04:47:46 2005] [notice] msg3\n\
[Sun Dec 04 04:47:47 2005] [notice] msg4\n\
[Sun Dec 04 04:47:48 2005] [error] msg5\n\
[Sun Dec 04 04:47:49 2005] [notice] msg6\n\
[Mon Dec 05 04:47:50 2005] [notice] msg7\n";
        let confidence = domain.detect(data, None);
        assert!(confidence >= 0.8, "Apache error log confidence={confidence}");
    }

    #[test]
    fn apache_access_log_content_confidence_below_threshold() {
        // Access log detected via content only must return confidence < 0.5 (default min_confidence)
        // so MSN does not fire on large diverse access logs (e.g. NASA server logs).
        let domain = ApacheDomain;
        let data = b"199.72.81.55 - - [01/Jul/1995:00:00:01 -0400] \"GET /history/apollo/ HTTP/1.0\" 200 6245\n\
205.212.115.106 - - [01/Jul/1995:00:00:02 -0400] \"GET /shuttle/countdown/ HTTP/1.0\" 200 3985\n\
129.94.144.152 - - [01/Jul/1995:00:00:03 -0400] \"GET / HTTP/1.0\" 200 7074\n\
199.166.62.154 - - [01/Jul/1995:00:00:04 -0400] \"GET /images/NASA-logosmall.gif HTTP/1.0\" 200 786\n\
unicom016.unicom.net - - [01/Jul/1995:00:00:05 -0400] \"GET /shuttle/countdown/ HTTP/1.0\" 200 3985\n\
199.72.81.55 - - [01/Jul/1995:00:00:06 -0400] \"GET /images/NASA-logosmall.gif HTTP/1.0\" 200 786\n\
205.212.115.106 - - [01/Jul/1995:00:00:07 -0400] \"GET /images/KSC-logosmall.gif HTTP/1.0\" 200 1204\n";
        let confidence = domain.detect(data, None);
        assert!(
            confidence < 0.5,
            "access log content-only confidence should be < 0.5, got {confidence}"
        );
    }

    #[test]
    fn apache_access_log_filename_hint() {
        // Explicit 'access' in filename should trigger high confidence.
        let domain = ApacheDomain;
        let confidence = domain.detect(b"", Some("access_log"));
        assert!(confidence >= 0.7, "access_log filename confidence={confidence}");
        // Case-insensitive: 'Apache' should also match.
        let confidence2 = domain.detect(b"", Some("Apache_2k.log"));
        assert!(confidence2 >= 0.7, "Apache_ filename confidence={confidence2}");
    }

    #[test]
    fn apache_error_log_roundtrip() {
        let domain = ApacheDomain;
        let data = b"[Sun Dec 04 04:47:44 2005] [notice] workerEnv.init() ok\n\
[Sun Dec 04 04:47:44 2005] [error] mod_jk child in error state 6\n\
[Sun Dec 04 04:51:08 2005] [notice] jk2_init() Found child 6725\n\
[Mon Dec 05 04:47:44 2005] [notice] jk2_init() Found child 6726\n";
        let result = domain.extract(data).unwrap();
        // Should compact: [notice] and [error] replaced with placeholders
        assert!(
            result.residual.len() < data.len(),
            "error log should compress: residual={} vs input={}",
            result.residual.len(), data.len()
        );
        let reconstructed = domain.reconstruct(&result).unwrap();
        assert_eq!(data.as_slice(), reconstructed.as_slice());
    }
}
