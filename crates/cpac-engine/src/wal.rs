// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Append-only CPWL (Write-Ahead Log / Journal) format.
//!
//! Designed for incremental archival and crash-safe recovery of compressed data.
//! Each entry is individually framed and CRC-protected so partial writes can be
//! detected and the journal truncated to the last valid entry.
//!
//! # Wire Format — CPWL v1
//!
//! **File header** (written once):
//! ```text
//! Magic("WL", 2B) | Version(1B) | Flags(2B LE) | CreatedTimestamp(8B LE, Unix µs)
//! ```
//!
//! **Each entry**:
//! ```text
//! EntryMagic(0xE1, 1B) | SeqNo(8B LE) | Timestamp(8B LE, Unix µs) |
//! KeyLen(2B LE) | Key(UTF-8) | PayloadLen(4B LE) | Payload |
//! CRC32C(4B LE over Key+Payload)
//! ```
//!
//! Recovery: scan forward; when `EntryMagic` + CRC check fails, truncate.

use cpac_types::{CpacError, CpacResult};
use std::io::{self, Read, Seek, Write};
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const WAL_MAGIC: &[u8; 2] = b"WL";
const WAL_VERSION: u8 = 1;
const WAL_HEADER_SIZE: usize = 2 + 1 + 2 + 8; // 13 bytes
const ENTRY_MAGIC: u8 = 0xE1;

// ---------------------------------------------------------------------------
// CRC32C (minimal, no external dep)
// ---------------------------------------------------------------------------

/// CRC-32C (Castagnoli) lookup table.
fn crc32c_table() -> &'static [u32; 256] {
    use std::sync::OnceLock;
    static TABLE: OnceLock<[u32; 256]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [0u32; 256];
        for i in 0..256u32 {
            let mut crc = i;
            for _ in 0..8 {
                if crc & 1 != 0 {
                    crc = (crc >> 1) ^ 0x82F6_3B78; // CRC-32C polynomial
                } else {
                    crc >>= 1;
                }
            }
            table[i as usize] = crc;
        }
        table
    })
}

/// Compute CRC-32C of a byte slice.
pub fn crc32c(data: &[u8]) -> u32 {
    let table = crc32c_table();
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        let idx = ((crc ^ u32::from(b)) & 0xFF) as usize;
        crc = (crc >> 8) ^ table[idx];
    }
    crc ^ 0xFFFF_FFFF
}

// ---------------------------------------------------------------------------
// Timestamp helper
// ---------------------------------------------------------------------------

fn now_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

// ---------------------------------------------------------------------------
// Journal entry
// ---------------------------------------------------------------------------

/// A single WAL entry.
#[derive(Debug, Clone)]
pub struct WalEntry {
    /// Monotonically increasing sequence number.
    pub seq: u64,
    /// Timestamp (Unix microseconds).
    pub timestamp: u64,
    /// Entry key (e.g. filename or identifier).
    pub key: String,
    /// Compressed payload.
    pub payload: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

/// Append-only journal writer.
pub struct WalWriter<W: Write + Seek> {
    inner: W,
    next_seq: u64,
    entries_written: u64,
}

impl<W: Write + Seek> WalWriter<W> {
    /// Create a new WAL, writing the file header.
    pub fn create(mut inner: W) -> CpacResult<Self> {
        let ts = now_micros();
        inner.write_all(WAL_MAGIC).map_err(io_err)?;
        inner.write_all(&[WAL_VERSION]).map_err(io_err)?;
        inner.write_all(&0u16.to_le_bytes()).map_err(io_err)?; // flags
        inner.write_all(&ts.to_le_bytes()).map_err(io_err)?;
        inner.flush().map_err(io_err)?;
        Ok(Self {
            inner,
            next_seq: 0,
            entries_written: 0,
        })
    }

