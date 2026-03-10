# CPAC User Manual

**Version**: 0.1.0
**Copyright (c) 2026 BitConcepts, LLC. All rights reserved.**
**License**: LicenseRef-CPAC-Research-Evaluation-1.0

---

## 1. Introduction

CPAC (Constraint-Projected Adaptive Compression) is a high-performance compression
engine that adapts its pipeline to the structure of your data. It combines:

- **Structural analysis** (SSR) — lightweight heuristic gating in <1 ms
- **Semantic extraction** (MSN) — domain-aware normalization for JSON, CSV, XML, logs, and more
- **27 composable transforms** — delta, zigzag, transpose, ROLZ, tokenize, dedup, and others
- **Multiple entropy backends** — Zstd, Brotli, Gzip, LZMA, Raw
- **Post-quantum encryption** — X25519 + ML-KEM-768 hybrid (FIPS 203), ML-DSA-65 signatures (FIPS 204)
- **Hardware acceleration** — Intel QAT/IAA, AMD Xilinx FPGA, GPU compute, ARM SVE2
- **Parallel block compression** — scales across all CPU cores

CPAC outputs `.cpac` files (single), `.cpac-stream` (streaming), `.cpar` (archive),
and `.cpac-enc` (encrypted) formats.

---

## 2. Installation

### From Source (Rust)

```bash
# Clone and build (release mode, optimized)
git clone <repo-url> && cd cpac
cargo build --release --features pqc

# Binary is at target/release/cpac (or cpac.exe on Windows)
```

### Shell Completions

```bash
# Bash
cpac completions bash > ~/.local/share/bash-completion/completions/cpac

# Zsh
cpac completions zsh > ~/.zsh/completions/_cpac

# PowerShell
cpac completions powershell | Out-String | Invoke-Expression

# Fish
cpac completions fish > ~/.config/fish/completions/cpac.fish
```

---

## 3. Quick Start

```bash
# Compress a file
cpac compress data.json

# Decompress
cpac decompress data.json.cpac

# Compress with MSN for structured data (JSON, CSV, XML, logs)
cpac compress --enable-msn --smart data.json

# View file info
cpac info data.json

# View host capabilities (CPU, SIMD, accelerators)
cpac info --host

# Benchmark a file
cpac benchmark data.json --quick
```

---

## 4. Commands Reference

### 4.1 `cpac compress` (alias: `c`)

Compress one or more files.

```
cpac compress [OPTIONS] <INPUT>
```

**Positional arguments:**
- `INPUT` — File path, or `-` for stdin. If a directory, use `-r` to recurse.

**Key options:**

| Flag | Description | Default |
|------|-------------|---------|
| `-o, --output` | Output path (or `-` for stdout) | `<input>.cpac` |
| `-b, --backend` | Entropy backend: `raw`, `zstd`, `brotli`, `gzip`, `lzma` | auto |
| `-f, --force` | Overwrite existing output | off |
| `-r, --recursive` | Recurse into directories | off |
| `-v` | Verbosity (`-v`, `-vv`, `-vvv`) | quiet |
| `-T, --threads` | Worker threads (`0` = auto) | 0 |
| `-M, --max-memory` | Memory cap in MB (`0` = auto: 25% RAM) | 0 |
| `--mmap` | Force memory-mapped I/O | auto for >64 MB |
| `--level` | `fast`, `default`, `best` | `default` |
| `--smart` | Enable data-driven transform selection | off |
| `--preset` | Named preset (see §5) | none |
| `--accel` | HW accelerator: `auto`, `software`, `qat`, `iaa`, `gpu`, `fpga`, `sve2` | `auto` |
| `--dict` | Pre-trained dictionary (`.cpac-dict`) | none |
| `--streaming` | Streaming mode (bounded memory) | off |
| `--stream-block` | Streaming block size in bytes | 1 MiB |

**MSN options:**

| Flag | Description | Default |
|------|-------------|---------|
| `--enable-msn` | Enable Multi-Scale Normalization | off |
| `--msn-confidence` | Minimum confidence threshold (0.0-1.0) | 0.5 |
| `--msn-domain` | Force domain (e.g., `text.json`, `log.apache`) | auto |

**Encryption options (produces `.cpac-enc`):**

| Flag | Description | Default |
|------|-------------|---------|
| `--encrypt` | Encrypt output (password or PQC) | off |
| `--encrypt-key` | PQC public key file (`.cpac-pub`) | none (password) |
| `--encrypt-algo` | `chacha20` or `aes256gcm` | `chacha20` |

