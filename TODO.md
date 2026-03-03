# CPAC TODO & Known Issues

## 🚨 Critical Issues

### Test Isolation Bug (Session 10)
**Status:** Active Investigation Required  
**Priority:** High  
**Impact:** CI/CD reliability

**Symptoms:**
- `corpus_csv` and `corpus_large_csv` tests fail when run with full workspace
- Pass when run individually: `cargo test corpus_csv --exact` ✅
- Fail with size mismatch (~1000 bytes extra) when run after other tests
- Decompressed size doesn't match expected (38182 vs 37182 for corpus_csv)

**Context:**
- Introduced during Session 10 MSN streaming implementation
- Only affects CSV corpus tests in cpac-engine
- Non-streaming compression/decompression works correctly
- Suggests global state pollution or test ordering dependency

**Next Steps:**
1. Investigate global registry state in cpac-msn
2. Check for mutable static variables or lazy_static pollution
3. Add test isolation guards or reset mechanisms
4. Consider adding test fixtures that restore global state

---

## ⚠️ Known Limitations

### JsonLogDomain Streaming (Session 10)
**Status:** Documented, Workaround In Place  
**Priority:** Medium

**Issue:**
- JsonLogDomain not safe for arbitrary block boundaries
- Blocks can split mid-line in newline-separated JSON
- Causes corruption when reconstruction tries to expand incomplete lines

**Current Solution:**
- `extract_with_fields()` disabled for streaming (returns error)
- Falls back to passthrough compression
- Still provides compression but without MSN benefits

**Future Work:**
- Implement line-aligned blocking strategy
- Add buffering for incomplete lines between blocks
- Consider streaming-specific domain variant

### CsvDomain Streaming (Session 10)
**Status:** Documented, Workaround In Place  
**Priority:** Medium

**Issue:**
- CSV blocks after first lack header row
- Can't extract headers from headerless blocks
- Detection-phase headers don't apply to data-only blocks

**Current Solution:**
- `extract_with_fields()` disabled for streaming (returns error)
- Falls back to passthrough compression

**Future Work:**
- Implement header-less extraction for data rows
- Use detection-phase column positions
- Optimize for streaming CSV with consistent schema

### Large File Frame Errors (Session 9)
**Status:** Identified, Needs Investigation  
**Priority:** High

**Issue:**
- "Invalid frame version" errors on files >5 MB
- Affects CPAC backends (zstd, brotli, gzip)
- Parallel compression path suspected

**Examples:**
- mozilla (51 MB): Frame error
- xml (5 MB): Frame error
- dickens (10 MB): Baseline works, CPAC TBD

**Next Steps:**
- Debug parallel compression frame encoding
- Verify block assembly in CPBL format
- Test with various block sizes

---

## 📋 Future Enhancements

### MSN Streaming Optimizations
**Priority:** Low  
**Session:** 10+

- [ ] Implement XML domain `extract_with_fields()`
- [ ] Implement YAML domain `extract_with_fields()`  
- [ ] Add line-aligned blocking for text-based domains
- [ ] Optimize JsonDomain for JSONL format
- [ ] Per-block metadata verification
- [ ] Streaming MSN benchmarks

### Benchmark Infrastructure
**Priority:** Medium  
**Session:** 9+

- [ ] Automate full corpus benchmarking
- [ ] Add memory profiling to benchmark suite
- [ ] Create benchmark regression detection
- [ ] Expand corpus with real-world datasets
- [ ] Add streaming-specific benchmarks

### Code Quality
**Priority:** Low  
**Session:** 8+

- [ ] Address 528 clippy pedantic warnings (deferred from Session 7)
- [ ] Expand API documentation with more examples
- [ ] Add integration test suite
- [ ] Improve error messages with more context

### Performance
**Priority:** Medium

- [ ] Investigate SIMD opportunities in MSN extraction
- [ ] Optimize metadata serialization/deserialization
- [ ] Profile streaming compression hot paths
- [ ] Memory pool tuning for streaming

---

## ✅ Completed (Session 10)

- [x] Extend Domain trait with `extract_with_fields()` method
- [x] Implement `extract_with_fields()` for JsonDomain
- [x] Implement `extract_with_fields()` for JsonLogDomain (disabled)
- [x] Implement `extract_with_fields()` for CsvDomain (disabled)
- [x] Add `extract_with_metadata()` public API
- [x] Update StreamingCompressor to use consistent metadata
- [x] Update StreamingDecompressor for per-block reconstruction
- [x] Fix JSON field-value mapping corruption
- [x] Verify 85%+ compression improvement with MSN
- [x] All 10 MSN streaming tests passing

---

## 📊 Test Status Summary

| Test Suite | Status | Count | Notes |
|------------|--------|-------|-------|
| MSN Streaming | ✅ Pass | 10/10 | All tests passing |
| Core Engine | ✅ Pass | 30 | All passing |
| MSN Domains | ✅ Pass | 14 | All passing |
| Golden Vectors | ✅ Pass | 15 | All passing |
| Property Tests | ✅ Pass | 16 | All passing |
| **Corpus Tests** | ⚠️ **Flaky** | 4/6 | CSV tests fail in full workspace |
| Total | ✅ Mostly Pass | 250+ | 2 test isolation issues |

---

## 🔍 Investigation Queue

1. **High Priority:** CSV corpus test isolation bug
2. **High Priority:** Large file frame errors (>5MB)
3. **Medium Priority:** JsonLogDomain streaming optimization
4. **Medium Priority:** CsvDomain streaming optimization

---

*Last Updated: 2026-03-03 (Session 10)*
