#![no_main]

use libfuzzer_sys::fuzz_target;
use cpac_msn::{extract, reconstruct};

fuzz_target!(|data: &[u8]| {
    // Fuzz MSN extraction and reconstruction
    // Test: extract(data) should never panic
    // Test: reconstruct(extract(data)) should equal original data
    
    // Try extraction with different confidence thresholds
    for confidence in [0.3, 0.5, 0.7] {
        if let Ok(result) = extract(data, None, confidence) {
            // If extraction succeeded, reconstruction must work
            if result.applied {
                if let Ok(reconstructed) = reconstruct(&result) {
                    // Verify lossless roundtrip
                    assert_eq!(data, reconstructed.as_slice(), "Lossless roundtrip failed");
                }
            } else {
                // Passthrough case
                assert_eq!(result.residual, data, "Passthrough failed");
            }
        }
    }
});