**Examples:**

```bash
# Standard compression
cpac compress report.csv

# Maximum ratio for archival
cpac compress --preset max-ratio --enable-msn database_dump.json

# Streaming compression for very large files
cpac compress --streaming --stream-block 4194304 huge_log.txt

# Encrypt with password
CPAC_PASSWORD=mysecret cpac compress --encrypt sensitive.csv

# Encrypt with PQC keys
cpac compress --encrypt --encrypt-key recipient.cpac-pub confidential.pdf

# Compress all files in a directory
cpac compress -r ./logs/ -f
```

### 4.2 `cpac decompress` (aliases: `d`, `x`)

Decompress a CPAC file.

```
cpac decompress [OPTIONS] <INPUT>
```

| Flag | Description |
|------|-------------|
| `-o, --output` | Output path | 
| `-f, --force` | Overwrite existing |
| `--streaming` | Streaming decompression |
| `--encrypt-key` | Secret key for PQC decryption (`.cpac-sec`) |

Password-encrypted files read `CPAC_PASSWORD` env var or prompt interactively.

```bash
# Basic decompression
cpac decompress data.json.cpac

# Decrypt and decompress PQC-encrypted file
cpac decompress --encrypt-key my.cpac-sec encrypted.cpac-enc
```

### 4.3 `cpac info` (alias: `i`)

Show file structure or host system info.

```bash
# File analysis
cpac info data.json
# Output: size, track, viability, entropy, ASCII ratio, domain hint

# Host info
cpac info --host
# Output: CPU, cores, RAM, SIMD extensions, accelerators, env var hints
```

### 4.4 `cpac benchmark` (alias: `bench`)

Benchmark compression performance.

```bash
# Quick: 3 iterations, <10s
cpac benchmark --quick data.json

# Default: 10 iterations
cpac benchmark data.json

# Full: 50 iterations, all baselines
cpac benchmark --full data.json

# Compare CPAC Track 1 vs raw backends
cpac benchmark --track1 data.json

# JSON output for automation
cpac benchmark --json data.json
```

| Flag | Description |
|------|-------------|
| `--quick` | 3 iterations, 2 baselines |
| `--full` | 50 iterations, 4 baselines |
| `--skip-baselines` | Skip gzip/zstd/brotli/lzma baselines |
| `--track1` | Also benchmark SSR auto-routing |
| `--discovery` | Compare MSN-on vs MSN-off ceiling/floor |
| `--json` | Machine-readable output |

### 4.5 `cpac analyze` (alias: `a`)

Analyze a file and recommend compression strategy.

```bash
cpac analyze server.log
# Output: byte frequency, entropy profile, track recommendation,
#         domain detection, suggested transforms
```

### 4.6 `cpac profile` (alias: `p`)

Run a trial compression matrix with gap analysis.

```bash
cpac profile --quick data.json
# Output: trial matrix (14 configs), gap analysis, recommendations
```

### 4.7 `cpac encrypt` / `cpac decrypt`

Standalone encryption (password-based AEAD, without compression).

```bash
# Encrypt
cpac encrypt sensitive.doc -a chacha20
# Prompts for password or reads CPAC_PASSWORD

# Decrypt
cpac decrypt sensitive.doc.cpac-enc -a chacha20
```

### 4.8 Archives (`archive-create`, `archive-extract`, `archive-list`)

Multi-file archive support via the CPAR format.

```bash
# Create archive
cpac archive-create ./project/ -o project.cpar

# Create solid archive (better ratio for similar files)
cpac archive-create --solid ./configs/ -o configs.cpar

# List contents
cpac archive-list project.cpar

# Extract
cpac archive-extract project.cpar -o ./restored/
```

### 4.9 Post-Quantum Cryptography (`cpac pqc`)

PQC operations using X25519 + ML-KEM-768 hybrid encryption and ML-DSA-65 signatures.

```bash
# Generate hybrid key pair
cpac pqc keygen -o ./keys/
# Creates: cpac-keypair.cpac-pub and cpac-keypair.cpac-sec

# Hybrid-encrypt a file
cpac pqc encrypt report.pdf -k recipient.cpac-pub

# Hybrid-decrypt
cpac pqc decrypt report.pdf.cpac-pqe -k my.cpac-sec

# Sign a file (ML-DSA-65)
cpac pqc sign release.tar -k my.cpac-sec
# Creates: release.tar.cpac-sig

# Verify signature
cpac pqc verify release.tar -s release.tar.cpac-sig -k signer.cpac-pub
```

