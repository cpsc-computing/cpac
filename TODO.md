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

### JsonLogDomain Streaming (Session 10/11)
**Status:** ✅ Resolved in Session 11  
**Resolution:** Implemented `extract_with_fields()` with line-aligned blocking. Blocks split at last `\n`; incomplete tail stored as suffix in 0x01-prefixed wire format. All streaming tests pass.

### CsvDomain Streaming (Session 10/11)
**Status:** ✅ Resolved in Session 11  
**Resolution:** Implemented `extract_with_fields()` with header detection. Data-only blocks use 0x01-prefix passthrough. All streaming tests pass.

### Large File Frame Errors (Session 9)
**Status:** ✅ Resolved (Session 10 `is_cpbl()` guard fix)  
**Verification:** All 12 Silesia files (5–51 MB) benchmark cleanly in Session 11 full benchmark run.

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

## ✅ Completed (Session 11)

- [x] Fix msn_streaming regression (double MSN on streaming residuals)
- [x] Implement JsonLogDomain `extract_with_fields()` with line-aligned blocking
- [x] Implement CsvDomain `extract_with_fields()` with header detection
- [x] Fix 8 clippy warnings across cpac-msn
- [x] All workspace tests passing (300+, 0 failures)
- [x] Run quick/balanced/full benchmarks
- [x] Update BENCHMARKING.md with Session 11 results
- [x] Update LEDGER.md and TODO.md
- [x] Remove SESSION-10-SUMMARY.md

---

## 📊 Test Status Summary (Session 11)

| Test Suite | Status | Count | Notes |
|------------|--------|-------|-------|
| MSN Streaming | ✅ Pass | 13/13 | All tests passing (3 fixed in S11) |
| Core Engine | ✅ Pass | 30 | All passing |
| MSN Domains | ✅ Pass | 36 | All passing |
| Golden Vectors | ✅ Pass | 15 | All passing |
| Property Tests | ✅ Pass | 16 | All passing |
| Corpus Tests | ✅ Pass | 6/6 | All passing |
| Total | ✅ Pass | 300+ | 0 failures |

---

## 🔍 Investigation Queue

1. **Medium Priority:** XML domain `extract_with_fields()` for streaming
2. **Medium Priority:** YAML domain `extract_with_fields()` for streaming
3. **Low Priority:** Per-block metadata verification in streaming

---

*Last Updated: 2026-03-03 (Session 11)*
