// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Build script: compile vendored Lizard C source into a static library.

fn main() {
    let lizard = "lizard";

    cc::Build::new()
        // Core compress / decompress
        .file(format!("{lizard}/lizard_compress.c"))
        .file(format!("{lizard}/lizard_decompress.c"))
        // Entropy (FSE + Huffman) — required for levels 30-49
        .file(format!("{lizard}/entropy/entropy_common.c"))
        .file(format!("{lizard}/entropy/fse_compress.c"))
        .file(format!("{lizard}/entropy/fse_decompress.c"))
        .file(format!("{lizard}/entropy/huf_compress.c"))
        .file(format!("{lizard}/entropy/huf_decompress.c"))
        .file(format!("{lizard}/entropy/hist.c"))
        .file(format!("{lizard}/entropy/debug.c"))
        // Include paths
        .include(lizard)
        .include(format!("{lizard}/entropy"))
        // Optimisation: let cc pick release/debug based on Cargo profile
        .opt_level_str("2")
        // Suppress noisy upstream warnings
        .warnings(false)
        .compile("lizard");
}
