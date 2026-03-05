#![no_main]
// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Fuzz the streaming decompressor: parse_header() and process() must
//! never panic on arbitrary input — only return Err.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Feed the entire input in one shot.
    let mut decomp = cpac_streaming::stream::StreamingDecompressor::new().unwrap();
    let _ = decomp.feed(data);

    // Also feed in small incremental chunks to exercise partial-header paths.
    let mut decomp2 = cpac_streaming::stream::StreamingDecompressor::new().unwrap();
    for chunk in data.chunks(7) {
        if decomp2.feed(chunk).is_err() {
            break; // Err is expected, but not panic
        }
    }

    // MSN metadata deserialize path: wrap arbitrary data in a minimal
    // well-formed streaming header so parse_header() can reach the
    // msn_len field.  Feed 23-byte header with msn_len = remaining bytes.
    if data.len() >= 5 {
        let msn_len = (data.len() - 5).min(0xFFFF) as u16;
        let mut header: Vec<u8> = Vec::with_capacity(23 + data.len());
        header.extend_from_slice(b"CS");   // magic
        header.push(1);                    // version
        header.extend_from_slice(&1u16.to_le_bytes()); // FLAG_MSN_ENABLED
        header.extend_from_slice(&1u32.to_le_bytes()); // num_blocks = 1
        header.extend_from_slice(&(data.len() as u64).to_le_bytes()); // orig_size
        header.extend_from_slice(&(1u32 << 20).to_le_bytes()); // block_size
        header.extend_from_slice(&msn_len.to_le_bytes()); // msn_len
        header.extend_from_slice(&data[5..5 + msn_len as usize]);
        let mut decomp3 = cpac_streaming::stream::StreamingDecompressor::new().unwrap();
        let _ = decomp3.feed(&header);
    }
});
