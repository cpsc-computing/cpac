// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Multi-file archive format (.cpar) for CPAC.

#![allow(clippy::cast_possible_truncation, clippy::missing_errors_doc)]

use cpac_types::{CompressConfig, CpacError, CpacResult};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const CPAR_MAGIC: &[u8; 4] = b"CPAR";
const CPAR_VERSION: u8 = 1;

/// Metadata for a single archive entry.
#[derive(Clone, Debug)]
pub struct ArchiveEntry {
    pub path: String,
    pub original_size: u64,
    pub compressed_size: u64,
    pub flags: u8,
    pub timestamp: u64,
}

/// Create a .cpar archive from a directory.
pub fn create_archive(dir: &Path, config: &CompressConfig) -> CpacResult<Vec<u8>> {
    let mut entries: Vec<(ArchiveEntry, Vec<u8>)> = Vec::new();
    collect_entries(dir, dir, config, &mut entries)?;
    let num_entries = entries.len() as u32;
    let mut buf = Vec::new();
    buf.extend_from_slice(CPAR_MAGIC);
    buf.push(CPAR_VERSION);
    buf.push(0);
    buf.extend_from_slice(&num_entries.to_le_bytes());
    for (entry, data) in &entries {
        let pb = entry.path.as_bytes();
        buf.extend_from_slice(&(pb.len() as u16).to_le_bytes());
        buf.extend_from_slice(pb);
        buf.extend_from_slice(&entry.original_size.to_le_bytes());
        buf.extend_from_slice(&entry.compressed_size.to_le_bytes());
        buf.push(entry.flags);
        buf.extend_from_slice(&entry.timestamp.to_le_bytes());
        buf.extend_from_slice(data);
    }
    Ok(buf)
}

fn collect_entries(
    base: &Path,
    dir: &Path,
    config: &CompressConfig,
    out: &mut Vec<(ArchiveEntry, Vec<u8>)>,
) -> CpacResult<()> {
    let rd = std::fs::read_dir(dir)
        .map_err(|e| CpacError::IoError(format!("{}: {e}", dir.display())))?;
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_entries(base, &p, config, out)?;
        } else if p.is_file() {
            let rel = p
                .strip_prefix(base)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            let data = std::fs::read(&p)
                .map_err(|e| CpacError::IoError(format!("{}: {e}", p.display())))?;
            let orig = data.len() as u64;
            let comp = cpac_engine::compress(&data, config)?;
            let ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            out.push((
                ArchiveEntry {
                    path: rel,
                    original_size: orig,
                    compressed_size: comp.compressed_size as u64,
                    flags: 0,
                    timestamp: ts,
                },
                comp.data,
            ));
        }
    }
    Ok(())
}

/// Extract a .cpar archive to a directory.
pub fn extract_archive(archive: &[u8], out_dir: &Path) -> CpacResult<Vec<ArchiveEntry>> {
    let parsed = parse_entries(archive)?;
    let mut entries = Vec::with_capacity(parsed.len());
    for (entry, data_off) in &parsed {
        let end = data_off + entry.compressed_size as usize;
        if end > archive.len() {
            return Err(CpacError::InvalidFrame("truncated archive data".into()));
        }
        let dec = cpac_engine::decompress(&archive[*data_off..end])?;
        let fp = out_dir.join(&entry.path);
        if let Some(parent) = fp.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| CpacError::IoError(format!("{}: {e}", parent.display())))?;
        }
        std::fs::write(&fp, &dec.data)
            .map_err(|e| CpacError::IoError(format!("{}: {e}", fp.display())))?;
        entries.push(entry.clone());
    }
    Ok(entries)
}

/// List entries without extracting.
pub fn list_archive(archive: &[u8]) -> CpacResult<Vec<ArchiveEntry>> {
    Ok(parse_entries(archive)?
        .into_iter()
        .map(|(e, _)| e)
        .collect())
}

/// Sanity cap: prevent OOM from malicious/corrupted input.
const MAX_ENTRIES: usize = 1_000_000;

/// Returns Vec of (entry, `data_offset`) pairs.
fn parse_entries(data: &[u8]) -> CpacResult<Vec<(ArchiveEntry, usize)>> {
    if data.len() < 10 || &data[0..4] != CPAR_MAGIC {
        return Err(CpacError::InvalidFrame("not a CPAR archive".into()));
    }
    if data[4] != CPAR_VERSION {
        return Err(CpacError::InvalidFrame("unsupported CPAR version".into()));
    }
    let n = u32::from_le_bytes([data[6], data[7], data[8], data[9]]) as usize;
    if n > MAX_ENTRIES {
        return Err(CpacError::InvalidFrame(format!(
            "archive claims {n} entries (max {MAX_ENTRIES})"
        )));
    }
    let mut off = 10usize;
    let mut entries = Vec::with_capacity(n);
    for _ in 0..n {
        if off + 2 > data.len() {
            return Err(CpacError::InvalidFrame("truncated".into()));
        }
        let pl = u16::from_le_bytes([data[off], data[off + 1]]) as usize;
        off += 2;
        if off + pl + 25 > data.len() {
            return Err(CpacError::InvalidFrame("truncated".into()));
        }
        let path = String::from_utf8_lossy(&data[off..off + pl]).to_string();
        off += pl;
        let orig = u64::from_le_bytes(data[off..off + 8].try_into().unwrap());
        off += 8;
        let comp = u64::from_le_bytes(data[off..off + 8].try_into().unwrap());
        off += 8;
        let flags = data[off];
        off += 1;
        let ts = u64::from_le_bytes(data[off..off + 8].try_into().unwrap());
        off += 8;
        let data_off = off;
        off += comp as usize; // skip past inline compressed data
        if off > data.len() {
            return Err(CpacError::InvalidFrame("truncated".into()));
        }
        entries.push((
            ArchiveEntry {
                path,
                original_size: orig,
                compressed_size: comp,
                flags,
                timestamp: ts,
            },
            data_off,
        ));
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_corpus() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::File::create(dir.path().join("hello.txt"))
            .unwrap()
            .write_all(b"Hello, CPAR!")
            .unwrap();
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        std::fs::File::create(dir.path().join("sub/data.bin"))
            .unwrap()
            .write_all(&[0u8; 256])
            .unwrap();
        dir
    }

    #[test]
    fn create_and_list() {
        let c = make_corpus();
        let ar = create_archive(c.path(), &CompressConfig::default()).unwrap();
        let entries = list_archive(&ar).unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn roundtrip() {
        let c = make_corpus();
        let ar = create_archive(c.path(), &CompressConfig::default()).unwrap();
        let out = tempfile::tempdir().unwrap();
        extract_archive(&ar, out.path()).unwrap();
        assert_eq!(
            std::fs::read(out.path().join("hello.txt")).unwrap(),
            b"Hello, CPAR!"
        );
        assert_eq!(
            std::fs::read(out.path().join("sub/data.bin")).unwrap(),
            vec![0u8; 256]
        );
    }

    #[test]
    fn empty_archive() {
        let d = tempfile::tempdir().unwrap();
        let ar = create_archive(d.path(), &CompressConfig::default()).unwrap();
        assert_eq!(list_archive(&ar).unwrap().len(), 0);
    }

    #[test]
    fn bad_magic() {
        assert!(list_archive(b"NOPE1234").is_err());
    }
}