### 4.10 Transform Laboratory (`cpac lab`)

Developer tools for transform calibration.

```bash
# Calibrate analyzer from benchmark CSV results
cpac lab calibrate --dir .work/benchmarks/transform-study/
```

---

## 5. Presets

Presets auto-configure level, transforms, MSN, block size, and threading.

| Preset | Level | Smart | MSN | Use Case |
|--------|-------|-------|-----|----------|
| `turbo` | Fast | off | off | Maximum throughput, real-time pipelines |
| `balanced` | Default | on | off | General purpose, good ratio/speed balance |
| `maximum` | High | on | on | Best ratio with reasonable speed |
| `archive` | Best | on | on | Cold storage, archival workloads |
| `max-ratio` | Best | on | on | Absolute best ratio (Brotli-11, 32 MB blocks) |

```bash
cpac compress --preset archive big_dataset.tar
cpac compress --preset turbo streaming_logs.jsonl
```

Individual flags override preset values:
```bash
# Archive preset but force zstd backend
cpac compress --preset archive --backend zstd data.bin
```

---

## 6. Entropy Backends

| Backend | Best For | Ratio | Speed |
|---------|----------|-------|-------|
| `zstd` | General purpose, fast decompression | Good | Fast |
| `brotli` | Maximum ratio, web content | Best | Moderate |
| `gzip` | Compatibility, interop | OK | Moderate |
| `lzma` | High ratio, archival | Very Good | Slow |
| `raw` | Testing, pre-compressed data | 1:1 | Instant |

CPAC's SSR analysis automatically selects the optimal backend if none is specified.

---

## 7. Multi-Scale Normalization (MSN)

MSN extracts domain-specific patterns before entropy coding, dramatically improving
ratio on structured data.

### Supported Domains

| Domain | Category | Auto-Detected |
|--------|----------|---------------|
| JSON / JSONL | text.json | Yes |
| CSV | text.csv | Yes |
| XML / HTML | text.xml | Yes |
| MessagePack | binary.msgpack | Yes |
| CBOR | binary.cbor | Yes |
| Protobuf | binary.protobuf | Yes |
| Syslog | log.syslog | Yes |
| Apache access log | log.apache | Yes |
| JSON log (structured) | log.json_log | Yes |

### When to Enable MSN

- **Always enable** for homogeneous structured data (JSON APIs, CSV exports, logs)
- **No benefit** for binary blobs, images, executables, pre-compressed data
- Use `cpac analyze <file>` to check if MSN would help

```bash
# Auto-detect domain
cpac compress --enable-msn api_responses.jsonl

# Force specific domain
cpac compress --enable-msn --msn-domain log.apache access.log
```

---

## 8. Streaming Mode

For files that exceed available RAM or require bounded memory:

```bash
# Compress with streaming (1 MiB blocks)
cpac compress --streaming large_dataset.bin

# Custom block size (4 MiB)
cpac compress --streaming --stream-block 4194304 huge.log

# Decompress streaming file
cpac decompress --streaming large_dataset.bin.cpac-stream
```

Streaming mode produces `.cpac-stream` files with independently-decompressible blocks.
Each block goes through the full CPAC pipeline (SSR → MSN → transforms → entropy).

---

## 9. Encryption

### Password-Based Encryption

Uses Argon2id key derivation with ChaCha20-Poly1305 or AES-256-GCM.

```bash
# Via environment variable (recommended for scripts)
export CPAC_PASSWORD="strong-password-here"
cpac compress --encrypt data.csv

# Interactive prompt
cpac compress --encrypt data.csv
# Password: ********

# Decrypt
export CPAC_PASSWORD="strong-password-here"
cpac decompress data.csv.cpac-enc
```

### Post-Quantum Hybrid Encryption

Defence-in-depth: combines X25519 (classical) + ML-KEM-768 (NIST FIPS 203, post-quantum).
Even if one primitive is broken, the other still protects confidentiality.

```bash
# One-time: generate key pair
cpac pqc keygen -o ./keys/

# Encrypt for a recipient
cpac compress --encrypt --encrypt-key recipient.cpac-pub secret.json

# Recipient decrypts
cpac decompress --encrypt-key my.cpac-sec secret.json.cpac-enc
```

