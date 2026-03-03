// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Domain registry for auto-detection and management.

use crate::domain::Domain;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Registry for MSN domain handlers.
///
/// Manages domain registration and provides auto-detection functionality.
pub struct DomainRegistry {
    domains: RwLock<HashMap<String, Arc<dyn Domain>>>,
}

impl DomainRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            domains: RwLock::new(HashMap::new()),
        }
    }

    /// Register a domain handler.
    ///
    /// The domain's ID (from DomainInfo) is used as the key.
    pub fn register(&self, domain: Arc<dyn Domain>) {
        let info = domain.info();
        let mut domains = self.domains.write().unwrap();
        domains.insert(info.id.to_string(), domain);
    }

    /// Get a domain handler by ID.
    pub fn get(&self, domain_id: &str) -> Option<Arc<dyn Domain>> {
        let domains = self.domains.read().unwrap();
        domains.get(domain_id).cloned()
    }

    /// Auto-detect the best domain for given data.
    ///
    /// Returns the domain with the highest confidence score above the threshold.
    pub fn auto_detect(
        &self,
        data: &[u8],
        filename: Option<&str>,
        min_confidence: f64,
    ) -> Option<(Arc<dyn Domain>, f64)> {
        let domains = self.domains.read().unwrap();
        
        let mut best_domain: Option<Arc<dyn Domain>> = None;
        let mut best_score = min_confidence;

        for domain in domains.values() {
            let score = domain.detect(data, filename);
            if score > best_score {
                best_score = score;
                best_domain = Some(Arc::clone(domain));
            }
        }

        best_domain.map(|d| (d, best_score))
    }

    /// List all registered domain IDs.
    pub fn list_domains(&self) -> Vec<String> {
        let domains = self.domains.read().unwrap();
        domains.keys().cloned().collect()
    }

    /// Get the number of registered domains.
    pub fn count(&self) -> usize {
        let domains = self.domains.read().unwrap();
        domains.len()
    }
}

impl Default for DomainRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Global domain registry singleton.
static GLOBAL_REGISTRY: once_cell::sync::Lazy<DomainRegistry> =
    once_cell::sync::Lazy::new(|| {
        let registry = DomainRegistry::new();
        
        // Register all domain handlers
        registry.register(Arc::new(crate::domains::PassthroughDomain));
        registry.register(Arc::new(crate::domains::JsonDomain));
        registry.register(Arc::new(crate::domains::CsvDomain));
        registry.register(Arc::new(crate::domains::XmlDomain));
        registry.register(Arc::new(crate::domains::MsgPackDomain));
        registry.register(Arc::new(crate::domains::CborDomain));
        registry.register(Arc::new(crate::domains::ProtobufDomain));
        registry.register(Arc::new(crate::domains::SyslogDomain));
        registry.register(Arc::new(crate::domains::ApacheDomain));
        registry.register(Arc::new(crate::domains::JsonLogDomain));
        
        registry
    });

/// Get the global domain registry.
pub fn global_registry() -> &'static DomainRegistry {
    &GLOBAL_REGISTRY
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Domain, DomainInfo, ExtractionResult};
    use cpac_types::CpacResult;
    use std::collections::HashMap;

    struct TestDomain;

    impl Domain for TestDomain {
        fn info(&self) -> DomainInfo {
            DomainInfo {
                id: "test.mock",
                name: "Test Domain",
                extensions: &[".test"],
                mime_types: &["application/test"],
                magic_bytes: &[b"TEST"],
            }
        }

        fn detect(&self, data: &[u8], _filename: Option<&str>) -> f64 {
            if data.starts_with(b"TEST") {
                0.9
            } else {
                0.1
            }
        }

        fn extract(&self, data: &[u8]) -> CpacResult<ExtractionResult> {
            Ok(ExtractionResult {
                fields: HashMap::new(),
                residual: data.to_vec(),
                metadata: HashMap::new(),
                domain_id: "test.mock".to_string(),
            })
        }

        fn reconstruct(&self, result: &ExtractionResult) -> CpacResult<Vec<u8>> {
            Ok(result.residual.clone())
        }
    }

    #[test]
    fn registry_register_and_get() {
        let registry = DomainRegistry::new();
        let domain: Arc<dyn Domain> = Arc::new(TestDomain);
        
        registry.register(Arc::clone(&domain));
        
        let retrieved = registry.get("test.mock");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().info().id, "test.mock");
    }

    #[test]
    fn registry_auto_detect() {
        let registry = DomainRegistry::new();
        let domain: Arc<dyn Domain> = Arc::new(TestDomain);
        
        registry.register(domain);
        
        // Should detect with high confidence
        let result = registry.auto_detect(b"TEST data", None, 0.5);
        assert!(result.is_some());
        let (detected, score) = result.unwrap();
        assert_eq!(detected.info().id, "test.mock");
        assert!(score >= 0.9);
        
        // Should not detect with low confidence data
        let result = registry.auto_detect(b"other data", None, 0.5);
        assert!(result.is_none());
    }

    #[test]
    fn registry_list_domains() {
        let registry = DomainRegistry::new();
        assert_eq!(registry.count(), 0);
        
        let domain: Arc<dyn Domain> = Arc::new(TestDomain);
        registry.register(domain);
        assert_eq!(registry.count(), 1);
        
        let domains = registry.list_domains();
        assert!(domains.contains(&"test.mock".to_string()));
    }
}
