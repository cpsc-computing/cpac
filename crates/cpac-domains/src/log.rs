// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Log file domain handler.
//!
//! Detects structured log lines (ISO timestamps + level + message)
//! and decomposes into typed columns.

use cpac_types::{CpacError, CpacResult, CpacType, DomainHint};

use crate::DomainHandler;

const LOG_LEVELS: &[&str] = &[
    "TRACE", "DEBUG", "INFO", "WARN", "WARNING", "ERROR", "FATAL", "CRITICAL",
];

/// Detect log-style content.
#[must_use] 
pub fn detect_log(data: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(&data[..data.len().min(1024)]) else { return false; };
    let mut log_lines = 0;
    let mut total = 0;
    for line in text.lines().take(10) {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        total += 1;
        // Check for ISO-ish timestamp prefix or log level
        if (l.len() > 19 && l.as_bytes()[4] == b'-' && l.as_bytes()[10] == b'T')
            || LOG_LEVELS.iter().any(|lv| l.contains(lv))
        {
            log_lines += 1;
        }
    }
    total >= 3 && log_lines * 2 >= total
}

pub struct LogHandler;

impl DomainHandler for LogHandler {
    fn name(&self) -> &'static str {
        "log"
    }
    fn domain_hint(&self) -> DomainHint {
        DomainHint::Log
    }
    fn can_handle(&self, data: &[u8]) -> bool {
        detect_log(data)
    }
    fn decompose(&self, data: &[u8]) -> CpacResult<CpacType> {
        let text = std::str::from_utf8(data)
            .map_err(|e| CpacError::Other(format!("log: invalid UTF-8: {e}")))?;
        let mut timestamps = Vec::new();
        let mut levels = Vec::new();
        let mut messages = Vec::new();
        for line in text.lines() {
            let l = line.trim();
            if l.is_empty() {
                continue;
            }
            // Try to parse: <timestamp> <LEVEL> <message>
            let mut parts = l.splitn(3, ' ');
            let ts = parts.next().unwrap_or("").to_string();
            let rest = parts.next().unwrap_or("");
            let level = if LOG_LEVELS.iter().any(|lv| rest.eq_ignore_ascii_case(lv)) {
                rest.to_string()
            } else {
                String::new()
            };
            let msg = parts.next().unwrap_or("").to_string();
            timestamps.push(ts);
            levels.push(level);
            messages.push(msg);
        }
        let ts_bytes: usize = timestamps.iter().map(std::string::String::len).sum();
        let lv_bytes: usize = levels.iter().map(std::string::String::len).sum();
        let msg_bytes: usize = messages.iter().map(std::string::String::len).sum();
        Ok(CpacType::ColumnSet {
            columns: vec![
                (
                    "timestamp".into(),
                    CpacType::StringColumn {
                        values: timestamps,
                        total_bytes: ts_bytes,
                    },
                ),
                (
                    "level".into(),
                    CpacType::StringColumn {
                        values: levels,
                        total_bytes: lv_bytes,
                    },
                ),
                (
                    "message".into(),
                    CpacType::StringColumn {
                        values: messages,
                        total_bytes: msg_bytes,
                    },
                ),
            ],
        })
    }
    fn reconstruct(&self, columns: &CpacType) -> CpacResult<Vec<u8>> {
        let cols = match columns {
            CpacType::ColumnSet { columns } if columns.len() == 3 => columns,
            _ => return Err(CpacError::Other("log: expected 3-column ColumnSet".into())),
        };
        let CpacType::StringColumn { values: ts, .. } = &cols[0].1 else {
            return Err(CpacError::Other("bad col".into()));
        };
        let CpacType::StringColumn { values: lv, .. } = &cols[1].1 else {
            return Err(CpacError::Other("bad col".into()));
        };
        let CpacType::StringColumn { values: msg, .. } = &cols[2].1 else {
            return Err(CpacError::Other("bad col".into()));
        };
        let mut out = String::new();
        for i in 0..ts.len() {
            out.push_str(&ts[i]);
            if !lv[i].is_empty() {
                out.push(' ');
                out.push_str(&lv[i]);
            }
            if !msg[i].is_empty() {
                out.push(' ');
                out.push_str(&msg[i]);
            }
            out.push('\n');
        }
        Ok(out.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect() {
        let log = b"2026-03-01T12:00:00 INFO Starting\n2026-03-01T12:00:01 DEBUG Step 1\n2026-03-01T12:00:02 INFO Done\n";
        assert!(detect_log(log));
        assert!(!detect_log(b"not a log file at all"));
    }

    #[test]
    fn decompose_reconstruct() {
        let log =
            b"2026-03-01T12:00:00 INFO Starting server\n2026-03-01T12:00:01 DEBUG Loading config\n";
        let h = LogHandler;
        let cols = h.decompose(log).unwrap();
        let restored = h.reconstruct(&cols).unwrap();
        assert!(restored.windows(4).any(|w| w == b"INFO"));
    }
}
