// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Domain-aware parsers for CPAC.
//!
//! Detects CSV, JSON, and XML formats and decomposes them into
//! typed column sets for optimized compression.

pub mod csv;
pub mod json;
pub mod log;
pub mod xml;
pub mod yaml;

use cpac_types::{CpacResult, CpacType, DomainHint};

/// Trait for domain-specific data handlers.
pub trait DomainHandler: Send + Sync {
    /// Human-readable name.
    fn name(&self) -> &str;

    /// Domain hint this handler covers.
    fn domain_hint(&self) -> DomainHint;

    /// Check if this handler can process the given data.
    fn can_handle(&self, data: &[u8]) -> bool;

    /// Decompose raw data into a `ColumnSet` for optimized compression.
    fn decompose(&self, data: &[u8]) -> CpacResult<CpacType>;

    /// Reconstruct raw data from a `ColumnSet`.
    fn reconstruct(&self, columns: &CpacType) -> CpacResult<Vec<u8>>;
}

/// Detect the domain of the given data.
#[must_use] 
pub fn detect_domain(data: &[u8]) -> Option<DomainHint> {
    if data.is_empty() {
        return None;
    }
    // Check first non-whitespace byte
    let first_non_ws = data.iter().find(|b| !b.is_ascii_whitespace());
    match first_non_ws {
        Some(b'{' | b'[') => Some(DomainHint::Json),
        Some(b'<') => {
            // Could be XML or HTML
            let start = String::from_utf8_lossy(&data[..data.len().min(200)]);
            if start.contains("<?xml") || start.contains("<root") || start.contains("xmlns") {
                Some(DomainHint::Xml)
            } else {
                None
            }
        }
        _ => {
            // Check for YAML, log, CSV in order
            if yaml::detect_yaml(data) {
                Some(DomainHint::Yaml)
            } else if log::detect_log(data) {
                Some(DomainHint::Log)
            } else if csv::detect_csv(data) {
                Some(DomainHint::Csv)
            } else {
                Some(DomainHint::Binary)
            }
        }
    }
}

/// Registry of all built-in domain handlers.
#[must_use] 
pub fn builtin_handlers() -> Vec<Box<dyn DomainHandler>> {
    vec![
        Box::new(csv::CsvHandler),
        Box::new(json::JsonHandler),
        Box::new(xml::XmlHandler),
        Box::new(yaml::YamlHandler),
        Box::new(log::LogHandler),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_json() {
        assert_eq!(detect_domain(b"{\"key\": 1}"), Some(DomainHint::Json));
        assert_eq!(detect_domain(b"[1, 2, 3]"), Some(DomainHint::Json));
    }

    #[test]
    fn detect_csv() {
        assert_eq!(
            detect_domain(b"name,age,city\nAlice,30,NYC\nBob,25,LA"),
            Some(DomainHint::Csv)
        );
    }

    #[test]
    fn detect_xml() {
        assert_eq!(
            detect_domain(b"<?xml version=\"1.0\"?><root/>"),
            Some(DomainHint::Xml)
        );
    }

    #[test]
    fn detect_binary() {
        assert_eq!(
            detect_domain(&[0x00, 0x01, 0x02, 0x03]),
            Some(DomainHint::Binary)
        );
    }
}
