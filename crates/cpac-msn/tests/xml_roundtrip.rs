// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Test XML domain roundtrip with realistic data.

use cpac_msn::{extract, reconstruct};

#[test]
fn xml_complex_roundtrip() {
    let xml_data = br#"<?xml version="1.0"?>
<config>
  <server><host>localhost</host><port>8080</port><ssl>true</ssl></server>
  <database><host>db.example.com</host><port>5432</port><name>production</name></database>
  <cache><enabled>true</enabled><ttl>3600</ttl><max_size>1000</max_size></cache>
  <logging><level>INFO</level><format>json</format><output>/var/log/app.log</output></logging>
</config>"#;

    println!("Original length: {}", xml_data.len());

    let result = extract(xml_data, None, 0.5).unwrap();
    println!("Applied: {}", result.applied);
    println!("Domain: {:?}", result.domain_id);
    println!("Residual length: {}", result.residual.len());

    let reconstructed = reconstruct(&result).unwrap();
    println!("Reconstructed length: {}", reconstructed.len());

    if reconstructed != xml_data {
        println!("\nMismatch!");
        println!("Original:");
        println!("{}", String::from_utf8_lossy(xml_data));
        println!("\nReconstructed:");
        println!("{}", String::from_utf8_lossy(&reconstructed));

        // Find first difference
        for (i, (a, b)) in xml_data.iter().zip(reconstructed.iter()).enumerate() {
            if a != b {
                println!("\nFirst difference at byte {}: {} != {}", i, a, b);
                println!(
                    "Context: {:?}",
                    String::from_utf8_lossy(
                        &xml_data[i.saturating_sub(20)..i.saturating_add(20).min(xml_data.len())]
                    )
                );
                break;
            }
        }
    }

    assert_eq!(reconstructed, xml_data, "XML roundtrip should be lossless");
}