### Digital Signatures (ML-DSA-65)

Post-quantum signatures using NIST FIPS 204.

```bash
cpac pqc sign release.tar -k my.cpac-sec
cpac pqc verify release.tar -s release.tar.cpac-sig -k publisher.cpac-pub
```

---

## 10. Hardware Acceleration

CPAC detects and uses available hardware accelerators automatically.

| Accelerator | Platform | Env Var |
|-------------|----------|---------|
| Intel QAT | Xeon (data centers) | `CPAC_QAT_ENABLED=1` |
| Intel IAA | Sapphire Rapids+ | `CPAC_IAA_ENABLED=1` |
| GPU Compute | CUDA/Vulkan | `CPAC_GPU_ENABLED=1` |
| AMD Xilinx | Alveo FPGA | `CPAC_XILINX_ENABLED=1` |
| ARM SVE2 | AArch64 (Graviton3+) | `CPAC_SVE2_ENABLED=1` |

```bash
# Check detected accelerators
cpac info --host

# Force specific accelerator
cpac compress --accel qat large_dataset.bin

# Force software-only (disable HW accel)
cpac compress --accel software data.bin
```

SIMD transform dispatch is automatic: AVX-512 → AVX2 → SSE4.1 → SSE2 → NEON → scalar.

---

## 11. Dictionaries

Pre-trained dictionaries improve compression on homogeneous corpora by up to 20-40%.

```bash
# Train a dictionary (via Python tooling)
python cpac.py train-dict --corpus ./logs/ --output logs.cpac-dict

# Compress with dictionary
cpac compress --dict logs.cpac-dict new_log.txt

# Auto-dictionary from benchmarks
cpac compress data.json  # Automatically checks .work/benchmarks/
cpac compress --no-auto-dict data.json  # Disable auto-selection
```

---

## 12. Cloud and Data Center Integration

### Resource Tuning

CPAC auto-tunes for the available hardware:

```bash
# Auto (recommended): physical cores, 25% RAM cap
cpac compress data.bin

# Constrain for containerized environments
cpac compress -T 4 -M 512 data.bin  # 4 threads, 512 MB cap
```

### Batch Processing

```bash
# Compress all files in a directory
cpac compress -r ./data/ -f

# Create archive from directory
cpac archive-create ./daily_logs/ -o daily.cpar --solid
```

### Encrypted Pipeline (CPCE Wire Format)

Compress-then-encrypt in a single pipeline:

```bash
# Password mode
CPAC_PASSWORD=$VAULT_SECRET cpac compress --encrypt --streaming incoming.csv

# PQC mode
cpac compress --encrypt --encrypt-key service.cpac-pub --streaming data.json
```

### Memory-Mapped I/O

For files larger than 64 MB, CPAC automatically uses mmap for zero-copy reads.
Force with `--mmap`, or disable by omitting the flag (falls back to read()).

### Environment Variables

| Variable | Purpose |
|----------|---------|
| `CPAC_PASSWORD` | Password for encrypt/decrypt (avoids prompt) |
| `CPAC_QAT_ENABLED` | Enable Intel QAT acceleration |
| `CPAC_IAA_ENABLED` | Enable Intel IAA acceleration |
| `CPAC_GPU_ENABLED` | Enable GPU compute |
| `CPAC_XILINX_ENABLED` | Enable AMD Xilinx FPGA |
| `CPAC_SVE2_ENABLED` | Enable ARM SVE2 |

---

## 13. Wire Formats

CPAC uses several wire formats for different purposes:

| Format | Magic | Extension | Purpose |
|--------|-------|-----------|---------|
| CP | `"CP"` | `.cpac` | Standard single-file frame |
| CPBL | `"CPBL"` | `.cpac` | Block-parallel frame (auto for large files) |
| CS | `"CS"` | `.cpac-stream` | Streaming frame (bounded memory) |
| CPAR | `"CPAR"` | `.cpar` | Multi-file archive |
| CPHE | `"CPHE"` | `.cpac-pqe` | PQC hybrid encryption |
| CPCE | `"CPCE"` | `.cpac-enc` | Compressed+encrypted |

All formats use little-endian byte ordering. See `docs/SPEC.md` for full wire format
specification.

---

## 14. Troubleshooting

