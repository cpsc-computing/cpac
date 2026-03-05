# cpac-ffi

C/C++ FFI bindings for the CPAC compression engine.

## Features

- **Simple API**: One-shot compression and decompression functions
- **Streaming API**: Incremental processing with bounded memory
- **Thread-safe**: All functions are thread-safe
- **Cross-platform**: Windows, macOS, Linux support
- **CMake integration**: Easy integration into C/C++ projects

## Building

### Prerequisites

- Rust toolchain (stable)
- CMake ≥ 3.15 (for CMake integration)
- cbindgen (optional, for header generation): `cargo install cbindgen`

### Build with Cargo

```powershell
# Build static + shared libraries
cargo build --release

# Outputs:
# - target/release/cpac_ffi.lib (Windows static)
# - target/release/cpac_ffi.dll (Windows shared)
# - target/release/libcpac_ffi.a (Unix static)
# - target/release/libcpac_ffi.so (Unix shared)
```

### Generate C Header

```powershell
cbindgen --config cbindgen.toml --crate cpac-ffi --output cpac.h
```

### Build with CMake

```powershell
mkdir build
cd build
cmake ..
cmake --build . --config Release
cmake --install . --prefix /path/to/install
```

CMake options:
- `CPAC_BUILD_STATIC=ON/OFF` - Build static library (default: ON)
- `CPAC_BUILD_SHARED=ON/OFF` - Build shared library (default: ON)
- `CPAC_GENERATE_HEADER=ON/OFF` - Generate header with cbindgen (default: ON)

## Usage

### CMake Integration

```cmake
find_package(cpac REQUIRED)

add_executable(my_app main.c)
target_link_libraries(my_app cpac::static)  # or cpac::shared
```

### Manual Linking

**Windows (MSVC)**:
```powershell
cl /I. main.c cpac_ffi.lib ws2_32.lib userenv.lib bcrypt.lib ntdll.lib
```

**Linux/macOS**:
```bash
gcc -I. main.c -L. -lcpac_ffi -ldl -lpthread -lm -o main
```

## API Reference

### Simple API

```c
#include "cpac.h"

// One-shot compression
CpacErrorCode cpac_compress(
    const uint8_t* input,
    size_t input_size,
    uint8_t* output,
    size_t output_capacity,
    size_t* output_size,
    const CpacCompressConfig* config  // NULL = default
);

// One-shot decompression
CpacErrorCode cpac_decompress(
    const uint8_t* input,
    size_t input_size,
    uint8_t* output,
    size_t output_capacity,
    size_t* output_size
);

// Get upper bound for compressed size
size_t cpac_compress_bound(size_t input_size);

// Get library version
const char* cpac_version(void);
```

### Streaming API

```c
// Compression
CpacCompressor* cpac_compressor_new(const CpacCompressConfig* config);
CpacErrorCode cpac_compressor_write(CpacCompressor* compressor, const uint8_t* input, size_t input_size);
CpacErrorCode cpac_compressor_finish(CpacCompressor* compressor);
CpacErrorCode cpac_compressor_read(CpacCompressor* compressor, uint8_t* output, size_t output_capacity, size_t* output_size);
void cpac_compressor_free(CpacCompressor* compressor);

// Decompression
CpacDecompressor* cpac_decompressor_new(void);
CpacErrorCode cpac_decompressor_feed(CpacDecompressor* decompressor, const uint8_t* input, size_t input_size);
CpacErrorCode cpac_decompressor_read(CpacDecompressor* decompressor, uint8_t* output, size_t output_capacity, size_t* output_size);
int cpac_decompressor_is_done(const CpacDecompressor* decompressor);
void cpac_decompressor_free(CpacDecompressor* decompressor);
```

### Error Codes

