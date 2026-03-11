// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Build script: compile vendored LZHAM C++ source into a static library.

fn main() {
    let mut build = cc::Build::new();
    build.cpp(true);

    // --- Include paths ---
    build.include("lzham/include");
    build.include("lzham/lzhamdecomp");
    build.include("lzham/lzhamcomp");

    // --- Decompressor sources (also contains shared helpers) ---
    build.file("lzham/lzhamdecomp/lzham_assert.cpp");
    build.file("lzham/lzhamdecomp/lzham_checksum.cpp");
    build.file("lzham/lzhamdecomp/lzham_huffman_codes.cpp");
    build.file("lzham/lzhamdecomp/lzham_lzdecomp.cpp");
    build.file("lzham/lzhamdecomp/lzham_lzdecompbase.cpp");
    build.file("lzham/lzhamdecomp/lzham_mem.cpp");
    build.file("lzham/lzhamdecomp/lzham_platform.cpp");
    build.file("lzham/lzhamdecomp/lzham_prefix_coding.cpp");
    build.file("lzham/lzhamdecomp/lzham_symbol_codec.cpp");
    build.file("lzham/lzhamdecomp/lzham_timer.cpp");
    build.file("lzham/lzhamdecomp/lzham_vector.cpp");

    // --- Compressor sources ---
    build.file("lzham/lzhamcomp/lzham_lzbase.cpp");
    build.file("lzham/lzhamcomp/lzham_lzcomp.cpp");
    build.file("lzham/lzhamcomp/lzham_lzcomp_internal.cpp");
    build.file("lzham/lzhamcomp/lzham_lzcomp_state.cpp");
    build.file("lzham/lzhamcomp/lzham_match_accel.cpp");

    // --- Library entry point ---
    build.file("lzham/lzhamlib/lzham_lib.cpp");

    // --- Platform defines + threading ---
    if cfg!(target_os = "windows") {
        // MSVC defines _WIN32 but NOT WIN32; lzham_core.h checks for WIN32.
        build.define("WIN32", None);
        // Win32 threading (InterlockedCompareExchange, CreateThread, etc.)
        build.file("lzham/lzhamcomp/lzham_win32_threading.cpp");
    } else if cfg!(unix) {
        // POSIX pthreads
        build.file("lzham/lzhamcomp/lzham_pthreads_threading.cpp");
    } else {
        // Fallback: disable threading via LZHAM_ANSI_CPLUSPLUS
        build.define("LZHAM_ANSI_CPLUSPLUS", "1");
    }

    // Suppress noisy upstream warnings
    build.warnings(false);
    build.opt_level_str("2");
    build.compile("lzham");
}
