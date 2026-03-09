// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Configuration file support for CPAC CLI.
//!
//! Lookup chain: `./cpac.toml` → parent dirs → `~/.config/cpac/config.toml`.

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Top-level configuration from `cpac.toml`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct CpacConfig {
    pub backend: Option<String>,
    pub block_size: Option<usize>,
    pub preset: Option<String>,
    pub encrypt: Option<EncryptConfig>,
    pub archive: Option<ArchiveConfig>,
    pub resources: Option<ResourcesConfig>,
}

/// Datacenter resource knobs from `[resources]` TOML section.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct ResourcesConfig {
    pub cpu_percent: Option<u8>,
    pub memory_percent: Option<u8>,
    pub budget_ms: Option<u64>,
    pub priority: Option<String>,
    pub io_bandwidth_mbps: Option<u32>,
    pub batch_concurrency: Option<usize>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct EncryptConfig {
    pub algorithm: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct ArchiveConfig {
    pub compression: Option<String>,
}

impl CpacConfig {
    /// Load configuration from the lookup chain, returning the first found.
    pub fn load() -> Self {
        Self::from_lookup_chain().unwrap_or_default()
    }

    fn from_lookup_chain() -> Option<Self> {
        // Walk from cwd upward looking for cpac.toml
        if let Ok(cwd) = std::env::current_dir() {
            let mut dir: Option<&Path> = Some(cwd.as_path());
            while let Some(d) = dir {
                let candidate = d.join("cpac.toml");
                if candidate.is_file() {
                    return Self::from_file(&candidate);
                }
                dir = d.parent();
            }
        }
        // Fall back to ~/.config/cpac/config.toml
        if let Some(home) = home_dir() {
            let global = home.join(".config").join("cpac").join("config.toml");
            if global.is_file() {
                return Self::from_file(&global);
            }
        }
        None
    }

    fn from_file(path: &Path) -> Option<Self> {
        let content = std::fs::read_to_string(path).ok()?;
        toml::from_str(&content).ok()
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let cfg = CpacConfig::default();
        assert!(cfg.backend.is_none());
        assert!(cfg.block_size.is_none());
    }

    #[test]
    fn parse_toml_config() {
        let toml_str = r#"
backend = "zstd"
block_size = 65536

[encrypt]
algorithm = "chacha20"

[archive]
compression = "zstd"
"#;
        let cfg: CpacConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.backend.as_deref(), Some("zstd"));
        assert_eq!(cfg.block_size, Some(65536));
        assert_eq!(cfg.encrypt.unwrap().algorithm.as_deref(), Some("chacha20"));
        assert_eq!(cfg.archive.unwrap().compression.as_deref(), Some("zstd"));
    }
}
