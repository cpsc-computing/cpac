// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Benchmark corpus generator - creates representative test data files.
//!
//! Run with: cargo bench --bench generate_corpus -- --ignored

use criterion::{criterion_group, criterion_main, Criterion};
use std::fs;
use std::path::Path;

const CORPUS_DIR: &str = "benches/corpus";

fn generate_text_english(size_kb: usize) -> Vec<u8> {
    let sentence = b"The quick brown fox jumps over the lazy dog. ";
    let mut data = Vec::with_capacity(size_kb * 1024);
    while data.len() < size_kb * 1024 {
        data.extend_from_slice(sentence);
    }
    data.truncate(size_kb * 1024);
    data
}

fn generate_csv(rows: usize) -> Vec<u8> {
    let mut csv = String::from("id,timestamp,user_id,action,value,status\n");
    for i in 0..rows {
        csv.push_str(&format!(
            "{},{},{},{},{},{}\n",
            i,
            1704067200 + i * 60,
            i % 1000,
            ["login", "logout", "view", "edit", "delete"][i % 5],
            i * 7 % 10000,
            if i % 3 == 0 { "success" } else { "pending" }
        ));
    }
    csv.into_bytes()
}

fn generate_json(objects: usize) -> Vec<u8> {
    let mut json = String::from("[\n");
    for i in 0..objects {
        json.push_str(&format!(
            r#"  {{"id": {}, "name": "item_{}", "value": {}, "active": {}, "tags": ["tag{}", "cat{}"]}}{}
"#,
            i,
            i,
            i * 13 % 10000,
            if i % 2 == 0 { "true" } else { "false" },
            i % 10,
            i % 5,
            if i < objects - 1 { "," } else { "" }
        ));
    }
    json.push_str("]\n");
    json.into_bytes()
}

fn generate_xml(records: usize) -> Vec<u8> {
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<records>\n");
    for i in 0..records {
        xml.push_str(&format!(
            "  <record id=\"{}\">\n    <name>Record {}</name>\n    <value>{}</value>\n    <category>{}</category>\n  </record>\n",
            i,
            i,
            i * 11 % 10000,
            ["A", "B", "C", "D"][i % 4]
        ));
    }
    xml.push_str("</records>\n");
    xml.into_bytes()
}

fn generate_log(lines: usize) -> Vec<u8> {
    let mut log = String::new();
    for i in 0..lines {
        let level = ["INFO", "WARN", "ERROR", "DEBUG"][i % 4];
        let hour = 10 + (i / 3600) % 14;
        let minute = (i / 60) % 60;
        let second = i % 60;
        log.push_str(&format!(
            "2026-03-01 {:02}:{:02}:{:02} [{}] server: Processing request {} from 192.168.{}.{} - {} ms\n",
            hour, minute, second, level, i, (i / 256) % 256, i % 256, i % 1000
        ));
    }
    log.into_bytes()
}

fn generate_binary_structured(size_kb: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(size_kb * 1024);
    // ELF-like header
    data.extend_from_slice(b"\x7fELF\x02\x01\x01\x00");
    // Structured sections
    while data.len() < size_kb * 1024 {
        data.extend_from_slice(&[0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07]);
        data.extend_from_slice(&[0xFF, 0xFE, 0xFD, 0xFC, 0xFB, 0xFA, 0xF9, 0xF8]);
    }
    data.truncate(size_kb * 1024);
    data
}

fn generate_source_code(lines: usize) -> Vec<u8> {
    let mut code = String::from("// Rust source code sample\n");
    code.push_str("use std::collections::HashMap;\n\n");
    for i in 0..lines {
        let indent = "    ".repeat((i % 3) + 1);
        code.push_str(&format!(
            "{}fn function_{}(param: i32) -> Result<i32, Error> {{\n",
            indent, i
        ));
        code.push_str(&format!("{}    Ok(param * {})\n", indent, i % 100));
        code.push_str(&format!("{}}}\n\n", indent));
    }
    code.into_bytes()
}

