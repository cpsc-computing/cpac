# OpenZL / CoMERA Superiority Analysis

## Methodology

This document defines the objective comparison framework between CPAC (via the
OpenZL initiative) and competing datacenter compression systems, including
CoMERA/Meta-style neural compressors, Zstd, Brotli, LZ4, and Snappy.

### Comparison Dimensions

1. **Compression Ratio** — bytes-in / bytes-out across corpus categories
2. **Throughput** — MB/s compress and decompress (single-thread and multi-thread)
3. **CPU Efficiency** — CPU cycles per input byte (perf stat / RDTSC)
4. **Memory Footprint** — peak RSS during compression/decompression
5. **Multi-Thread Scaling** — speedup factor at 2/4/8/16/24 threads
6. **Latency Distribution** — p50/p95/p99 per-block compress time
7. **Quality of Service** — ratio variance across file types
8. **Feature Coverage** — encryption, streaming, dedup, MSN, PQC, cloud upload

### Corpus Categories

- DC-Logs: JSON/syslog/access logs (text, high redundancy)
- DC-Configs: YAML/TOML/JSON/XML configs (structured, small)
- DC-Mixed: code + docs + binaries (heterogeneous)
- DC-Large: DB dumps, VM images, backups (>100 MB, sequential)
- DC-Dedup: versioned datasets with high duplication
- DC-Streaming: Kafka-style message batches

### Benchmark Procedure

1. Generate or collect corpus data per category (min 10 files each)
2. Run `shell.ps1 benchmark-external` with quick/default/full profile
3. Run `shell.ps1 benchmark-all` for internal metrics
4. Collect system counters (CPU cycles, RSS, thread utilisation)
5. Produce comparison tables (CSV → markdown via `scripts/gen-report.py`)

## Expected Advantage Areas

### CPAC Strengths (vs Zstd/Brotli/LZ4)
- **MSN**: 20-50% better ratio on structured text (JSON/CSV/YAML)
- **Adaptive pipeline**: SSR → backend selection → DAG transforms
- **Dedup-aware**: CDC chunking eliminates redundant blocks
- **PQC encryption**: zero-overhead encryption pipeline
- **Streaming**: bounded-memory block processing

### Areas to Monitor
- Raw throughput on incompressible data (binary/encrypted)
- Compression ratio on small files (<4 KB)
- Memory overhead from MSN detection phase
- Warm-up cost of thread pool initialisation

## Report Template

### Per-Category Summary

For each category, generate:

```
Category: DC-Logs
Files: 42 | Total size: 1.2 GB
                Ratio   Comp MB/s   Decomp MB/s   Peak RSS MB
  CPAC          12.4x   450         1200           64
  Zstd-3        8.2x    520         1400           32
  Brotli-6      9.1x    120         450            48
  LZ4           4.5x    1800        3500           16
  Gzip          7.8x    80          350            24
```

### Aggregate Summary

Geometric mean across all categories:

```
                GMean Ratio   GMean Comp   GMean Decomp
  CPAC          6.8x          380          1100
  Zstd-3        5.2x          480          1300
  Brotli-6      5.8x          110          420
```

### Multi-Thread Scaling

```
Threads:    1      2      4      8      16     24
  CPAC      380    720    1350   2400   3200   3600
  Zstd      480    900    1600   2800   3400   3800
```

## Generating the Report

```bash
# Run benchmarks
./shell.ps1 benchmark-external --corpus benches/openzl/corpora --profile full

# Generate markdown report
python3 scripts/gen-report.py benchmark-external-results.csv > docs/BENCHMARK_REPORT.md
```