### "Compression failed" on specific files
```bash
# Check file structure
cpac info problematic_file
cpac analyze problematic_file

# Try different backend
cpac compress --backend zstd problematic_file

# Disable transforms
cpac compress --level fast problematic_file
```

### Low compression ratio
```bash
# Enable MSN for structured data
cpac compress --enable-msn --smart data.json

# Use higher compression level
cpac compress --level best data.json

# Profile to find optimal config
cpac profile data.json
```

### Out of memory
```bash
# Use streaming mode for large files
cpac compress --streaming large.bin

# Constrain memory
cpac compress -M 1024 large.bin

# Reduce thread count
cpac compress -T 2 -M 512 large.bin
```

### Decryption fails
- Ensure `CPAC_PASSWORD` matches the encryption password exactly
- For PQC: ensure you're using the matching `.cpac-sec` key file
- Check file integrity: the `.cpac-enc` file may be truncated

### Accelerator not detected
```bash
# Check what's available
cpac info --host

# Set environment variable
export CPAC_QAT_ENABLED=1
cpac compress --accel qat data.bin
```

---

## 15. Cookbook: Common Workflows

### Log Compression Pipeline

```bash
# Daily log rotation with encryption
CPAC_PASSWORD=$LOG_SECRET \
  cpac compress --enable-msn --msn-domain log.apache \
    --preset archive --encrypt /var/log/access.log

# Compress all rotated logs
cpac compress -r --enable-msn --preset maximum /var/log/archived/
```

### API Response Archival

```bash
# JSON API dumps with maximum ratio
cpac compress --enable-msn --preset max-ratio api_dump.jsonl

# Archive multiple API snapshots
cpac archive-create --solid ./api_snapshots/ -o api_archive.cpar
```

### Database Backup Pipeline

```bash
# Compress + PQC encrypt a database dump
cpac compress --streaming --enable-msn --encrypt \
  --encrypt-key backup-key.cpac-pub dump.sql

# Verify integrity after restore
cpac pqc verify dump.sql -s dump.sql.cpac-sig -k publisher.cpac-pub
cpac decompress --encrypt-key restore-key.cpac-sec dump.sql.cpac-enc
```

### CI/CD Artifact Compression

```bash
# Fast compression for build artifacts
cpac compress --preset turbo build_output.tar

# Sign release artifacts
cpac pqc sign release-v1.2.3.tar -k ci.cpac-sec
```

### Benchmarking Workflow

```bash
# Quick benchmark to check improvement
cpac benchmark --quick data.json

# Full benchmark for reporting
cpac benchmark --full --json data.json > bench_results.json

# Profile to identify optimization gaps
cpac profile data.json
```

---

## 16. Security Considerations

- **Password entropy**: Use strong passwords (≥20 characters). Argon2id provides
  memory-hard key derivation to resist brute force.
- **Key management**: Store `.cpac-sec` files securely. Never share secret keys.
  Public keys (`.cpac-pub`) can be freely distributed.
- **Post-quantum readiness**: CPAC's hybrid encryption uses both classical (X25519)
  and post-quantum (ML-KEM-768) primitives. If either is broken, the other still
  protects your data.
- **Algorithm choices**:
  - ChaCha20-Poly1305 — Recommended for most use cases (fast, constant-time)
  - AES-256-GCM — Preferred when hardware AES-NI is available
- **Signature verification**: Always verify signatures before trusting content.
  ML-DSA-65 provides NIST Level 3 post-quantum security.

---

## 17. File Extensions

| Extension | Format | Description |
|-----------|--------|-------------|
| `.cpac` | CP / CPBL | Compressed file |
| `.cpac-stream` | CS | Streaming compressed file |
| `.cpac-enc` | CPCE | Compressed + encrypted file |
| `.cpac-pqe` | CPHE | PQC hybrid encrypted file |
| `.cpac-sig` | — | Digital signature (ML-DSA-65) |
| `.cpac-pub` | — | Public key (X25519 + ML-KEM-768) |
| `.cpac-sec` | — | Secret key (X25519 + ML-KEM-768) |
| `.cpac-dict` | — | Pre-trained compression dictionary |
| `.cpar` | CPAR | Multi-file archive |

---

## 18. Getting Help

```bash
# General help
cpac --help

# Command-specific help
cpac compress --help
cpac pqc --help

# Version
cpac --version
```

For bug reports and feature requests: info@bitconcepts.tech