    /// Append an entry to the journal.
    pub fn append(&mut self, key: &str, payload: &[u8]) -> CpacResult<u64> {
        let seq = self.next_seq;
        self.next_seq += 1;
        let ts = now_micros();
        let key_bytes = key.as_bytes();

        // CRC over key + payload
        let mut crc_data = Vec::with_capacity(key_bytes.len() + payload.len());
        crc_data.extend_from_slice(key_bytes);
        crc_data.extend_from_slice(payload);
        let crc = crc32c(&crc_data);

        // Write entry
        self.inner.write_all(&[ENTRY_MAGIC]).map_err(io_err)?;
        self.inner.write_all(&seq.to_le_bytes()).map_err(io_err)?;
        self.inner.write_all(&ts.to_le_bytes()).map_err(io_err)?;
        self.inner
            .write_all(&(key_bytes.len() as u16).to_le_bytes())
            .map_err(io_err)?;
        self.inner.write_all(key_bytes).map_err(io_err)?;
        self.inner
            .write_all(&(payload.len() as u32).to_le_bytes())
            .map_err(io_err)?;
        self.inner.write_all(payload).map_err(io_err)?;
        self.inner.write_all(&crc.to_le_bytes()).map_err(io_err)?;
        self.inner.flush().map_err(io_err)?;

        self.entries_written += 1;
        Ok(seq)
    }

    /// Number of entries written so far.
    pub fn entries_written(&self) -> u64 {
        self.entries_written
    }

    /// Consume the writer, returning the inner writer.
    pub fn into_inner(self) -> W {
        self.inner
    }
}

// ---------------------------------------------------------------------------
// Reader / Recovery
// ---------------------------------------------------------------------------

/// Journal reader with crash-recovery support.
pub struct WalReader<R: Read> {
    inner: R,
    _header_read: bool,
    created_timestamp: u64,
}

impl<R: Read> WalReader<R> {
    /// Open a WAL for reading.
    pub fn open(mut inner: R) -> CpacResult<Self> {
        let mut header = [0u8; WAL_HEADER_SIZE];
        inner
            .read_exact(&mut header)
            .map_err(|e| CpacError::InvalidFrame(format!("WAL header read: {e}")))?;
        if &header[0..2] != WAL_MAGIC || header[2] != WAL_VERSION {
            return Err(CpacError::InvalidFrame("not a CPWL journal".into()));
        }
        let created = u64::from_le_bytes(header[5..13].try_into().unwrap());
        Ok(Self {
            inner,
            _header_read: true,
            created_timestamp: created,
        })
    }

    /// Read the next entry, or `None` if EOF / corrupt.
    pub fn next_entry(&mut self) -> Option<CpacResult<WalEntry>> {
        // Read entry magic
        let mut magic = [0u8; 1];
        if self.inner.read_exact(&mut magic).is_err() {
            return None; // EOF
        }
        if magic[0] != ENTRY_MAGIC {
            return Some(Err(CpacError::InvalidFrame("bad entry magic".into())));
        }

        // seq(8) + ts(8) + key_len(2)
        let mut buf = [0u8; 18];
        if self.inner.read_exact(&mut buf).is_err() {
            return Some(Err(CpacError::InvalidFrame("truncated entry header".into())));
        }
        let seq = u64::from_le_bytes(buf[0..8].try_into().unwrap());
        let ts = u64::from_le_bytes(buf[8..16].try_into().unwrap());
        let key_len = u16::from_le_bytes(buf[16..18].try_into().unwrap()) as usize;

        // Key
        let mut key_buf = vec![0u8; key_len];
        if self.inner.read_exact(&mut key_buf).is_err() {
            return Some(Err(CpacError::InvalidFrame("truncated key".into())));
        }
        let key = match String::from_utf8(key_buf.clone()) {
            Ok(k) => k,
            Err(e) => {
                return Some(Err(CpacError::InvalidFrame(format!(
                    "invalid UTF-8 key: {e}"
                ))))
            }
        };

        // Payload length
        let mut plen_buf = [0u8; 4];
        if self.inner.read_exact(&mut plen_buf).is_err() {
            return Some(Err(CpacError::InvalidFrame(
                "truncated payload length".into(),
            )));
        }
        let plen = u32::from_le_bytes(plen_buf) as usize;

        // Payload
        let mut payload = vec![0u8; plen];
        if self.inner.read_exact(&mut payload).is_err() {
            return Some(Err(CpacError::InvalidFrame("truncated payload".into())));
        }

        // CRC
        let mut crc_buf = [0u8; 4];
        if self.inner.read_exact(&mut crc_buf).is_err() {
            return Some(Err(CpacError::InvalidFrame("truncated CRC".into())));
        }
        let stored_crc = u32::from_le_bytes(crc_buf);

        // Verify CRC
        let mut crc_data = Vec::with_capacity(key_buf.len() + payload.len());
        crc_data.extend_from_slice(&key_buf);
        crc_data.extend_from_slice(&payload);
        let computed_crc = crc32c(&crc_data);

        if stored_crc != computed_crc {
            return Some(Err(CpacError::InvalidFrame(format!(
                "CRC mismatch at seq {seq}: stored={stored_crc:#010X} computed={computed_crc:#010X}"
            ))));
        }

        Some(Ok(WalEntry {
            seq,
            timestamp: ts,
            key,
            payload,
        }))
    }

