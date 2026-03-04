// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Profile cache — named transform profiles for different data types.

use std::collections::HashMap;
use std::path::Path;

use cpac_types::{CpacError, CpacResult};
use serde::{Deserialize, Serialize};

use crate::dag::TransformDAG;
use crate::registry::TransformRegistry;

/// A named compression profile specifying a transform chain.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Profile {
    /// Profile name (e.g. "generic", "csv", "json").
    pub name: String,
    /// Ordered list of transform names in the chain.
    pub transforms: Vec<String>,
    /// Description of what this profile is optimized for.
    pub description: String,
}

/// Cache of named profiles that can compile into DAGs.
pub struct ProfileCache {
    profiles: HashMap<String, Profile>,
    registry: TransformRegistry,
}

impl ProfileCache {
    /// Create a new cache with the given registry.
    #[must_use] 
    pub fn new(registry: TransformRegistry) -> Self {
        Self {
            profiles: HashMap::new(),
            registry,
        }
    }

    /// Create a cache pre-loaded with built-in profiles.
    #[must_use] 
    pub fn with_builtins() -> Self {
        let registry = TransformRegistry::with_builtins();
        let mut cache = Self::new(registry);

        // Generic profile: SSR-guided preprocess (handled by engine, DAG is passthrough)
        cache.add_profile(Profile {
            name: "generic".into(),
            transforms: vec![],
            description: "Generic — SSR-guided preprocessing with no fixed chain".into(),
        });

        // CSV numeric: parse_int → delta → zigzag → range_pack
        cache.add_profile(Profile {
            name: "csv_numeric".into(),
            transforms: vec!["parse_int".into()],
            description: "CSV numeric columns: text→int conversion".into(),
        });

        // CSV string: prefix → tokenize
        cache.add_profile(Profile {
            name: "csv_string".into(),
            transforms: vec![],
            description: "CSV string columns: prefix extraction + tokenization".into(),
        });

        // Binary structured: transpose
        cache.add_profile(Profile {
            name: "binary_struct".into(),
            transforms: vec![],
            description: "Binary structured data: column transposition".into(),
        });

        // Float data: float_split
        cache.add_profile(Profile {
            name: "float".into(),
            transforms: vec![],
            description: "Float arrays: exponent/mantissa splitting".into(),
        });

        cache
    }

    /// Add a profile to the cache.
    pub fn add_profile(&mut self, profile: Profile) {
        self.profiles.insert(profile.name.clone(), profile);
    }

    /// Get a profile by name.
    #[must_use] 
    pub fn get_profile(&self, name: &str) -> Option<&Profile> {
        self.profiles.get(name)
    }

    /// Compile a profile into a DAG.
    pub fn compile(&self, profile_name: &str) -> CpacResult<TransformDAG> {
        let profile = self
            .profiles
            .get(profile_name)
            .ok_or_else(|| CpacError::Transform(format!("unknown profile: {profile_name}")))?;
        if profile.transforms.is_empty() {
            return Ok(TransformDAG::passthrough());
        }
        let names: Vec<&str> = profile.transforms.iter().map(std::string::String::as_str).collect();
        TransformDAG::compile(&self.registry, &names)
    }

    /// List all profile names.
    #[must_use] 
    pub fn profile_names(&self) -> Vec<&str> {
        self.profiles.keys().map(std::string::String::as_str).collect()
    }

    /// Get the underlying registry.
    #[must_use] 
    pub fn registry(&self) -> &TransformRegistry {
        &self.registry
    }

    /// Save all profiles to a directory as `MessagePack` files.
    pub fn save_to_dir(&self, dir: &Path) -> CpacResult<()> {
        std::fs::create_dir_all(dir).map_err(|e| CpacError::Other(format!("create dir: {e}")))?;
        for (name, profile) in &self.profiles {
            let data = rmp_serde::to_vec(profile)
                .map_err(|e| CpacError::Other(format!("serialize {name}: {e}")))?;
            let path = dir.join(format!("{name}.mpk"));
            std::fs::write(&path, &data)
                .map_err(|e| CpacError::Other(format!("write {}: {e}", path.display())))?;
        }
        Ok(())
    }

    /// Load profiles from a directory of `MessagePack` files.
    pub fn load_from_dir(&mut self, dir: &Path) -> CpacResult<usize> {
        let rd = std::fs::read_dir(dir).map_err(|e| CpacError::Other(format!("read dir: {e}")))?;
        let mut count = 0;
        for entry in rd.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "mpk") {
                let data = std::fs::read(&path)
                    .map_err(|e| CpacError::Other(format!("read {}: {e}", path.display())))?;
                let profile: Profile = rmp_serde::from_slice(&data).map_err(|e| {
                    CpacError::Other(format!("deserialize {}: {e}", path.display()))
                })?;
                self.profiles.insert(profile.name.clone(), profile);
                count += 1;
            }
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_loaded() {
        let cache = ProfileCache::with_builtins();
        assert!(cache.get_profile("generic").is_some());
        assert!(cache.get_profile("csv_numeric").is_some());
        assert!(cache.get_profile("csv_string").is_some());
        assert!(cache.get_profile("binary_struct").is_some());
        assert!(cache.get_profile("float").is_some());
    }

    #[test]
    fn compile_generic_is_passthrough() {
        let cache = ProfileCache::with_builtins();
        let dag = cache.compile("generic").unwrap();
        assert!(dag.is_empty());
    }

    #[test]
    fn compile_csv_numeric() {
        let cache = ProfileCache::with_builtins();
        let dag = cache.compile("csv_numeric").unwrap();
        assert_eq!(dag.len(), 1);
        assert_eq!(dag.transform_names(), vec!["parse_int"]);
    }

    #[test]
    fn compile_unknown_fails() {
        let cache = ProfileCache::with_builtins();
        assert!(cache.compile("nonexistent").is_err());
    }

    #[test]
    fn save_load_roundtrip() {
        let cache = ProfileCache::with_builtins();
        let dir = tempfile::tempdir().unwrap();
        cache.save_to_dir(dir.path()).unwrap();

        let mut cache2 = ProfileCache::new(crate::registry::TransformRegistry::with_builtins());
        let loaded = cache2.load_from_dir(dir.path()).unwrap();
        assert!(loaded >= 5); // at least the 5 builtins
        assert!(cache2.get_profile("generic").is_some());
        assert!(cache2.get_profile("csv_numeric").is_some());
    }
}
