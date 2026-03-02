# CPAC Optimization Recommendations
*Based on Full Benchmark Analysis - March 2, 2026*

## Executive Summary

The Full benchmark revealed excellent overall performance (242.3 MB/s compress, 2.07 GB/s decompress) with room for targeted optimizations. This document outlines specific recommendations ordered by impact and implementation effort.

---

## High-Priority Optimizations (Immediate Impact)

### 1. **Parallel Block Compression for Large Files** 🔥
**Observed**: Large files (>1MB) processed sequentially
**Impact**: 2-4x throughput improvement on large files
**Effort**: Medium (already have streaming infrastructure)

**Current Performance:**
- Large logs (3.5MB): 36.6 MB/s compress
- Large JSON (690KB): 56.0 MB/s compress
- Large CSV (1.5MB): 203.6 MB/s compress

**Expected After:**
- Large logs: ~120-150 MB/s (3-4x faster)
- Large JSON: ~180-220 MB/s (3-4x faster)
- Large CSV: ~600-800 MB/s (3-4x faster)

**Implementation:**
```rust
// Use existing CPBL (CPAC Parallel Blocks) format
// Split files >1MB into blocks, compress in parallel with rayon
if data.len() > 1_048_576 {
    return compress_parallel(data, config);
}
```

### 2. **Dictionary Training for Domain-Specific Data** 🎯
**Observed**: JSON/CSV show good but improvable ratios
**Impact**: 1.2-2x better compression on structured data
**Effort**: Low (infrastructure exists in cpac-dict)

**Current Performance:**
- Large JSON: 15.40x (Zstd), 18.88x (Brotli)
- Small JSON: 96.93x (Zstd), 106.79x (Brotli)
- CSV: 3.12x-3.39x ratio

**Expected After Training:**
- Large JSON: 20-25x with trained dict
- CSV: 4-5x with schema-aware dict

**Implementation:**
- Use cpac-dict to train on sample JSON/CSV corpus
- Store common keys, patterns, schemas
- Apply dict before compression

### 3. **Improve Gzip Throughput** ⚡
**Observed**: Gzip compress slower than baseline gzip-9 on some files
**Impact**: 2-3x faster Gzip compression
**Effort**: Low (configuration tuning)

**Current vs Baseline:**
- Small CSV: CPAC 42.3 MB/s vs baseline 44.1 MB/s (similar)
- Large logs: CPAC 12.6 MB/s vs baseline 39.1 MB/s (3x slower)
- Large code: CPAC 118.3 MB/s vs baseline 728.1 MB/s (6x slower!)

**Root Cause**: Level 9 may be too aggressive for large files
**Solution**: Adaptive levels based on file size
```rust
let gzip_level = if data_size > 1_048_576 { 6 } else { 9 };
```

### 4. **Brotli Speed Optimization** 🚀
**Observed**: Excellent ratios but compress speed lags baseline on large files
**Impact**: 10-20x faster compression with similar ratios
**Effort**: Medium (quality parameter tuning)

**Current vs Baseline:**
- Large JSON: CPAC 29.7 MB/s vs baseline 0.9 MB/s (✅ 33x faster!)
- Large logs: CPAC 19.2 MB/s vs baseline 0.8 MB/s (✅ 24x faster!)
- Large code: CPAC 98.4 MB/s vs baseline 24.5 MB/s (✅ 4x faster!)
- Large CSV: CPAC 13.0 MB/s vs baseline 0.8 MB/s (✅ 16x faster!)

**Status**: ✅ Already optimized! Brotli quality=8 is excellent trade-off

---

## Medium-Priority Optimizations (Performance Gains)

### 5. **SIMD Acceleration for Transforms** 🔧
**Observed**: Preprocessing overhead visible on medium files
**Impact**: 1.5-2x faster preprocessing
**Effort**: Medium (ARM SVE infrastructure exists)

**Current State:**
- ARM SVE kernels implemented (placeholder)
- x86_64 SIMD not yet utilized
- Delta/zigzag transforms: scalar only

**Implementation:**
- Activate x86_64 AVX2/AVX-512 for delta/zigzag
- Finish ARM SVE implementation (requires nightly)
- Add runtime feature detection

### 6. **Adaptive Entropy Backend Selection Tuning** 📊
**Observed**: Some files might benefit from different backends
**Impact**: 5-10% better compression ratios
**Effort**: Low (parameter tuning)

**Current Heuristic:**
```rust
if entropy < 1.0 { Raw }
else if entropy < 6.0 { Zstd }
else { Brotli }
```

**Refinement Needed:**
- Add file size consideration
- JSON/structured → prefer Brotli even at low entropy
- Very large files → prefer Zstd for speed
- Logs → prefer Zstd (better speed/ratio balance)

**Proposed:**
```rust
match (entropy, data_size, domain_hint) {
    (_, 0..1024, _) => Raw,  // < 1KB
    (e, _, DomainHint::Json | DomainHint::Xml) if e > 4.0 => Brotli,
    (e, size, _) if size > 10_000_000 && e < 7.0 => Zstd,
    (e, _, _) if e < 1.0 => Raw,
    (e, _, _) if e < 6.0 => Zstd,
    _ => Brotli,
}
```

### 7. **Memory Pool for Frequent Allocations** 💾
**Observed**: Peak memory 125.7 MB for 54MB data (2.3x overhead)
**Impact**: 20-30% less memory, 5-10% faster
**Effort**: Medium (requires cpac-pool integration)

**Current**: Each compress/decompress allocates fresh buffers
**Proposed**: Thread-local buffer pools

**Implementation:**
- Use existing cpac-pool infrastructure
- Pool sizes: [64KB, 256KB, 1MB, 4MB]
- Reuse buffers within same thread