    /// Read all valid entries, stopping at first corrupt/truncated entry.
    pub fn recover_all(&mut self) -> Vec<WalEntry> {
        let mut entries = Vec::new();
        while let Some(Ok(entry)) = self.next_entry() {
            entries.push(entry);
        }
        entries
    }

    /// Creation timestamp (Unix microseconds).
    pub fn created_timestamp(&self) -> u64 {
        self.created_timestamp
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn io_err(e: io::Error) -> CpacError {
    CpacError::IoError(e.to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn wal_roundtrip() {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut writer = WalWriter::create(&mut buf).unwrap();
            writer.append("file1.dat", b"compressed_data_1").unwrap();
            writer.append("file2.dat", b"compressed_data_2").unwrap();
            assert_eq!(writer.entries_written(), 2);
        }

        buf.set_position(0);
        let mut reader = WalReader::open(&mut buf).unwrap();
        let entries = reader.recover_all();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].seq, 0);
        assert_eq!(entries[0].key, "file1.dat");
        assert_eq!(entries[0].payload, b"compressed_data_1");
        assert_eq!(entries[1].seq, 1);
        assert_eq!(entries[1].key, "file2.dat");
    }

    #[test]
    fn wal_recovery_truncated() {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut writer = WalWriter::create(&mut buf).unwrap();
            writer.append("good", b"ok").unwrap();
            writer.append("also_good", b"ok2").unwrap();
        }

        // Truncate the last entry (simulate crash)
        let data = buf.into_inner();
        let truncated = &data[..data.len() - 5]; // cut off CRC + part of payload
        let mut cursor = Cursor::new(truncated);
        let mut reader = WalReader::open(&mut cursor).unwrap();
        let entries = reader.recover_all();
        assert_eq!(entries.len(), 1); // only first entry recovered
        assert_eq!(entries[0].key, "good");
    }

    #[test]
    fn wal_bad_magic() {
        let bad = b"XXbaddata";
        let mut cursor = Cursor::new(bad.as_slice());
        let result = WalReader::open(&mut cursor);
        assert!(result.is_err());
    }

    #[test]
    fn crc32c_basic() {
        // Known test vector: CRC-32C of "123456789" = 0xE3069283
        let crc = crc32c(b"123456789");
        assert_eq!(crc, 0xE306_9283);
    }

    #[test]
    fn wal_empty_journal() {
        let mut buf = Cursor::new(Vec::new());
        WalWriter::create(&mut buf).unwrap();
        buf.set_position(0);
        let mut reader = WalReader::open(&mut buf).unwrap();
        assert!(reader.created_timestamp() > 0);
        let entries = reader.recover_all();
        assert!(entries.is_empty());
    }
}
