// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Dictionary training and management for CPAC.
//!
//! Provides Zstd dictionary generation from corpus files, storage format,
//! and integration with compression pipeline.

#![allow(clippy::cast_possible_truncation, clippy::missing_errors_doc, clippy::missing_panics_doc)]

use cpac_types::{CpacError, CpacResult};
use std::path::Path;

/// Default maximum dictionary size: 128 KB.
pub const DEFAULT_MAX_DICT_SIZE: usize = 128 * 1024;

/// Dictionary metadata.
#[derive(Debug, Clone)]
pub struct DictionaryMetadata {
    /// Dictionary ID (XXH64 hash of dictionary content).
    pub dict_id: u64,
    /// Dictionary size in bytes.
    pub size: usize,
    /// Number of samples used for training.
    pub samples: usize,
    /// Total size of training corpus.
    pub corpus_size: usize,
    /// Creation timestamp (Unix epoch seconds).
    pub created_at: u64,
}

/// CPAC dictionary format with metadata.
#[derive(Debug, Clone)]
pub struct CpacDictionary {
    /// Dictionary metadata.
    pub metadata: DictionaryMetadata,
    /// Raw dictionary data (zstd format).
    pub data: Vec<u8>,
}

impl CpacDictionary {
    /// Train a dictionary from corpus files.
    ///
    /// # Arguments
    /// * `samples` - List of sample data (each sample is a file's content)
    /// * `max_size` - Maximum dictionary size in bytes
    ///
    /// # Errors
    /// Returns error if training fails.
    pub fn train(samples: &[Vec<u8>], max_size: usize) -> CpacResult<Self> {
        if samples.is_empty() {
            return Err(CpacError::Other("no samples provided for training".into()));
        }

        let corpus_size: usize = samples.iter().map(std::vec::Vec::len).sum();

        // Convert samples to slice of slices for zstd
        let sample_refs: Vec<&[u8]> = samples.iter().map(std::vec::Vec::as_slice).collect();

        // Train dictionary using zstd
        let dict_data = zstd::dict::from_samples(&sample_refs, max_size)
            .map_err(|e| CpacError::Other(format!("dictionary training failed: {e}")))?;

        // Compute dict_id (XXH64 hash)
        let dict_id = xxhash_rust::xxh64::xxh64(&dict_data, 0);

        let metadata = DictionaryMetadata {
            dict_id,
            size: dict_data.len(),
            samples: samples.len(),
            corpus_size,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        Ok(Self {
            metadata,
            data: dict_data,
        })
    }

    /// Train dictionary from files in a directory.
    ///
    /// # Errors
    /// Returns error if files cannot be read or training fails.
    pub fn train_from_directory(dir: &Path, max_size: usize) -> CpacResult<Self> {
        let mut samples = Vec::new();
        for entry in std::fs::read_dir(dir)
            .map_err(|e| CpacError::IoError(format!("cannot read directory: {e}")))?
        {
            let entry = entry.map_err(|e| CpacError::IoError(format!("dir entry error: {e}")))?;
            let path = entry.path();
            if path.is_file() {
                let data = std::fs::read(&path)
                    .map_err(|e| CpacError::IoError(format!("read {}: {e}", path.display())))?;
                samples.push(data);
            }
        }
        Self::train(&samples, max_size)
    }

    /// Serialize dictionary to binary format.
    ///
    /// Format: `[magic:4][version:1][dict_id:8][size:4][samples:4][corpus_size:8][created_at:8][data]`
    #[must_use] 
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(37 + self.data.len());
        bytes.extend_from_slice(b"CPDI"); // Magic: CPAC Dictionary
        bytes.push(1); // Version
        bytes.extend_from_slice(&self.metadata.dict_id.to_le_bytes());
        bytes.extend_from_slice(&(self.metadata.size as u32).to_le_bytes());
        bytes.extend_from_slice(&(self.metadata.samples as u32).to_le_bytes());
        bytes.extend_from_slice(&(self.metadata.corpus_size as u64).to_le_bytes());
        bytes.extend_from_slice(&self.metadata.created_at.to_le_bytes());
        bytes.extend_from_slice(&self.data);
        bytes
    }

