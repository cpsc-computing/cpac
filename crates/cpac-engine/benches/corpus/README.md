# Benchmark Corpus

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
