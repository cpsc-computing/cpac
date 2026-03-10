// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Multi-file archive format (.cpar) for CPAC.

#![allow(clippy::cast_possible_truncation, clippy::missing_errors_doc)]

use cpac_types::{CompressConfig, CpacError, CpacResult};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const CPAR_MAGIC: &[u8; 4] = b"CPAR";
const CPAR_VERSION: u8 = 1;

/// Global archive flags (byte at offset 5).
const FLAG_SOLID: u8 = 0x01;

/// Metadata for a single archive entry.
#[derive(Clone, Debug)]
pub struct ArchiveEntry {
    pub path: String,
    pub original_size: u64,
    /// Per-file compressed size (regular) or offset in concatenation (solid).
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
///
/// Handles both regular and solid archives transparently.
pub fn extract_archive(archive: &[u8], out_dir: &Path) -> CpacResult<Vec<ArchiveEntry>> {
    if is_solid(archive) {
        return extract_solid(archive, out_dir);
    }
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

/// Extract a solid CPAR archive.
fn extract_solid(archive: &[u8], out_dir: &Path) -> CpacResult<Vec<ArchiveEntry>> {
    let (parsed, blob_meta_off) = parse_solid_index(archive)?;

    // Read blob size and decompress
    if blob_meta_off + 8 > archive.len() {
        return Err(CpacError::InvalidFrame("truncated solid blob header".into()));
    }
    let blob_size = u64::from_le_bytes(
        archive[blob_meta_off..blob_meta_off + 8].try_into().unwrap(),
    ) as usize;
    let blob_start = blob_meta_off + 8;
    let blob_end = blob_start + blob_size;
    if blob_end > archive.len() {
        return Err(CpacError::InvalidFrame("truncated solid blob data".into()));
    }
    let dec = cpac_engine::decompress(&archive[blob_start..blob_end])?;
    let concat_data = &dec.data;

    // Extract each file from the decompressed concatenation
    let mut entries = Vec::with_capacity(parsed.len());
    for (entry, offset_in_concat) in &parsed {
        let start = *offset_in_concat as usize;
        let end = start + entry.original_size as usize;
        if end > concat_data.len() {
            return Err(CpacError::InvalidFrame(format!(
                "solid entry '{}' out of bounds (offset {} + size {} > blob {})",
                entry.path, start, entry.original_size, concat_data.len()
            )));
        }
        let fp = out_dir.join(&entry.path);
        if let Some(parent) = fp.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| CpacError::IoError(format!("{}: {e}", parent.display())))?;
        }
        std::fs::write(&fp, &concat_data[start..end])
            .map_err(|e| CpacError::IoError(format!("{}: {e}", fp.display())))?;
        entries.push(entry.clone());
    }
    Ok(entries)
}

/// Create a solid .cpar archive — all files concatenated, compressed as one stream.
///
/// Solid archives exploit cross-file redundancy for better compression when
/// files share patterns (e.g., cloud configs, similar logs).
pub fn create_archive_solid(dir: &Path, config: &CompressConfig) -> CpacResult<Vec<u8>> {
    let mut file_entries: Vec<(String, Vec<u8>, u64)> = Vec::new(); // (rel_path, raw_data, timestamp)
    collect_raw_entries(dir, dir, &mut file_entries)?;

    // Concatenate all file data
    let mut concat = Vec::new();
    let mut offsets: Vec<u64> = Vec::with_capacity(file_entries.len());
    for (_path, data, _ts) in &file_entries {
        offsets.push(concat.len() as u64);
        concat.extend_from_slice(data);
    }

    // Compress the entire concatenation as one stream
    let compressed = cpac_engine::compress(&concat, config)?;

    // Build the archive
    let num_entries = file_entries.len() as u32;
    let mut buf = Vec::new();
    buf.extend_from_slice(CPAR_MAGIC);
    buf.push(CPAR_VERSION);
    buf.push(FLAG_SOLID);
    buf.extend_from_slice(&num_entries.to_le_bytes());

    // Write entry metadata (no inline data — compressed_size field = offset_in_concat)
    for (i, (path, data, ts)) in file_entries.iter().enumerate() {
        let pb = path.as_bytes();
        buf.extend_from_slice(&(pb.len() as u16).to_le_bytes());
        buf.extend_from_slice(pb);
        buf.extend_from_slice(&(data.len() as u64).to_le_bytes()); // original_size
        buf.extend_from_slice(&offsets[i].to_le_bytes()); // offset_in_concat (reuses compressed_size slot)
        buf.push(0); // entry flags
        buf.extend_from_slice(&ts.to_le_bytes());
    }

    // Append compressed blob: size + data
    buf.extend_from_slice(&(compressed.data.len() as u64).to_le_bytes());
    buf.extend_from_slice(&compressed.data);

    Ok(buf)
}

