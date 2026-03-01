#![no_main]
use libfuzzer_sys::fuzz_target;
use cpac_types::{Backend, CompressConfig};

fuzz_target!(|data: &[u8]| {
    // Test with default config
    let config = CompressConfig::default();
    if let Ok(compressed) = cpac_engine::compress(data, &config) {
        if let Ok(decompressed) = cpac_engine::decompress(&compressed.data) {
            assert_eq!(decompressed.data, data, "default roundtrip mismatch");
        }
    }
    
    // Test with each backend explicitly
    for backend in [Backend::Zstd, Backend::Brotli, Backend::Raw] {
        let config = CompressConfig {
            backend: Some(backend),
            ..Default::default()
        };
        if let Ok(compressed) = cpac_engine::compress(data, &config) {
            // Verify backend was used
            assert_eq!(compressed.backend, backend, "backend mismatch");
            // Verify roundtrip
            if let Ok(decompressed) = cpac_engine::decompress(&compressed.data) {
                assert_eq!(decompressed.data, data, "backend {:?} roundtrip mismatch", backend);
            }
        }
    }
});
