#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }
    let compressed = cpac_cas::cas_compress(data);
    if let Ok(decompressed) = cpac_cas::cas_decompress(&compressed) {
        assert_eq!(decompressed, data, "CAS roundtrip mismatch");
    }
});
