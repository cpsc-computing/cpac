#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Exercise transform decode paths with arbitrary input.
    // None of these should panic.
    let _ = cpac_transforms::delta::delta_decode(data);
    let _ = cpac_transforms::rolz::rolz_decode(data);
    let _ = cpac_transforms::zigzag::zigzag_decode_batch(data);
    // Transpose requires valid width; try a few.
    for w in [2, 4, 8, 16] {
        let _ = cpac_transforms::transpose::transpose_decode(data, w);
    }
    // Unpreprocess
    let _ = cpac_transforms::unpreprocess(data, &[]);
});
