# CPAC Benchmarking Guide

## Overview

CPAC includes comprehensive benchmarking against **industry-standard compression
corpora** used by compression research for decades. The benchmark suite supports
17+ corpora, 12 entropy backends, profile-driven execution, and automated
corpus downloading.

## Quick Start

### Download Corpora

```powershell
# Default set (Canterbury, Calgary, Silesia, Loghub-2k, enwik8)
.\shell.ps1 download-corpus

# All corpora for the full benchmark profile (17 corpora)
.\shell.ps1 download-corpus --profile full

# Specific corpora
.\shell.ps1 download-corpus --corpus "canterbury,silesia,enwik9"
```

### Run Benchmarks

```powershell
# Quick benchmark (< 2 minutes)
.\shell.ps1 benchmark-all --profile quick

# Balanced benchmark (default, ~10 minutes)
.\shell.ps1 benchmark-all

# Full benchmark (17 corpora, 10 iterations, ~30-60 minutes)
.\shell.ps1 benchmark-all --profile full
```

### Single File Benchmark

```bash
cpac benchmark .work/benchdata/canterbury/alice29.txt --quick
cpac benchmark .work/benchdata/silesia/dickens
```

## Industry-Standard Corpora

### Canterbury Corpus

**Citation**: Ross Arnold and Timothy Bell, "A corpus for the evaluation of
lossless compression algorithms," Proceedings of DCC'97, Snowbird, Utah, 1997.

- **Files**: 11 diverse files (text, C source, HTML, Lisp, Excel, binary)
- **Total Size**: ~2.8 MB
- **Use Case**: Classic benchmark since 1997, quick validation
- **License**: Public domain

### Silesia Corpus

**Citation**: Silesian University of Technology, "Silesia Compression Corpus."
Available at: https://sun.aei.polsl.pl/~sdeor/index.php?page=silesia

- **Files**: 12 mixed-content files
- **Total Size**: ~211 MB
- **Use Case**: Industry standard for realistic performance testing
- **License**: Freely available for research

### Calgary Corpus

**Citation**: University of Calgary, "Calgary Compression Corpus."

- **Files**: 18 text-heavy files
- **Total Size**: ~3.2 MB
- **Use Case**: Classic text compression benchmark
- **License**: Public domain

### Additional Corpora (Full Profile)

The full benchmark profile includes 17 corpora:

| Corpus | Size | Description |
|--------|------|-------------|
| canterbury | 2.8 MB | Classic benchmark (11 files) |
| calgary | 3.2 MB | Classic text benchmark (18 files) |
| silesia | 211 MB | Mixed content (12 files) |
| enwik9 | 1 GB | Wikipedia XML (Large Text Compression Benchmark) |
| loghub2_2k | 3.7 MB | Loghub-2.0 2k samples (14 log types) |
| loghub2_full | ~12.7 GB | Loghub-2.0 complete dataset |
| kodak | 14.7 MB | Kodak image set (24 PNG) |
| nasa_logs | 391 MB | NASA HTTP access logs (Jul/Aug 1995) |
| digitalcorpora | ~5.3 GB | Govdocs1 government documents |
| cloud_configs | 308 MB | Cloud config files (YAML/JSON/TOML) |
| docker_layers | ~10 MB | Alpine Linux minirootfs tarballs |
| database_dumps | ~692 MB | PostgreSQL, MySQL, MongoDB samples |
| json_apis | ~1.1 GB | OpenAPI specs, CloudFormation, Azure templates |
| github_events_large | ~2.9 GB | GitHub Archive event data (6 hours) |
| musan | ~12 GB | Music, Speech, and Noise audio corpus |
| vctk | ~11.2 GB | English multi-speaker speech corpus |
| librispeech | ~6.3 GB | LibriSpeech ASR corpus (train-clean-100) |

All corpus configurations are in `benches/cpac/corpora/corpus_*.yaml`.

## Latest Benchmark Results

### Session 21 — Full 8-Corpus Benchmark (Post Phase 1–6 Optimizations)

**Date:** March 2026 | **Version:** 0.3.0 | **Platform:** Windows x86_64, Rust 1.93+
**Methodology:** Balanced profile, 3 iterations, 12 entropy backends, 777 files
across 8 standard corpora. All results verified lossless.

#### Per-Corpus Best Compression Ratios

| Corpus | Files | Size | Avg Best | Median | Max | Min |
|--------|-------|------|----------|--------|-----|-----|
| loghub2_2k | 14 | 3.7 MB | **16.63×** | 15.55× | 32.63× | 6.97× |
| nasa_logs | 4 | 391.4 MB | **8.56×** | 8.50× | 16.23× | 1.00× |
| canterbury | 11 | 2.7 MB | **5.84×** | 3.56× | 20.96× | 2.86× |
| silesia | 12 | 202.1 MB | **4.30×** | 3.55× | 12.42× | 1.63× |
| calgary | 18 | 3.1 MB | **4.03×** | 3.38× | 12.56× | 1.85× |
| enwik8 | 1 | 95.4 MB | **3.75×** | — | — | — |
| cloud_configs | 691 | 3.0 MB | **3.63×** | 2.87× | 17.98× | 0.00× |
| kodak | 26 | 14.7 MB | **1.08×** | 1.00× | 2.15× | 1.00× |

