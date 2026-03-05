// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Syslog domain handler (RFC 5424 and BSD syslog).

use crate::domain::{Domain, DomainInfo, ExtractionResult};
use cpac_types::{CpacError, CpacResult};
use std::collections::HashMap;

/// True if `s` is a 3-letter month abbreviation used in BSD syslog headers.
#[inline]
fn is_bsd_month(s: &str) -> bool {
    matches!(
        s,
        "Jan" | "Feb" | "Mar" | "Apr" | "May" | "Jun"
            | "Jul" | "Aug" | "Sep" | "Oct" | "Nov" | "Dec"
    )
}

/// Strip trailing `:` and optional PID bracket `[digits]` from a BSD syslog app-name token.
///
/// Examples:
/// - `"sshd(pam_unix)[19939]:"` -> `"sshd(pam_unix)"`
/// - `"sshd[24200]:"` -> `"sshd"`
/// - `"kernel:"` -> `"kernel"`
/// - `"app[notnum]:"` -> `"app[notnum]"` (non-digit content, not a PID)
#[inline]
fn strip_pid_suffix(app: &str) -> &str {
    // Trim trailing ':' then look for a trailing "[digits]" pattern.
    let s = app.trim_end_matches(':');
    if s.ends_with(']') {
        if let Some(open) = s.rfind('[') {
            let inner = &s[open + 1..s.len() - 1]; // content between '[' and ']'
            if !inner.is_empty() && inner.bytes().all(|b| b.is_ascii_digit()) {
                return &s[..open];
            }
        }
    }
    s
}

