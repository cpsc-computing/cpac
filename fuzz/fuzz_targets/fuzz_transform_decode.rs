#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Exercise transform decode paths with arbitrary input.
    // None of these should panic - only return Err on invalid input.
    
    // Serial transforms
    let _ = cpac_transforms::delta::delta_decode(data);
    let _ = cpac_transforms::rolz::rolz_decode(data);
    let _ = cpac_transforms::zigzag::zigzag_decode_batch(data);
    
    // Transpose with various widths
    for w in [2, 4, 8, 16, 32] {
        let _ = cpac_transforms::transpose::transpose_decode(data, w);
    }
    
    // Float split (requires specific structure but shouldn't panic)
    let _ = cpac_transforms::float_split::float_split_decode_framed(data);
    
    // Range pack framed decode
    let _ = cpac_transforms::range_pack::range_pack_decode_framed(data);
    
    // Prefix and dedup decodes
    let _ = cpac_transforms::prefix::prefix_decode(data);
    let _ = cpac_transforms::dedup::dedup_columns_decode(data);
    
    // Generic unpreprocess
    let _ = cpac_transforms::unpreprocess(data, &[]);
});
