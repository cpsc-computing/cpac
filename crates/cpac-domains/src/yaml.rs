// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! YAML domain handler.

use cpac_types::{CpacError, CpacResult, CpacType, DomainHint};

use crate::DomainHandler;

/// Detect YAML content (starts with `---` or `key: value` patterns).
#[must_use]
pub fn detect_yaml(data: &[u8]) -> bool {
    if data.len() < 4 {
        return false;
    }
    let Ok(text) = std::str::from_utf8(&data[..data.len().min(512)]) else {
        return false;
    };
    let trimmed = text.trim_start();
    if trimmed.starts_with("---") {
        return true;
    }
    // Check for key: value pattern on first non-empty lines
    let mut kv_lines = 0;
    for line in trimmed.lines().take(5) {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        if l.contains(": ") || l.ends_with(':') {
            kv_lines += 1;
        }
    }
    kv_lines >= 2
}

pub struct YamlHandler;

impl DomainHandler for YamlHandler {
    fn name(&self) -> &'static str {
        "yaml"
    }
    fn domain_hint(&self) -> DomainHint {
        DomainHint::Yaml
    }
    fn can_handle(&self, data: &[u8]) -> bool {
        detect_yaml(data)
    }
    fn decompose(&self, data: &[u8]) -> CpacResult<CpacType> {
        let text = std::str::from_utf8(data)
            .map_err(|e| CpacError::Other(format!("yaml: invalid UTF-8: {e}")))?;
        let mut keys = Vec::new();
        let mut values = Vec::new();
        for line in text.lines() {
            let l = line.trim();
            if l.is_empty() || l.starts_with('#') || l.starts_with("---") {
                continue;
            }
            if let Some((k, v)) = l.split_once(": ") {
                keys.push(k.trim().to_string());
                values.push(v.trim().to_string());
            } else if let Some(k) = l.strip_suffix(':') {
                keys.push(k.trim().to_string());
                values.push(String::new());
            }
        }
        let key_bytes: usize = keys.iter().map(std::string::String::len).sum();
        let val_bytes: usize = values.iter().map(std::string::String::len).sum();
        Ok(CpacType::ColumnSet {
            columns: vec![
                (
                    "keys".into(),
                    CpacType::StringColumn {
                        values: keys,
                        total_bytes: key_bytes,
                    },
                ),
                (
                    "values".into(),
                    CpacType::StringColumn {
                        values,
                        total_bytes: val_bytes,
                    },
                ),
            ],
        })
    }
    fn reconstruct(&self, columns: &CpacType) -> CpacResult<Vec<u8>> {
        let (keys, values) = match columns {
            CpacType::ColumnSet { columns } if columns.len() == 2 => {
                let CpacType::StringColumn { values: k, .. } = &columns[0].1 else {
                    return Err(CpacError::Other("yaml: expected StringColumn".into()));
                };
                let CpacType::StringColumn { values: v, .. } = &columns[1].1 else {
                    return Err(CpacError::Other("yaml: expected StringColumn".into()));
                };
                (k, v)
            }
            _ => return Err(CpacError::Other("yaml: expected 2-column ColumnSet".into())),
        };
        let mut out = String::from("---\n");
        for (k, v) in keys.iter().zip(values.iter()) {
            if v.is_empty() {
                out.push_str(k);
                out.push_str(":\n");
            } else {
                out.push_str(k);
                out.push_str(": ");
                out.push_str(v);
                out.push('\n');
            }
        }
        Ok(out.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect() {
        assert!(detect_yaml(b"---\nname: test\n"));
        assert!(detect_yaml(b"name: test\nage: 30\n"));
        assert!(!detect_yaml(b"just some text"));
    }

    #[test]
    fn decompose_reconstruct() {
        let data = b"name: Alice\nage: 30\ncity: NYC\n";
        let h = YamlHandler;
        let cols = h.decompose(data).unwrap();
        let restored = h.reconstruct(&cols).unwrap();
        assert!(restored.starts_with(b"---\n"));
        assert!(restored.windows(5).any(|w| w == b"Alice"));
    }
}