/// Syslog domain handler.
///
/// Supports RFC 5424 (`<PRI>VERSION TIMESTAMP HOSTNAME APP-NAME ...`) and
/// BSD syslog (`Mon DD HH:MM:SS hostname app[pid]: msg`).
/// Extracts repeated hostnames and app names to reduce redundancy.
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
        // Minimum size below which metadata overhead typically exceeds savings.
        const MIN_USEFUL_SIZE: usize = 65536; // 64 KB

        if let Some(fname) = filename {
            if fname.contains("syslog")
                || std::path::Path::new(fname)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("log"))
            {
                return 0.6;
            }
        }

        let text = std::str::from_utf8(data).unwrap_or("");

        // RFC 5424 priority pattern: <NUMBER>
        let has_priority = text
            .lines()
            .take(10)
            .filter(|line| {
                line.starts_with('<') && line.chars().nth(1).is_some_and(|c| c.is_ascii_digit())
            })
            .count()
            > 5;
        if has_priority {
            return 0.9;
        }

        // BSD syslog pattern: "Mon DD HH:MM:SS hostname app: msg"
        // e.g. "Jun 14 15:16:01 combo sshd[19939]: ..."
        const BSD_MONTHS: &[&str] = &[
            "Jan ", "Feb ", "Mar ", "Apr ", "May ", "Jun ",
            "Jul ", "Aug ", "Sep ", "Oct ", "Nov ", "Dec ",
        ];
        let bsd_count = text
            .lines()
            .take(10)
            .filter(|line| BSD_MONTHS.iter().any(|m| line.starts_with(m)))
            .count();
        if bsd_count > 5 {
            // Size gate: very small BSD syslog blocks have too little repetition for MSN benefit.
            return if data.len() >= MIN_USEFUL_SIZE { 0.85 } else { 0.4 };
        }

        // RFC 5424 ISO timestamp: contains 'T', ':', '-'
        let has_timestamp = text
            .lines()
            .take(10)
            .filter(|line| line.contains('T') && line.contains(':') && line.contains('-'))
            .count()
            > 5;
        if has_timestamp {
            return 0.6;
        }

        const LOG_LEVELS: &[&str] = &[" INFO ", " ERROR ", " WARNING ", " DEBUG ", " CRITICAL "];

        // OpenStack structured log: field[0] is a log filename prefix (contains ".log") +
        // ISO date + log level. MSN extracts the repeated filename prefix token for meaningful
        // savings (~34 bytes/line × line_count).
        let openstack_count = text
            .lines()
            .take(10)
            .filter(|line| {
                let first = line.splitn(2, ' ').next().unwrap_or("");
                first.contains(".log") && LOG_LEVELS.iter().any(|lvl| line.contains(lvl))
            })
            .count();
        if openstack_count > 5 {
            return if data.len() >= MIN_USEFUL_SIZE { 0.75 } else { 0.4 };
        }

        // Generic structured log (Hadoop, BGL, HPC, etc.): ISO date + log level, but without a
        // repeated filename prefix. Zstd handles these natively; MSN adds metadata overhead
        // without proportional savings. Keep confidence below min_confidence (0.5) to avoid
        // regression (e.g. Hadoop_2k: -0.57x without this gate).
        let structured_count = text
            .lines()
            .take(10)
            .filter(|line| {
                line.contains('-')
                    && line.contains(':')
                    && LOG_LEVELS.iter().any(|lvl| line.contains(lvl))
            })
            .count();
        if structured_count > 5 {
            return 0.45; // below default min_confidence of 0.5 → MSN passthrough
        }

        0.0
    }

    fn extract(&self, data: &[u8]) -> CpacResult<ExtractionResult> {
        let text = std::str::from_utf8(data)
            .map_err(|e| CpacError::CompressFailed(format!("Syslog decode: {e}")))?;

        let mut hostname_freq: HashMap<String, usize> = HashMap::new();
        let mut appname_freq: HashMap<String, usize> = HashMap::new();
        // Prefix tokens for OpenStack-style structured logs.
        // e.g. "nova-api.log.1.2017-05-16_13:53:08" repeated on every line.
        let mut prefix_freq: HashMap<String, usize> = HashMap::new();

        for line in text.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 5 {
                // Detect BSD syslog by checking whether field[0] is a month abbreviation.
                // BSD:       "Mon DD HH:MM:SS hostname app[pid]: msg"
                // OpenStack: "log-prefix.log.N.date ISO-date ISO-time PID LEVEL module msg"
                // RFC5424:   "<PRI>VERSION TIMESTAMP HOSTNAME APP-NAME ..."
                let is_bsd = is_bsd_month(parts[0]);
                let is_openstack = parts[0].contains(".log");

                if is_openstack {
                    // Extract the log filename prefix (field[0]): large repeated token.
                    *prefix_freq.entry(parts[0].to_string()).or_insert(0) += 1;
                    // Log level is field[4] (e.g. INFO/WARNING/ERROR).
                    if let Some(level) = parts.get(4) {
                        *appname_freq.entry(level.to_string()).or_insert(0) += 1;
                    }
                } else {
                    // Regular syslog: hostname=field[3], appname=field[4].
                    if let Some(hostname) = parts.get(3) {
                        *hostname_freq.entry(hostname.to_string()).or_insert(0) += 1;
                    }
                    // App-name is field[4].
                    // For BSD, strip the PID bracket so "sshd[19939]:" and "sshd[19937]:"
                    // both map to "sshd", preventing a metadata explosion from unique PIDs.
                    if let Some(appname) = parts.get(4) {
                        let key = if is_bsd {
                            strip_pid_suffix(appname)
                        } else {
                            appname
                        };
                        *appname_freq.entry(key.to_string()).or_insert(0) += 1;
                    }
                }
            }
        }

        // Only keep values that appear at least twice (worth storing in metadata)
        let mut repeated_prefixes: Vec<(String, usize)> = prefix_freq
            .into_iter()
            .filter(|(_, count)| *count >= 2)
            .collect();
        repeated_prefixes.sort_by(|a, b| b.1.cmp(&a.1));

        let mut repeated_hostnames: Vec<(String, usize)> = hostname_freq
            .into_iter()
            .filter(|(_, count)| *count >= 2)
            .collect();
        repeated_hostnames.sort_by(|a, b| b.1.cmp(&a.1));

        let mut repeated_appnames: Vec<(String, usize)> = appname_freq
            .into_iter()
            .filter(|(_, count)| *count >= 2)
            .collect();
        repeated_appnames.sort_by(|a, b| b.1.cmp(&a.1));

        // Build replacement maps and compact the log
        let mut prefix_map: HashMap<String, String> = HashMap::new();
        for (idx, (prefix, _)) in repeated_prefixes.iter().enumerate() {
            prefix_map.insert(prefix.clone(), format!("@P{idx}"));
        }

        let mut hostname_map: HashMap<String, String> = HashMap::new();
        for (idx, (hostname, _)) in repeated_hostnames.iter().enumerate() {
            hostname_map.insert(hostname.clone(), format!("@H{idx}"));
        }

        let mut appname_map: HashMap<String, String> = HashMap::new();
        for (idx, (appname, _)) in repeated_appnames.iter().enumerate() {
            appname_map.insert(appname.clone(), format!("@A{idx}"));
        }

        let mut compacted = text.to_string();
        // Apply prefix substitutions before hostname/appname to avoid partial clobbering.
        for (orig, replacement) in &prefix_map {
            compacted = compacted.replace(orig, replacement);
        }
        for (orig, replacement) in &hostname_map {
            compacted = compacted.replace(orig, replacement);
        }
        for (orig, replacement) in &appname_map {
            compacted = compacted.replace(orig, replacement);
        }

        let mut fields = HashMap::new();
        fields.insert(
            "prefixes".to_string(),
            serde_json::Value::Array(
                repeated_prefixes
                    .iter()
                    .map(|(p, _)| serde_json::Value::String(p.clone()))
                    .collect(),
            ),
        );
        fields.insert(
            "hostnames".to_string(),
            serde_json::Value::Array(
                repeated_hostnames
                    .iter()
                    .map(|(h, _)| serde_json::Value::String(h.clone()))
                    .collect(),
            ),
        );
        fields.insert(
            "appnames".to_string(),
            serde_json::Value::Array(
                repeated_appnames
                    .iter()
                    .map(|(a, _)| serde_json::Value::String(a.clone()))
                    .collect(),
            ),
        );

        Ok(ExtractionResult {
            fields,
            residual: compacted.into_bytes(),
            metadata: HashMap::new(),
            domain_id: "log.syslog".to_string(),
        })
    }

    fn reconstruct(&self, result: &ExtractionResult) -> CpacResult<Vec<u8>> {
        let hostnames_value = result
            .fields
            .get("hostnames")
            .ok_or_else(|| CpacError::DecompressFailed("Missing hostnames".into()))?;
        let appnames_value = result
            .fields
            .get("appnames")
            .ok_or_else(|| CpacError::DecompressFailed("Missing appnames".into()))?;

        let hostnames: Vec<String> = if let serde_json::Value::Array(arr) = hostnames_value {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        } else {
            return Err(CpacError::DecompressFailed(
                "Invalid hostnames format".into(),
            ));
        };

        let appnames: Vec<String> = if let serde_json::Value::Array(arr) = appnames_value {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        } else {
            return Err(CpacError::DecompressFailed(
                "Invalid appnames format".into(),
            ));
        };

        // Prefixes are optional (only present in OpenStack-style structured logs).
        let prefixes: Vec<String> = result
            .fields
            .get("prefixes")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        let mut reconstructed = std::str::from_utf8(&result.residual)
            .map_err(|e| CpacError::DecompressFailed(format!("UTF-8 decode: {e}")))?
            .to_string();

        // Expand placeholders in descending index order to avoid partial matches
        // (e.g. "@P10" being partially replaced by an "@P1" rule).
        for (idx, prefix) in prefixes.iter().enumerate().rev() {
            reconstructed = reconstructed.replace(&format!("@P{idx}"), prefix);
        }
        for (idx, hostname) in hostnames.iter().enumerate().rev() {
            reconstructed = reconstructed.replace(&format!("@H{idx}"), hostname);
        }
        for (idx, appname) in appnames.iter().enumerate().rev() {
            reconstructed = reconstructed.replace(&format!("@A{idx}"), appname);
        }

        Ok(reconstructed.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syslog_rfc5424_roundtrip() {
        let domain = SyslogDomain;
        let data =
            b"<134>1 2021-03-01T12:00:00Z server1 app1 - - msg1\n\
<134>1 2021-03-01T12:00:01Z server1 app1 - - msg2";
        let result = domain.extract(data).unwrap();
        let reconstructed = domain.reconstruct(&result).unwrap();
        assert_eq!(data.as_slice(), reconstructed.as_slice());
    }

    #[test]
    fn syslog_bsd_detect() {
        let domain = SyslogDomain;
        // Build a sample > 64 KB (MIN_USEFUL_SIZE) so the size gate yields full confidence.
        let line = b"Jun 14 15:16:01 host sshd[1234]: authentication failure\n";
        let data: Vec<u8> = line.iter().copied().cycle().take(70_000).collect();
        let confidence = domain.detect(&data, None);
        assert!(confidence >= 0.8, "BSD syslog large-block confidence={confidence}");
    }

    #[test]
    fn syslog_bsd_detect_small_sample_size_gated() {
        // Small samples (< 64 KB) should be size-gated to confidence 0.4 so MSN
        // doesn't fire where metadata overhead would exceed savings.
        let domain = SyslogDomain;
        let data = b"Jun 14 15:16:01 host sshd[1]: msg\n\
Jun 14 15:16:02 host sshd[2]: msg\n\
Jun 14 15:16:03 host sshd[3]: msg\n\
Jun 14 15:16:04 host sshd[4]: msg\n\
Jun 14 15:16:05 host sshd[5]: msg\n\
Jun 14 15:16:06 host sshd[6]: msg\n\
Jun 14 15:16:07 host sshd[7]: msg\n";
        let confidence = domain.detect(data, None);
        assert_eq!(confidence, 0.4, "small BSD syslog should be size-gated to 0.4");
    }

    #[test]
    fn syslog_bsd_roundtrip() {
        let domain = SyslogDomain;
        // Varying PIDs: all map to base appname "sshd" after PID stripping.
        let data = b"Jun 14 15:16:01 combo sshd[1]: auth failure\n\
Jun 14 15:16:02 combo sshd[2]: invalid user\n\
Jun 14 15:16:03 combo sshd[3]: auth failure\n";
        let result = domain.extract(data).unwrap();
        let reconstructed = domain.reconstruct(&result).unwrap();
        assert_eq!(data.as_slice(), reconstructed.as_slice());
    }

    #[test]
    fn strip_pid_suffix_cases() {
        assert_eq!(strip_pid_suffix("sshd(pam_unix)[19939]:"), "sshd(pam_unix)");
        assert_eq!(strip_pid_suffix("sshd[24200]:"), "sshd");
        assert_eq!(strip_pid_suffix("kernel:"), "kernel");
        assert_eq!(strip_pid_suffix("app[notnum]:"), "app[notnum]");
    }

    #[test]
    fn openstack_detect() {
        let domain = SyslogDomain;
        let data = b"nova-api.log.1.2017-05-16_13:53:08 2017-05-16 00:00:00.008 25746 INFO nova.osapi: req 1\n\
nova-api.log.1.2017-05-16_13:53:08 2017-05-16 00:00:00.009 25746 INFO nova.osapi: req 2\n\
nova-api.log.1.2017-05-16_13:53:08 2017-05-16 00:00:00.010 25746 WARNING nova.osapi: slow\n\
nova-api.log.1.2017-05-16_13:53:08 2017-05-16 00:00:00.011 25746 INFO nova.osapi: req 3\n\
nova-api.log.1.2017-05-16_13:53:08 2017-05-16 00:00:00.012 25746 INFO nova.osapi: req 4\n\
nova-api.log.1.2017-05-16_13:53:08 2017-05-16 00:00:00.013 25746 INFO nova.osapi: req 5\n\
nova-api.log.1.2017-05-16_13:53:08 2017-05-16 00:00:00.014 25746 INFO nova.osapi: req 6\n";
        // Data is > 64 KB? No, it's a small test sample. Size gate applies.
        // Confidence should reflect size gate (< 64KB → 0.4, which is below default min_conf).
        // We test the raw detect() score here.
        let confidence = domain.detect(data, None);
        // Small sample → 0.4 (size-gated). Large sample would be 0.75.
        assert!(
            confidence == 0.4 || confidence == 0.75,
            "OpenStack confidence={confidence} (expected 0.4 for small sample or 0.75 for large)"
        );
    }

    #[test]
    fn openstack_roundtrip() {
        let domain = SyslogDomain;
        // Build a sample > 64 KB so size gate doesn't suppress detection.
        let line = b"nova-api.log.1.2017-05-16_13:53:08 2017-05-16 00:00:00.008 25746 INFO nova.osapi_compute.wsgi.server [-] req-abc\n";
        let data: Vec<u8> = line.iter().copied().cycle().take(70_000).collect();
        // Trim to a clean line boundary.
        let end = data.iter().rposition(|&b| b == b'\n').map(|p| p + 1).unwrap_or(data.len());
        let data = &data[..end];

        let confidence = domain.detect(data, None);
        assert!(confidence >= 0.75, "OpenStack large block confidence={confidence}");

        let result = domain.extract(data).unwrap();
        // Prefix should have been extracted — residual must be shorter.
        assert!(
            result.residual.len() < data.len(),
            "OpenStack prefix extraction should reduce residual: residual={} vs input={}",
            result.residual.len(),
            data.len()
        );
        let reconstructed = domain.reconstruct(&result).unwrap();
        assert_eq!(data, reconstructed.as_slice(), "OpenStack roundtrip mismatch");
    }

    #[test]
    fn generic_structured_log_confidence_below_threshold() {
        // Hadoop-style logs (ISO date + log level, no .log prefix) must NOT trigger MSN
        // (confidence < 0.5) to avoid the -0.57x regression observed on Hadoop_2k.log.
        let domain = SyslogDomain;
        let data = b"2015-10-17 15:37:28,955 INFO org.apache.hadoop.mapred.MapTask: Processing split\n\
2015-10-17 15:37:29,001 INFO org.apache.hadoop.mapred.MapTask: Map output\n\
2015-10-17 15:37:29,100 WARNING org.apache.hadoop.ipc.Server: Error processing\n\
2015-10-17 15:37:29,200 INFO org.apache.hadoop.mapred.MapTask: Done\n\
2015-10-17 15:37:29,300 INFO org.apache.hadoop.mapred.ReduceTask: Reducing\n\
2015-10-17 15:37:29,400 INFO org.apache.hadoop.mapred.MapTask: Processing split\n\
2015-10-17 15:37:29,500 ERROR org.apache.hadoop.ipc.Server: Connection reset\n";
        let confidence = domain.detect(data, None);
        assert!(
            confidence < 0.5,
            "Hadoop-style log confidence should be < 0.5 to avoid MSN regression, got {confidence}"
        );
    }
}
