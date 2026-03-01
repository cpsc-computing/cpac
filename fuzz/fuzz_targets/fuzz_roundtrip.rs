#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let config = cpac_types::CompressConfig::default();
    if let Ok(compressed) = cpac_engine::compress(data, &config) {
        if let Ok(decompressed) = cpac_engine::decompress(&compressed.data) {
            assert_eq!(decompressed.data, data, "roundtrip mismatch");
        }
    }
});
