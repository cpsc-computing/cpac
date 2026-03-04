# CPAC TODO & Known Issues

## ✅ All Critical Issues Resolved (Session 11/12)

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

- [x] Implement XML domain `extract_with_fields()` ✅ Session 12
- [x] Implement YAML domain `extract_with_fields()` ✅ Session 12
- [x] Per-block metadata verification ✅ Session 12
- [x] Streaming MSN benchmarks (Criterion) ✅ Session 12
- [ ] Optimize JsonDomain for JSONL format (future work)

### Benchmark Infrastructure
**Priority:** Medium

- [x] Add memory profiling to benchmark suite ✅ (peak_memory_bytes field wired)
- [x] Create benchmark regression detection ✅ Session 12
- [x] Add streaming-specific benchmarks ✅ Session 12
- [ ] Automate full corpus benchmarking (future work)
- [ ] Expand corpus with real-world datasets (future work)

### Code Quality
**Priority:** Low

- [x] Address clippy pedantic warnings ✅ Session 12 (auto-fixed ~243; remaining ~366 are intentional)
- [x] Expand API documentation with examples ✅ Session 12
- [x] Add integration test suite ✅ Session 12
- [ ] Improve error messages with more context (future work)

### Performance
**Priority:** Medium

- [x] Investigate SIMD opportunities in MSN extraction ✅ Session 12 (identified hot paths; use memchr; deferred pending profiling)
- [x] Optimize metadata serialization ✅ Session 12 (MessagePack compact encoding, ~30-40% smaller)
- [ ] Profile streaming compression hot paths (future work)
- [ ] Memory pool tuning for streaming (future work)

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

## ✅ Completed (Session 12)

- [x] Fix YAML CRLF normalisation regression (compact_yaml/expand_yaml lines() → split('\n'))
- [x] Implement XML domain `extract_with_fields()` with 2 streaming tests
- [x] Implement YAML domain `extract_with_fields()` with 3 tests (prefix conflict, two-block)
- [x] StreamingDecompressor per-block output size verification
- [x] Compact MessagePack metadata encoding (`encode_metadata_compact` / `decode_metadata_compact`)
- [x] Benchmark regression detection (`save_baseline`, `load_baseline`, `check_regressions`)
- [x] Streaming Criterion benchmarks (4 groups: JSON/CSV/binary compress + decompress)
- [x] SIMD investigation: identified hot paths, added `memchr` opportunity comments
- [x] Clippy pedantic auto-fix (~243 warnings fixed workspace-wide)
- [x] API documentation expansion (doc examples on `with_msn`, `extract_with_metadata`, compact fns)
- [x] Integration test suite (13 tests: engine roundtrips, streaming+MSN, error handling, metadata)
- [x] Update LEDGER.md and TODO.md

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

## 📊 Test Status Summary (Session 12)

| Test Suite | Status | Count | Notes |
|------------|--------|-------|
| Integration Tests | ✅ Pass | 13/13 | Added in S12 |
| MSN Streaming | ✅ Pass | 13/13 | All tests passing |
| Core Engine | ✅ Pass | 32 | Includes 2 regression tests |
| MSN Domains | ✅ Pass | 41 | Includes XML/YAML streaming |
| Golden Vectors | ✅ Pass | 15 | All passing |
| Property Tests | ✅ Pass | 16 | All passing |
| Corpus Tests | ✅ Pass | 6/6 | All passing |
| Total | ✅ Pass | 341+ | 0 failures |

---

*Last Updated: 2026-03-04 (Session 12)*
