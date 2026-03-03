// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Transform registry — maps transform IDs and names to `TransformNode` instances.

use std::collections::HashMap;
use std::sync::Arc;

use cpac_transforms::TransformNode;

/// Registry holding all available transforms.
#[derive(Clone)]
pub struct TransformRegistry {
    by_id: HashMap<u8, Arc<dyn TransformNode>>,
    by_name: HashMap<String, Arc<dyn TransformNode>>,
}

impl TransformRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            by_id: HashMap::new(),
            by_name: HashMap::new(),
        }
    }

    /// Create a registry pre-loaded with all built-in transforms.
    pub fn with_builtins() -> Self {
        let mut reg = Self::new();
        reg.register(Arc::new(cpac_transforms::DeltaTransform));
        reg.register(Arc::new(cpac_transforms::ZigzagTransform));
        reg.register(Arc::new(cpac_transforms::TransposeTransform));
        reg.register(Arc::new(cpac_transforms::RolzTransform));
        reg.register(Arc::new(cpac_transforms::FloatSplitTransform));
        reg.register(Arc::new(cpac_transforms::FieldLzTransform));
        reg.register(Arc::new(cpac_transforms::RangePackTransform));
        reg.register(Arc::new(cpac_transforms::TokenizeTransform));
        reg.register(Arc::new(cpac_transforms::PrefixTransform));
        reg.register(Arc::new(cpac_transforms::DedupTransform));
        reg.register(Arc::new(cpac_transforms::ParseIntTransform));
        reg
    }

    /// Register a transform.
    pub fn register(&mut self, node: Arc<dyn TransformNode>) {
        self.by_id.insert(node.id(), Arc::clone(&node));
        self.by_name
            .insert(node.name().to_string(), Arc::clone(&node));
    }

    /// Look up a transform by wire ID.
    pub fn get_by_id(&self, id: u8) -> Option<&Arc<dyn TransformNode>> {
        self.by_id.get(&id)
    }

    /// Look up a transform by name.
    pub fn get_by_name(&self, name: &str) -> Option<&Arc<dyn TransformNode>> {
        self.by_name.get(name)
    }

    /// List all registered transform names.
    pub fn names(&self) -> Vec<&str> {
        self.by_name.keys().map(|s| s.as_str()).collect()
    }

    /// Number of registered transforms.
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

impl Default for TransformRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_loaded() {
        let reg = TransformRegistry::with_builtins();
        assert_eq!(reg.len(), 11);
        assert!(reg.get_by_name("delta").is_some());
        assert!(reg.get_by_name("zigzag").is_some());
        assert!(reg.get_by_name("transpose").is_some());
        assert!(reg.get_by_name("rolz").is_some());
        assert!(reg.get_by_name("float_split").is_some());
        assert!(reg.get_by_name("field_lz").is_some());
        assert!(reg.get_by_name("range_pack").is_some());
        assert!(reg.get_by_name("tokenize").is_some());
        assert!(reg.get_by_name("prefix").is_some());
        assert!(reg.get_by_name("dedup").is_some());
        assert!(reg.get_by_name("parse_int").is_some());
    }

    #[test]
    fn lookup_by_id() {
        let reg = TransformRegistry::with_builtins();
        let delta = reg.get_by_id(cpac_transforms::delta::TRANSFORM_ID);
        assert!(delta.is_some());
        assert_eq!(delta.unwrap().name(), "delta");
    }
}
