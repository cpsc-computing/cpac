#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Should never panic on arbitrary input, only return Err.
    // Test frame decoding
    let _ = cpac_frame::decode_frame(data);
    
    // Also test full decompress path (includes frame decode + entropy decode)
    let _ = cpac_engine::decompress(data);
    
    // Test parallel frame detection
    let _ = cpac_engine::is_cpbl(data);
});