#### Average Zstd Compression Ratio by CPAC Level

| Corpus | Fast | Default | High | Best |
|--------|------|---------|------|------|
| loghub2_2k | 11.04× | 12.87× | 13.61× | **15.25×** |
| nasa_logs | 5.41× | 6.30× | 7.09× | **8.41×** |
| canterbury | 3.96× | 4.17× | 4.22× | **5.06×** |
| silesia | 3.17× | 3.46× | 3.71× | **4.04×** |
| calgary | 3.20× | 3.39× | 3.47× | **3.67×** |
| enwik8 | 2.82× | 3.06× | 3.27× | **3.64×** |
| cloud_configs | 2.84× | 3.03× | 3.08× | **3.14×** |

#### Key Findings

- **Brotli@11 is the peak-ratio backend** across most corpora (except Silesia
  where Xz wins, and Kodak where Raw is best for near-incompressible images).
- **SSR vs MSN routing produces near-identical ratios** — SSR is a fast
  approximation of MSN with 20–50% higher throughput.
- **Kodak images are near-incompressible** (1.05×) as expected.
- **All results verified roundtrip** across all measurements.

### Session 22–30 — Infrastructure Improvements

Sessions 22–30 focused on benchmark infrastructure hardening:

- **Session 22**: Balanced benchmark 774/777 OK. MSN disabled by default
  (zero-copy `Cow` optimization).
- **Session 23**: Full benchmark 776/776 OK, 0 timeouts.
- **Session 30**: Fixed benchmark timeouts (enwik8, NASA raw logs), added
  large/very-large file level tiering, `.gz` exclusion for NASA corpus, fixed
  YAML inline-comment parser bug. Full profile: **776/776 OK, 0 timeouts,
  0 failures**.

For session-by-session details, see `LEDGER.md`.

## Benchmark Profiles

| Profile | Corpora | Iterations | Timeout | Use Case |
|---------|---------|------------|---------|----------|
| `quick` | 3 (canterbury, calgary, loghub2_2k) | 1 | 600s | Fast validation |
| `balanced` | 8 (standard set) | 3 | 7200s | Default, reliable |
| `full` | 17 (all corpora) | 10 | 14400s | Comprehensive evaluation |

Profile configs: `benches/cpac/profiles/profile_*.yaml`.

### Large File Handling

Profiles support tiered level selection for large files:

- **>15 MB**: Skip High level (use `[fast, default, best]`)
- **>100 MB**: Only `[fast, best]` levels
- Reduces timeout risk without losing coverage of extreme levels

## Running Your Own Benchmarks

### Prerequisites

```powershell
# Download corpora
.\shell.ps1 download-corpus --profile balanced

# Build release binary
.\shell.ps1 build --release
```

### Quick Benchmarks (Individual Files)

```bash
cpac benchmark .work/benchdata/canterbury/alice29.txt --quick
cpac benchmark .work/benchdata/silesia/mozilla --quick
cpac benchmark .work/benchdata/silesia/xml
```

### Criterion Microbenchmarks

```bash
cargo bench -p cpac-engine
cargo bench -p cpac-engine --bench compress
cargo bench -p cpac-engine --bench simd
cargo bench -p cpac-engine --bench dag
```

## Compression Presets

| Preset | zstd Level | Transforms | Block Size |
|--------|-----------|------------|------------|
| Turbo | zstd-1 (Fast) | OFF | 1 MB |
| Balanced | zstd-3 (Default) | Smart ON | 4 MB |
| Maximum | zstd-12 (High) | Smart ON | 8 MB |
| Archive | zstd-19 (Best) | Smart + aggressive | 16 MB |

See [TRANSFORMS.md](TRANSFORMS.md) for the full transform pipeline
documentation including SSR gates, confidence thresholds, and calibration data.

## License and Citation

All corpus data is used under respective licenses:
- **Canterbury/Calgary**: Public domain
- **Silesia**: Freely available for research
- **Loghub-2.0**: Research license (see corpus YAML)
- **Others**: See individual corpus YAML configs in `benches/cpac/corpora/`

When publishing results using CPAC benchmarks, please cite:
1. The relevant corpus (Canterbury, Silesia, etc.)
2. CPAC repository: https://github.com/cpsc-computing/cpac

---

**Last Updated**: 2026-03-15 (Session 30)
**CPAC Version**: 0.3.0
**Benchmark Suite Version**: 3.0