---

## Long-Term Optimizations (Strategic)

### 8. **GPU Acceleration via Vulkan Compute** 🎮
**Target**: Large file compression (>10MB)
**Impact**: 5-10x throughput on large files
**Effort**: High (requires Vulkan/CUDA integration)

**Best Candidates:**
- Preprocessing transforms (delta, zigzag, BWT)
- Dictionary matching
- LZ77 distance encoding

**Not Suitable for GPU:**
- Small files (< 1MB) - CPU overhead dominates
- Entropy coding (sequential dependencies)

### 9. **Context Modeling Transform** 🧠
**Target**: Structured data (JSON, XML, CSV)
**Impact**: 10-20% better compression
**Effort**: High (new transform type)

**Concept:**
- Learn patterns within data structure
- Predict next bytes based on context
- Feed predictions to entropy coder

**Expected:**
- Large JSON: 18.88x → 22-23x
- CSV: 3.39x → 4-4.5x

### 10. **Streaming Dictionary Training** 📚
**Target**: High-frequency similar data (logs, telemetry)
**Impact**: 2-3x better compression
**Effort**: Medium-High

**Concept:**
- Train dictionary online as data streams
- Update incrementally (don't retrain full corpus)
- Cache for next compression

**Best For:**
- Application logs from same service
- Time-series telemetry
- JSON APIs with similar schemas

---

## Quick Wins (Easy Implementations)

### 11. **Fix LZMA on Small Files** 🔧
**Observed**: LZMA shows 1.00x ratio on many small files
**Impact**: Proper compression on suitable files
**Effort**: Trivial (threshold check)

**Issue**: LZMA overhead exceeds benefit on tiny files
**Solution**:
```rust
Backend::Lzma => {
    if data.len() < 10_000 {
        return Err(CpacError::Other("File too small for LZMA"));
    }
    // ... existing xz_compress
}
```

### 12. **Increase Preprocessing Threshold** ⚙️
**Current**: Skip preprocessing for files < 1KB
**Observed**: Some 1-10KB files still show overhead
**Recommendation**: Increase to 4KB threshold

**Expected Impact:**
- 5-10% faster on 1-4KB files
- No ratio loss (preprocessing minimal benefit here)

### 13. **Add Compression Level Profiles** 📈
**Request**: User control over speed/ratio trade-off
**Effort**: Low (just expose existing backend levels)

**Profiles:**
```rust
pub enum CompressionProfile {
    Fastest,   // Zstd-1, Brotli-4, Gzip-1
    Balanced,  // Zstd-3, Brotli-8, Gzip-6 (current)
    Best,      // Zstd-19, Brotli-11, Gzip-9
}
```

---

## Performance Regression Risks

### Areas to Monitor

1. **Small File Overhead**
   - Current: Good performance on <1KB files
   - Risk: Adding complexity may slow these down
   - Mitigation: Keep fast-path for tiny files

2. **Preprocessing Cost**
   - Current: 1KB threshold works well
   - Risk: Complex transforms slow medium files
   - Mitigation: Profile-based transform selection

3. **Memory Growth**
   - Current: 2.3x overhead (acceptable)
   - Risk: Pooling/caching increases baseline
   - Mitigation: Lazy allocation, release on idle

---

## Benchmarking Recommendations

### Missing Test Cases

1. **Very Large Files (100MB+)**
   - Current corpus max: 3.5MB
   - Need: 100MB, 500MB, 1GB files
   - Test parallel block compression

2. **Real-World Workloads**
   - Application logs (1-10MB each)
   - Database dumps (100MB+)
   - Source code tarballs (50-200MB)
   - JSON API responses (1-100KB)

3. **Worst-Case Scenarios**
   - Already compressed data (ZIP, JPEG)
   - Random/encrypted data
   - Tiny files (10-100 bytes)

### Benchmark Automation

**Recommendation**: Add CI/CD benchmark suite
```yaml
# Run on every PR
- Quick benchmark (1 iteration, 30s)
# Run on main branch
- Full benchmark (10 iterations, 2-3 minutes)
# Run nightly
- Extended benchmark (50 iterations, large corpus)
```

---

## Implementation Priority

### Phase 1 (Next Sprint)
1. ✅ Fix Gzip adaptive levels
2. ✅ Increase preprocessing threshold to 4KB
3. ✅ Fix LZMA small file check
4. Parallel block compression (foundation)

### Phase 2 (Next Month)
5. Dictionary training integration
6. SIMD transforms (x86_64 AVX2)
7. Memory pool implementation
8. Adaptive backend tuning

### Phase 3 (Next Quarter)
9. Context modeling transform
10. GPU acceleration (Vulkan)
11. Streaming dictionary training
12. Extended benchmark suite

---

## Expected Overall Impact

**After Phase 1:**
- Compress: 242 MB/s → 350-400 MB/s (+45-65%)
- Decompress: 2.07 GB/s → 2.2-2.5 GB/s (+6-20%)
- Ratio: 3.05x → 3.2-3.4x (+5-11%)

**After Phase 2:**
- Compress: 400 MB/s → 600-800 MB/s (+50-100%)
- Decompress: 2.5 GB/s → 3.0-3.5 GB/s (+20-40%)
- Ratio: 3.4x → 3.8-4.2x (+12-24%)

**After Phase 3:**
- Compress: 800 MB/s → 2-5 GB/s (GPU: 2-6x)
- Decompress: 3.5 GB/s → 5-10 GB/s (+40-185%)
- Ratio: 4.2x → 4.5-5.0x (+7-19%)

---

*Analysis based on CPAC Full Benchmark - 10 iterations, 54MB corpus*  
*Recommendations validated against industry standards (zstd, brotli, lz4)*
