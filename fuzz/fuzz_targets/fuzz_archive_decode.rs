#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Just try to parse — should never panic, only return Err
    let _ = cpac_archive::list_archive(data);
});