```c
typedef enum CpacErrorCode {
    CPAC_OK = 0,
    CPAC_INVALID_ARG = 1,
    CPAC_IO = 2,
    CPAC_INVALID_FRAME = 3,
    CPAC_UNSUPPORTED_BACKEND = 4,
    CPAC_DECOMPRESS_FAILED = 5,
    CPAC_COMPRESS_FAILED = 6,
    CPAC_TRANSFORM = 7,
    CPAC_ENCRYPTION = 8,
    CPAC_OUT_OF_MEMORY = 9,
    CPAC_OTHER = 255
} CpacErrorCode;
```

### Backends

```c
typedef enum CpacBackend {
    CPAC_BACKEND_RAW = 0,
    CPAC_BACKEND_ZSTD = 1,
    CPAC_BACKEND_BROTLI = 2,
    CPAC_BACKEND_GZIP = 3,
    CPAC_BACKEND_LZMA = 4
} CpacBackend;
```

### Configuration

```c
typedef struct CpacCompressConfig {
    CpacBackend backend;         // 0 = auto-select
    uint32_t level;              // 0 = auto, 1-22 backend-specific
    uint32_t max_threads;        // 0 = auto
    uint64_t max_memory_bytes;   // 0 = auto
} CpacCompressConfig;
```

## Example

### Simple Compression

```c
#include "cpac.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(void) {
    const char* input = "Hello, CPAC from C!";
    size_t input_size = strlen(input);
    
    // Allocate output buffer
    size_t max_compressed = cpac_compress_bound(input_size);
    uint8_t* compressed = malloc(max_compressed);
    size_t compressed_size = 0;
    
    // Compress
    CpacErrorCode err = cpac_compress(
        (const uint8_t*)input,
        input_size,
        compressed,
        max_compressed,
        &compressed_size,
        NULL  // default config
    );
    
    if (err != CPAC_OK) {
        fprintf(stderr, "Compression failed: %d\n", err);
        free(compressed);
        return 1;
    }
    
    printf("Compressed %zu bytes to %zu bytes (%.2fx)\n",
           input_size, compressed_size,
           (double)input_size / compressed_size);
    
    // Decompress
    uint8_t* decompressed = malloc(input_size);
    size_t decompressed_size = 0;
    
    err = cpac_decompress(
        compressed,
        compressed_size,
        decompressed,
        input_size,
        &decompressed_size
    );
    
    if (err != CPAC_OK) {
        fprintf(stderr, "Decompression failed: %d\n", err);
        free(compressed);
        free(decompressed);
        return 1;
    }
    
    printf("Decompressed: %.*s\n", (int)decompressed_size, decompressed);
    
    free(compressed);
    free(decompressed);
    return 0;
}
```

### Streaming Compression

```c
#include "cpac.h"
#include <stdio.h>
#include <stdlib.h>

int main(void) {
    // Create compressor
    CpacCompressor* comp = cpac_compressor_new(NULL);
    if (!comp) {
        fprintf(stderr, "Failed to create compressor\n");
        return 1;
    }
    
    // Feed data in chunks
    const char* chunk1 = "First chunk ";
    const char* chunk2 = "Second chunk ";
    const char* chunk3 = "Third chunk";
    
    cpac_compressor_write(comp, (const uint8_t*)chunk1, strlen(chunk1));
    cpac_compressor_write(comp, (const uint8_t*)chunk2, strlen(chunk2));
    cpac_compressor_write(comp, (const uint8_t*)chunk3, strlen(chunk3));
    
    // Finalize
    cpac_compressor_finish(comp);
    
    // Read output
    uint8_t output[1024];
    size_t output_size = 0;
    cpac_compressor_read(comp, output, sizeof(output), &output_size);
    
    printf("Compressed %zu bytes\n", output_size);
    
    cpac_compressor_free(comp);
    return 0;
}
```

## License

Copyright (c) 2026 BitConcepts, LLC

Licensed under LicenseRef-CPSC-Research-Evaluation-1.0

For licensing inquiries, contact: info@bitconcepts.tech
