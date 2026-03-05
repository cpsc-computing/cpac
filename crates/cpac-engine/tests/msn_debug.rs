// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Debug MSN compression behavior.

use cpac_engine::{compress, CompressConfig};

#[test]
fn debug_msn_sizes() {
    let json_data = r#"{"name":"Alice","age":30,"city":"NYC","status":"active"}
{"name":"Bob","age":25,"city":"LA","status":"active"}
{"name":"Charlie","age":35,"city":"SF","status":"inactive"}
{"name":"Diana","age":28,"city":"NYC","status":"active"}
{"name":"Eve","age":32,"city":"LA","status":"active"}
{"name":"Frank","age":29,"city":"NYC","status":"active"}
{"name":"Grace","age":31,"city":"SF","status":"inactive"}
{"name":"Henry","age":27,"city":"LA","status":"active"}
{"name":"Iris","age":33,"city":"NYC","status":"active"}
{"name":"Jack","age":26,"city":"SF","status":"inactive"}"#;

    let data = json_data.repeat(10);
    println!("Original size: {} bytes", data.len());

    // Test MSN extraction directly
    let msn_result = cpac_msn::extract(data.as_bytes(), None, 0.5).unwrap();
    println!("MSN domain: {:?}", msn_result.domain_id);
    println!("MSN applied: {}", msn_result.applied);
    println!("MSN confidence: {:.2}", msn_result.confidence);
    println!(
        "MSN fields size: {} bytes",
        serde_json::to_vec(&msn_result.fields).unwrap().len()
    );
    println!("MSN residual size: {} bytes", msn_result.residual.len());
    println!(
        "MSN total (fields+residual): {} bytes",
        serde_json::to_vec(&msn_result.fields).unwrap().len() + msn_result.residual.len()
    );

    // Without MSN
    let config_no_msn = CompressConfig {
        enable_msn: false,
        ..Default::default()
    };
    let result_no_msn = compress(data.as_bytes(), &config_no_msn).unwrap();
    println!("\nWithout MSN:");
    println!("  Compressed size: {} bytes", result_no_msn.compressed_size);
    println!("  Ratio: {:.2}x", result_no_msn.ratio());

    // With MSN
    let config_msn = CompressConfig {
        enable_msn: true,
        ..Default::default()
    };
    let result_msn = compress(data.as_bytes(), &config_msn).unwrap();
    println!("\nWith MSN:");
    println!("  Compressed size: {} bytes", result_msn.compressed_size);
    println!("  Ratio: {:.2}x", result_msn.ratio());
}
