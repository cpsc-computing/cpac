#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Just try to parse — should never panic, only return Err
    // This exercises the entire archive parsing logic including
    // header validation, entry parsing, and bounds checking
    let _ = cpac_archive::list_archive(data);
});
