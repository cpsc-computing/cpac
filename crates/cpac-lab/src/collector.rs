// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Corpus file collection with extension-based filtering.

use std::path::{Path, PathBuf};

/// Known text-like extensions (passed through to the lab).
const TEXT_EXTENSIONS: &[&str] = &[
    "log", "txt", "text", "json", "jsonl", "yaml", "yml", "csv", "xml",
    "html", "htm", "sql", "rtf", "conf", "cnf", "tf", "java", "c", "h",
    "sh", "bash", "py", "rs", "go", "tex", "sgml", "ps", "eps", "kml",
    "pl", "f", "unk", "dump", "script", "lsp", "dist", "disabled", "3",
    "1", "0",
];

/// Known binary extensions (skip entirely).
const BINARY_EXTENSIONS: &[&str] = &[
    "flac", "wav", "mp3", "ogg", "aac", "mp4", "avi", "mkv", "mov",
    "jpg", "jpeg", "png", "gif", "bmp", "tiff", "webp",
    "pdf", "doc", "docx", "ppt", "pptx", "xls", "xlsx", "pub", "pps",
    "gz", "zip", "tar", "bz2", "xz", "7z", "rar", "cpac",
    "bin", "so", "dll", "exe", "obj", "o",
    "fits", "swf", "kmz", "wp", "hlp", "dbase3",
];

/// Options for file collection.
#[derive(Clone, Debug)]
pub struct CollectOptions {
    /// Only include files with these extensions (empty = use default text list).
    pub extensions: Vec<String>,
    /// Exclude files with these extensions.
    pub exclude_extensions: Vec<String>,
    /// Maximum file size in bytes (0 = no limit).
    pub max_file_size: u64,
    /// Whether to recurse into subdirectories.
    pub recursive: bool,
    /// Include all file types, not just text.
    pub all_types: bool,
}

impl Default for CollectOptions {
    fn default() -> Self {
        Self {
            extensions: vec![],
            exclude_extensions: vec![],
            max_file_size: 0,
            recursive: true,
            all_types: false,
        }
    }
}

/// A collected file with metadata.
#[derive(Clone, Debug)]
pub struct CorpusFile {
    /// Full path to the file.
    pub path: PathBuf,
    /// File extension (lowercase, without dot).
    pub extension: String,
    /// Relative directory from the corpus root.
    pub rel_dir: String,
    /// File name.
    pub name: String,
    /// File size in bytes.
    pub size: u64,
}

/// Collect files from a corpus directory.
#[must_use]
pub fn collect_files(root: &Path, opts: &CollectOptions) -> Vec<CorpusFile> {
    let mut files = Vec::new();
    collect_recursive(root, root, opts, &mut files);
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

fn collect_recursive(
    dir: &Path,
    root: &Path,
    opts: &CollectOptions,
    result: &mut Vec<CorpusFile>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            if opts.recursive {
                collect_recursive(&path, root, opts, result);
            }
            continue;
        }
        if !path.is_file() {
            continue;
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();

        // Extension filtering
        if !opts.all_types {
            if BINARY_EXTENSIONS.contains(&ext.as_str()) {
                continue;
            }
            if !opts.extensions.is_empty() {
                if !opts.extensions.iter().any(|e| e == &ext) {
                    continue;
                }
            } else if !ext.is_empty() && !TEXT_EXTENSIONS.contains(&ext.as_str()) {
                continue;
            }
        }

        if opts.exclude_extensions.iter().any(|e| e == &ext) {
            continue;
        }

        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if opts.max_file_size > 0 && size > opts.max_file_size {
            continue;
        }

        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();

        let rel_dir = path
            .strip_prefix(root)
            .ok()
            .and_then(|p| p.parent())
            .and_then(|p| p.to_str())
            .unwrap_or("")
            .to_string();

        result.push(CorpusFile {
            path,
            extension: ext,
            rel_dir,
            name,
            size,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_options() {
        let opts = CollectOptions::default();
        assert!(opts.extensions.is_empty());
        assert!(opts.recursive);
        assert!(!opts.all_types);
        assert_eq!(opts.max_file_size, 0);
    }
}