fn generate_random(size_kb: usize) -> Vec<u8> {
    let mut rng: u64 = 0x123456789ABCDEF0;
    (0..size_kb * 1024)
        .map(|_| {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (rng >> 33) as u8
        })
        .collect()
}

fn write_corpus(name: &str, data: &[u8]) {
    let path = Path::new(CORPUS_DIR).join(name);
    fs::write(&path, data).unwrap();
    println!("✓ Generated: {} ({} bytes)", path.display(), data.len());
}

#[allow(dead_code)]
fn generate_all_corpus() {
    fs::create_dir_all(CORPUS_DIR).unwrap();
    
    println!("\n🔨 Generating benchmark corpus...\n");
    
    // Text files (10KB, 100KB, 1MB)
    write_corpus("text_10kb.txt", &generate_text_english(10));
    write_corpus("text_100kb.txt", &generate_text_english(100));
    write_corpus("text_1mb.txt", &generate_text_english(1024));
    
    // CSV files
    write_corpus("csv_1k_rows.csv", &generate_csv(1000));
    write_corpus("csv_10k_rows.csv", &generate_csv(10000));
    write_corpus("csv_100k_rows.csv", &generate_csv(100000));
    
    // JSON files
    write_corpus("json_100_objects.json", &generate_json(100));
    write_corpus("json_1k_objects.json", &generate_json(1000));
    write_corpus("json_10k_objects.json", &generate_json(10000));
    
    // XML files
    write_corpus("xml_500_records.xml", &generate_xml(500));
    write_corpus("xml_5k_records.xml", &generate_xml(5000));
    
    // Log files
    write_corpus("log_1k_lines.log", &generate_log(1000));
    write_corpus("log_10k_lines.log", &generate_log(10000));
    write_corpus("log_100k_lines.log", &generate_log(100000));
    
    // Binary structured
    write_corpus("binary_10kb.bin", &generate_binary_structured(10));
    write_corpus("binary_100kb.bin", &generate_binary_structured(100));
    write_corpus("binary_1mb.bin", &generate_binary_structured(1024));
    
    // Source code
    write_corpus("source_100_funcs.rs", &generate_source_code(100));
    write_corpus("source_1k_funcs.rs", &generate_source_code(1000));
    
    // Random (incompressible)
    write_corpus("random_10kb.bin", &generate_random(10));
    write_corpus("random_100kb.bin", &generate_random(100));
    write_corpus("random_1mb.bin", &generate_random(1024));
    
    // Create README
    let readme = r#"# Benchmark Corpus

Representative test data files for performance benchmarking.

## Files

### Text (English)
- text_10kb.txt, text_100kb.txt, text_1mb.txt

### CSV (Structured data)
- csv_1k_rows.csv, csv_10k_rows.csv, csv_100k_rows.csv

### JSON (API responses)
- json_100_objects.json, json_1k_objects.json, json_10k_objects.json

### XML (Configuration/data)
- xml_500_records.xml, xml_5k_records.xml

### Logs (Application logs)
- log_1k_lines.log, log_10k_lines.log, log_100k_lines.log

### Binary (Structured binary)
- binary_10kb.bin, binary_100kb.bin, binary_1mb.bin

### Source Code (Rust)
- source_100_funcs.rs, source_1k_funcs.rs

### Random (Incompressible)
- random_10kb.bin, random_100kb.bin, random_1mb.bin

## Regeneration
```bash
cargo bench --bench generate_corpus
```
"#;
    fs::write(Path::new(CORPUS_DIR).join("README.md"), readme).unwrap();
    println!("✓ Generated: {}/README.md", CORPUS_DIR);
    
    println!("\n✅ Corpus generated!\n");
}

fn dummy_bench(c: &mut Criterion) {
    c.bench_function("generate_corpus", |b| {
        b.iter(|| {
            // This is never actually called in normal bench runs
        })
    });
}

criterion_group!(benches, dummy_bench);
criterion_main!(benches);

// Test runner for generation
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    #[ignore]
    fn run_corpus_generation() {
        generate_all_corpus();
    }
}