    /// Deserialize dictionary from binary format.
    ///
    /// # Errors
    /// Returns error if format is invalid.
    pub fn from_bytes(bytes: &[u8]) -> CpacResult<Self> {
        if bytes.len() < 37 {
            return Err(CpacError::InvalidFrame("dictionary too short".into()));
        }
        if &bytes[0..4] != b"CPDI" {
            return Err(CpacError::InvalidFrame("invalid dictionary magic".into()));
        }
        if bytes[4] != 1 {
            return Err(CpacError::InvalidFrame(
                "unsupported dictionary version".into(),
            ));
        }

        let dict_id = u64::from_le_bytes([
            bytes[5], bytes[6], bytes[7], bytes[8], bytes[9], bytes[10], bytes[11], bytes[12],
        ]);
        let size = u32::from_le_bytes([bytes[13], bytes[14], bytes[15], bytes[16]]) as usize;
        let samples = u32::from_le_bytes([bytes[17], bytes[18], bytes[19], bytes[20]]) as usize;
        let corpus_size = u64::from_le_bytes([
            bytes[21], bytes[22], bytes[23], bytes[24], bytes[25], bytes[26], bytes[27], bytes[28],
        ]) as usize;
        let created_at = u64::from_le_bytes([
            bytes[29], bytes[30], bytes[31], bytes[32], bytes[33], bytes[34], bytes[35], bytes[36],
        ]);

        let data = bytes[37..].to_vec();
        if data.len() != size {
            return Err(CpacError::InvalidFrame(format!(
                "dictionary size mismatch: expected {size}, got {}",
                data.len()
            )));
        }

        Ok(Self {
            metadata: DictionaryMetadata {
                dict_id,
                size,
                samples,
                corpus_size,
                created_at,
            },
            data,
        })
    }

    /// Save dictionary to file (`.cpac-dict` extension recommended).
    ///
    /// # Errors
    /// Returns error if write fails.
    pub fn save_to_file(&self, path: &Path) -> CpacResult<()> {
        let bytes = self.to_bytes();
        std::fs::write(path, bytes)
            .map_err(|e| CpacError::IoError(format!("write {}: {e}", path.display())))
    }

    /// Load dictionary from file.
    ///
    /// # Errors
    /// Returns error if read or parse fails.
    pub fn load_from_file(path: &Path) -> CpacResult<Self> {
        let bytes = std::fs::read(path)
            .map_err(|e| CpacError::IoError(format!("read {}: {e}", path.display())))?;
        Self::from_bytes(&bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // Requires large corpus data
    fn train_simple_dictionary() {
        let samples = vec![
            b"hello world hello world ".repeat(500),
            b"hello rust hello rust ".repeat(500),
            b"hello cpac hello cpac ".repeat(500),
        ];
        let dict = CpacDictionary::train(&samples, DEFAULT_MAX_DICT_SIZE).unwrap();
        assert!(dict.metadata.size > 0);
        assert!(dict.metadata.size <= DEFAULT_MAX_DICT_SIZE);
        assert_eq!(dict.metadata.samples, 3);
        assert!(dict.metadata.dict_id != 0);
    }

    #[test]
    fn dictionary_serialization_roundtrip() {
        // Create a mock dictionary without training
        let dict = CpacDictionary {
            metadata: DictionaryMetadata {
                dict_id: 12345,
                size: 100,
                samples: 5,
                corpus_size: 5000,
                created_at: 1234567890,
            },
            data: vec![42u8; 100],
        };
        let bytes = dict.to_bytes();
        let dict2 = CpacDictionary::from_bytes(&bytes).unwrap();
        assert_eq!(dict.metadata.dict_id, dict2.metadata.dict_id);
        assert_eq!(dict.metadata.size, dict2.metadata.size);
        assert_eq!(dict.data, dict2.data);
    }

    #[test]
    fn train_empty_samples_error() {
        let result = CpacDictionary::train(&[], 1024);
        assert!(result.is_err());
    }

    #[test]
    fn invalid_magic() {
        let bytes = b"XXXX".to_vec();
        let result = CpacDictionary::from_bytes(&bytes);
        assert!(result.is_err());
    }

    #[test]
    fn file_save_load_roundtrip() {
        // Create mock dictionary
        let dict = CpacDictionary {
            metadata: DictionaryMetadata {
                dict_id: 99999,
                size: 50,
                samples: 10,
                corpus_size: 10000,
                created_at: 9876543210,
            },
            data: vec![255u8; 50],
        };

        let temp_dir = std::env::temp_dir();
        let dict_path = temp_dir.join("test.cpac-dict");

        dict.save_to_file(&dict_path).unwrap();
        let dict2 = CpacDictionary::load_from_file(&dict_path).unwrap();

        assert_eq!(dict.metadata.dict_id, dict2.metadata.dict_id);
        assert_eq!(dict.data, dict2.data);

        std::fs::remove_file(dict_path).ok();
    }
}
