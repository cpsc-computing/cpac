# OpenZL Datacenter Benchmark Corpora

This directory holds corpus data for datacenter-class benchmarks targeting
the OpenZL comparison track. Subdirectories map to the profiles defined in
`benches/openzl/profiles/openzl_profile_dc.yaml`.

## Expected Subdirectories

| Directory | Content |
|-----------|---------|
| `logs/` | JSON, syslog, access logs |
| `configs/` | YAML, TOML, JSON, XML configuration files |
| `mixed/` | Heterogeneous: code, docs, binaries, images |
| `large/` | DB dumps, VM images (>100 MB each) |
| `dedup/` | Versioned / backup data with heavy duplication |
| `streaming/` | Kafka-style message batches |

## Populating

Corpus data is **not** committed to git.  Populate via:

```
shell.ps1 download-corpus --corpus openzl_dc
```

Or manually place representative files into the subdirectories above.
