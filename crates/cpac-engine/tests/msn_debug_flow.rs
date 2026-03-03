// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Debug MSN data flow to understand size mismatches.

use cpac_engine::{compress, decompress, CompressConfig};
use cpac_msn;

#[test]
fn debug_msn_extraction() {
    let data = b"test data with repeated patterns ".repeat(10);
    println!("Original data: {} bytes", data.len());
    println!("First 50 bytes: {:?}", &data[..50.min(data.len())]);
    
    // Test MSN extraction directly
    let msn_result = cpac_msn::extract(&data, None, 0.5).unwrap();
    println!("\nMSN extraction:");
    println!("  Applied: {}", msn_result.applied);
    println!("  Domain: {:?}", msn_result.domain_id);
    println!("  Confidence: {}", msn_result.confidence);
    println!("  Fields: {} entries", msn_result.fields.len());
    println!("  Residual size: {} bytes", msn_result.residual.len());
    
    // Test reconstruction
    let reconstructed = cpac_msn::reconstruct(&msn_result).unwrap();
    println!("\nMSN reconstruction:");
    println!("  Reconstructed size: {} bytes", reconstructed.len());
    println!("  Matches original: {}", reconstructed == data);
    
    if reconstructed != data {
        println!("\nMISMATCH DETAILS:");
        println!("  Expected: {} bytes", data.len());
        println!("  Got: {} bytes", reconstructed.len());
        if !reconstructed.is_empty() {
            println!("  First 50 reconstructed: {:?}", &reconstructed[..50.min(reconstructed.len())]);
        }
    }
}

#[test]
fn debug_msn_compress_decompress() {
    let data = b"test data with repeated patterns ".repeat(10);
    println!("Original: {} bytes", data.len());
    
    // Compress with MSN
    let config = CompressConfig {
        enable_msn: true,
        ..Default::default()
    };
    
    let compressed = compress(&data, &config).unwrap();
    println!("Compressed: {} bytes ({:.2}x)", compressed.compressed_size, compressed.ratio());
    
    // Decompress
    match decompress(&compressed.data) {
        Ok(decompressed) => {
            println!("Decompressed: {} bytes", decompressed.data.len());
            println!("Matches: {}", decompressed.data == data);
            
            if decompressed.data != data {
                println!("\nERROR: Size mismatch!");
                println!("  Expected: {}", data.len());
                println!("  Got: {}", decompressed.data.len());
            }
        }
        Err(e) => {
            println!("Decompression failed: {}", e);
        }
    }
}