/// Collect raw (uncompressed) file data for solid mode.
fn collect_raw_entries(
    base: &Path,
    dir: &Path,
    out: &mut Vec<(String, Vec<u8>, u64)>,
) -> CpacResult<()> {
    let rd = std::fs::read_dir(dir)
        .map_err(|e| CpacError::IoError(format!("{}: {e}", dir.display())))?;
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_raw_entries(base, &p, out)?;
        } else if p.is_file() {
            let rel = p
                .strip_prefix(base)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            let data = std::fs::read(&p)
                .map_err(|e| CpacError::IoError(format!("{}: {e}", p.display())))?;
            let ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            out.push((rel, data, ts));
        }
    }
    Ok(())
}

/// List entries without extracting.
pub fn list_archive(archive: &[u8]) -> CpacResult<Vec<ArchiveEntry>> {
    if is_solid(archive) {
        return Ok(parse_solid_index(archive)?
            .0
            .into_iter()
            .map(|(e, _off)| e)
            .collect());
    }
    Ok(parse_entries(archive)?
        .into_iter()
        .map(|(e, _)| e)
        .collect())
}

/// Check if an archive uses solid mode.
fn is_solid(data: &[u8]) -> bool {
    data.len() >= 6 && &data[0..4] == CPAR_MAGIC && (data[5] & FLAG_SOLID) != 0
}

/// Sanity cap: prevent OOM from malicious/corrupted input.
const MAX_ENTRIES: usize = 1_000_000;

/// Parse solid archive index — returns (entries_with_offsets, blob_start_offset).
fn parse_solid_index(data: &[u8]) -> CpacResult<(Vec<(ArchiveEntry, u64)>, usize)> {
    if data.len() < 10 || &data[0..4] != CPAR_MAGIC {
        return Err(CpacError::InvalidFrame("not a CPAR archive".into()));
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
        let offset_in_concat = u64::from_le_bytes(data[off..off + 8].try_into().unwrap());
        off += 8;
        let flags = data[off];
        off += 1;
        let ts = u64::from_le_bytes(data[off..off + 8].try_into().unwrap());
        off += 8;
        // No inline data in solid mode
        entries.push((
            ArchiveEntry {
                path,
                original_size: orig,
                compressed_size: offset_in_concat, // stores offset for extraction
                flags,
                timestamp: ts,
            },
            offset_in_concat,
        ));
    }
    Ok((entries, off))
}

/// Returns Vec of (entry, `data_offset`) pairs — regular (non-solid) archives.
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

    #[test]
    fn solid_create_and_list() {
        let c = make_corpus();
        let ar = create_archive_solid(c.path(), &CompressConfig::default()).unwrap();
        // Check solid flag
        assert_eq!(ar[5] & FLAG_SOLID, FLAG_SOLID);
        let entries = list_archive(&ar).unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn solid_roundtrip() {
        let c = make_corpus();
        let ar = create_archive_solid(c.path(), &CompressConfig::default()).unwrap();
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
    fn solid_empty_archive() {
        let d = tempfile::tempdir().unwrap();
        let ar = create_archive_solid(d.path(), &CompressConfig::default()).unwrap();
        assert_eq!(ar[5] & FLAG_SOLID, FLAG_SOLID);
        assert_eq!(list_archive(&ar).unwrap().len(), 0);
    }

    #[test]
    fn solid_smaller_for_similar_files() {
        // Create corpus with many similar files (should benefit from solid)
        let dir = tempfile::tempdir().unwrap();
        for i in 0..10 {
            let mut content = format!("apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: config-{i}\ndata:\n");
            content.push_str(&format!("  key: value-{i}\n  common: shared-pattern\n"));
            std::fs::write(dir.path().join(format!("config-{i}.yaml")), content).unwrap();
        }
        let regular = create_archive(dir.path(), &CompressConfig::default()).unwrap();
        let solid = create_archive_solid(dir.path(), &CompressConfig::default()).unwrap();
        // Solid should be smaller or equal for similar files
        assert!(
            solid.len() <= regular.len(),
            "solid ({}) should be <= regular ({}) for similar files",
            solid.len(),
            regular.len()
        );
    }
}
