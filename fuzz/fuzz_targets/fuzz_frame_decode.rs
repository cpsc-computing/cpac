#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Should never panic on arbitrary input, only return Err.
    let _ = cpac_frame::decode_frame(data);
});
