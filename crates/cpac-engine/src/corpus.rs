// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Corpus management and downloading for benchmark infrastructure.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;

/// Corpus metadata from YAML configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorpusConfig {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub download_url: DownloadUrl,
    #[serde(default)]
    pub download_kind: DownloadKind,
    #[serde(default)]
    pub target_subdir: String,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub citation: String,
}

/// Download URL variants (single or multiple).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(untagged)]
pub enum DownloadUrl {
    #[default]
    None,
    Single(String),
    Multiple(Vec<String>),
}

/// Download format type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DownloadKind {
    #[default]
    HttpFile,
    HttpTargz,
    HttpZip,
    HttpFileMulti,
}

/// Load corpus configuration from YAML file.
///
/// # Errors
///
/// Returns error if file cannot be read or parsed.
pub fn load_corpus_config(path: &Path) -> io::Result<CorpusConfig> {
    let contents = fs::read_to_string(path)?;
    serde_yaml::from_str(&contents)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// List all available corpus configs.
pub fn list_corpus_configs(config_dir: &Path) -> io::Result<Vec<CorpusConfig>> {
    let mut configs = Vec::new();
    
    if !config_dir.exists() {
        return Ok(configs);
    }
    
    for entry in fs::read_dir(config_dir)? {
        let entry = entry?;
        let path = entry.path();
        
        if path.extension().and_then(|s| s.to_str()) == Some("yaml") 
            && path.file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.starts_with("corpus_"))
                .unwrap_or(false)
        {
            if let Ok(config) = load_corpus_config(&path) {
                configs.push(config);
            }
        }
    }
    
    configs.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(configs)
}

/// Check if corpus is already downloaded.
#[must_use]
pub fn is_corpus_downloaded(benchdata_root: &Path, corpus_id: &str) -> bool {
    benchdata_root.join(corpus_id).exists()
}

/// Download and extract corpus.
///
/// # Errors
///
/// Returns error if download or extraction fails.
#[cfg(feature = "download")]
pub fn download_corpus(
    config: &CorpusConfig,
    benchdata_root: &Path,
    _cache_dir: &Path,
) -> io::Result<()> {
    use indicatif::{ProgressBar, ProgressStyle};
    
    let target_dir = benchdata_root.join(&config.target_subdir);
    fs::create_dir_all(&target_dir)?;
    
    match &config.download_url {
        DownloadUrl::None => {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "No download URL configured",
            ))
        }
        DownloadUrl::Single(url) => {
            println!("Downloading {} from {}", config.name, url);
            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template("{spinner:.green} [{elapsed_precise}] {msg}")
                    .unwrap(),
            );
            pb.set_message("Downloading...");
            
            let response = reqwest::blocking::get(url)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            
            let bytes = response
                .bytes()
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            
            pb.set_message("Extracting...");
            
            match config.download_kind {
                DownloadKind::HttpTargz => {
                    extract_targz(&bytes, &target_dir)?;
                }
                DownloadKind::HttpZip => {
                    extract_zip(&bytes, &target_dir)?;
                }
                DownloadKind::HttpFile => {
                    let filename = url.rsplit('/').next().unwrap_or("download");
                    let dest_path = target_dir.join(filename);
                    fs::write(dest_path, bytes)?;
                }
                DownloadKind::HttpFileMulti => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "Single URL cannot be used with http_file_multi",
                    ))
                }
            }
            
            pb.finish_with_message("Complete");
        }
        DownloadUrl::Multiple(urls) => {
            println!("Downloading {} ({} files)", config.name, urls.len());
            let pb = ProgressBar::new(urls.len() as u64);
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("[{bar:40.cyan/blue}] {pos}/{len} {msg}")
                    .unwrap()
                    .progress_chars("█▓░"),
            );
            
            for url in urls {
                let filename = url.rsplit('/').next().unwrap_or("download");
                pb.set_message(filename.to_string());
                
                let response = reqwest::blocking::get(url)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                
                let bytes = response
                    .bytes()
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                
                let dest_path = target_dir.join(filename);
                fs::write(dest_path, bytes)?;
                
                pb.inc(1);
            }
            
            pb.finish_with_message("Complete");
        }
    }
    
    Ok(())
}

#[cfg(feature = "download")]
fn extract_targz(data: &[u8], target_dir: &Path) -> io::Result<()> {
    use flate2::read::GzDecoder;
    use tar::Archive;
    
    let gz = GzDecoder::new(data);
    let mut archive = Archive::new(gz);
    archive.unpack(target_dir)?;
    Ok(())
}

#[cfg(feature = "download")]
fn extract_zip(data: &[u8], target_dir: &Path) -> io::Result<()> {
    use std::io::Cursor;
    use zip::ZipArchive;
    
    let cursor = Cursor::new(data);
    let mut archive = ZipArchive::new(cursor)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        
        let outpath = match file.enclosed_name() {
            Some(path) => target_dir.join(path),
            None => continue,
        };
        
        if file.is_dir() {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(parent) = outpath.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut outfile = fs::File::create(&outpath)?;
            io::copy(&mut file, &mut outfile)?;
        }
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_corpus_config_parse() {
        let yaml = r#"
id: test_corpus
name: Test Corpus
description: A test corpus
download_url: https://example.com/test.tar.gz
download_kind: http_targz
target_subdir: test
license: Public domain
citation: Test citation
"#;
        
        let config: CorpusConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.id, "test_corpus");
        assert_eq!(config.name, "Test Corpus");
        assert_eq!(config.download_kind, DownloadKind::HttpTargz);
    }
}
