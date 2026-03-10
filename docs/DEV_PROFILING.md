# CPAC Developer Profiling Guide

## Overview

CPAC includes a built-in profiling engine that analyzes files through trial
compression, identifies compression gaps, and recommends optimal pipeline
configurations. This guide covers the profiling workflow for developers
extending CPAC with new file format support.

## Quick Start

```bash
# Profile a single file (quick mode)
cpac profile --quick myfile.json

# Profile a single file (full mode — more trials)
cpac profile myfile.json

# Profile an entire corpus
shell.ps1 profile-corpus --profile balanced --quick

# Cross-file analysis (shared patterns, dictionary gains)
shell.ps1 analyze-multi /path/to/corpus
```

## Profiling Commands

### `cpac profile <file>`

Runs multiple compression configurations against a file and reports:

- **Trial Matrix**: Compressed size, ratio, compress/decompress time for each config
- **Gap Analysis**: How much better the best config is vs CPAC default
- **Recommendations**: Actionable suggestions (enable MSN, switch backend, etc.)

Options:
- `--quick` — Fewer trials (7 configs), faster results
- Default — Full trials (14 configs), includes high-compression and preset variants

### `shell.ps1 profile-corpus`

Profiles every file in a benchmark profile's corpora, saving per-file results
to `.work/profiles/`.

Options:
- `--profile <id>` — Benchmark profile (default: `balanced`)
- `--quick` — Quick mode per file

### `shell.ps1 analyze-multi <dir>`

Cross-file pattern analysis for archive optimization:
- Byte-frequency similarity (Jensen-Shannon divergence)
- Common n-gram extraction
- Domain/track clustering
- Dictionary gain estimation (requires `pip install zstandard`)

Options:
- `--max-files N` — Limit analysis to N files (default: 200)
- `-o <file>` — Save report to file

## Development Workflow: Adding a New File Format

### Step 1: Profile the Target Files

```bash
# Gather sample files for your format
mkdir .work/benchdata/my_format
cp /samples/*.myext .work/benchdata/my_format/

# Profile them
cpac profile --quick .work/benchdata/my_format/sample1.myext
cpac profile .work/benchdata/my_format/sample2.myext
```

Review the trial matrix output. Key questions:
- Does MSN improve ratio? If yes, a domain detector would help.
- Does smart/BWT help? The file likely has repeated text patterns.
- Is brotli significantly better than zstd? Consider backend routing.
- Is there a gap between default and best? Indicates optimization opportunity.

### Step 2: Cross-File Analysis

```bash
shell.ps1 analyze-multi .work/benchdata/my_format/
```

Key metrics:
- **Byte similarity > 80%**: Files are homogeneous → solid archive mode benefits
- **Dictionary gain > 20%**: Dictionary training would help significantly
- **Common 4-grams**: Shows shared tokens that a domain detector could exploit

### Step 3: Add a Corpus Config

Create `benches/corpora/corpus_my_format.yaml`:

```yaml
id: my_format
name: My Format Corpus
description: |
  Collection of .myext files for benchmarking.
target_subdir: my_format
download_url: null
download_kind: local
license: MIT
```

### Step 4: Implement Domain Support (Optional)

If MSN showed improvement in Step 1, add a domain detector in `crates/cpac-msn/`:

1. Create `src/domains/my_format.rs`
2. Implement the `Domain` trait
3. Register in the global registry
4. Test with `cpac analyze sample.myext` to verify detection

### Step 5: Benchmark and Validate

```bash
# Create a benchmark profile including your format
# Add to benches/cpac-profiles/profile_balanced.yaml corpora list

# Run benchmarks
shell.ps1 benchmark-all --profile balanced

# Run profiling across all corpora including yours
shell.ps1 profile-corpus --profile balanced
```

### Step 6: Calibrate

If you added new transforms or domain support:

```bash
# Run transform study (if applicable)
# Then calibrate the analyzer
shell.ps1 calibrate
```

## Understanding Profile Output

### Trial Configs

| Config | Description |
|--------|-------------|
| `cpac-default` | Default pipeline (zstd-3, no MSN, no smart) |
| `cpac-msn` | Default + MSN domain extraction enabled |
| `cpac-smart` | Default + smart transform selection |
| `cpac-smart+msn` | Both MSN and smart transforms |
| `zstd-3` | Explicit zstd backend at default level |
| `brotli-6` | Brotli backend at level 6 |
| `gzip-6` | Gzip backend at level 6 |
| `zstd-high` | Zstd at CompressionLevel::High |
| `zstd-best` | Zstd at CompressionLevel::Best |
| `brotli-best` | Brotli at CompressionLevel::Best |
| `force-track1` | Force MSN on every block |
| `force-track2` | Force MSN bypass |
| `preset-maximum` | Smart + MSN + High level |
| `preset-archive` | Smart + MSN + Best level |

### Gap Analysis

The gap shows how much better the best trial is compared to default:

```
Current (default): 2.67x (56982 bytes)
Best (brotli-6):   3.27x (46505 bytes)
Gap:               +18.4%
```

A gap > 5% indicates the file type would benefit from a different default
configuration. A gap > 15% strongly suggests adding domain-specific support.

### Recommendations

Generated based on trial results:

1. **Enable MSN** — If `cpac-msn` beats `cpac-default`, the file has
   domain-specific patterns that MSN can exploit.
2. **Enable smart transforms** — If `cpac-smart` wins, the analyzer's
   SSR-based transform recommendations are beneficial.
3. **Higher compression levels** — If `zstd-best` significantly beats
   `zstd-3`, ratio-critical workloads should use `--level best`.
4. **Switch pipeline** — If the best config is very different from default,
   consider adding a file-type routing rule.

## Output Directory Structure

```
.work/
├── profiles/
│   └── profile-balanced_2026-03-09_19-30/
│       ├── summary.txt          # Aggregated profile results
│       ├── canterbury/
│       │   ├── alice29.txt.txt  # Per-file profile output
│       │   └── ...
│       └── silesia/
│           └── ...
└── benchmarks/
    └── benchmark-archive_2026-03-09_19-02/
        └── summary.txt          # Archive benchmark results
```
