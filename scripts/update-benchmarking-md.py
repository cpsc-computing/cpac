#!/usr/bin/env python3
"""Update BENCHMARKING.md with completed benchmark results"""

import re

# Read the file
with open("BENCHMARKING.md", "r", encoding="utf-8") as f:
    content = f.read()

# Canterbury corpus updates
canterbury_updates = {
    "| asyoulik.txt | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD |":
        "| asyoulik.txt | 2.49x @ 14 MB/s | 2.68x @ 8 MB/s | 2.56x @ 9 MB/s | 1.00x @ 15 MB/s | 2.56x @ 26 MB/s | 2.50x @ 142 MB/s | **2.93x @ 1 MB/s** | 1.80x @ 44 MB/s | **Baseline brotli-11** |",
    
    "| kennedy.xls | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD |":
        "| kennedy.xls | 5.84x @ 42 MB/s | 7.26x @ 20 MB/s | 5.12x @ 7 MB/s | 1.13x @ 45 MB/s | 4.92x @ 10 MB/s | **9.21x @ 472 MB/s** | **16.75x @ 1 MB/s** | 2.68x @ 68 MB/s | **Baseline brotli-11** |",
    
    "| lcet10.txt | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD |":
        "| lcet10.txt | 3.03x @ 15 MB/s | 3.33x @ 9 MB/s | 2.95x @ 10 MB/s | 1.00x @ 16 MB/s | 2.95x @ 26 MB/s | 3.03x @ 239 MB/s | **3.76x @ 1 MB/s** | 1.84x @ 46 MB/s | **Baseline brotli-11** |",
    
    "| plrabn12.txt | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD |":
        "| plrabn12.txt | 2.51x @ 13 MB/s | 2.70x @ 8 MB/s | 2.48x @ 7 MB/s | 1.00x @ 14 MB/s | 2.48x @ 16 MB/s | 2.51x @ 214 MB/s | **2.95x @ 1 MB/s** | 1.87x @ 47 MB/s | **Baseline brotli-11** |"
}

# Silesia corpus updates
silesia_updates = {
    "| mozilla (51 MB) | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD |":
        "| mozilla (51 MB) | ⚠️ Error | ⚠️ Error | ⚠️ Error | ⚠️ Error | 2.68x @ 17 MB/s | **2.79x @ 351 MB/s** | **3.63x @ 1 MB/s** | 1.79x @ 43 MB/s | **Baseline brotli-11** |",
    
    "| xml (5 MB) | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD |":
        "| xml (5 MB) | ⚠️ Error | ⚠️ Error | ⚠️ Error | ⚠️ Error | 8.05x @ 54 MB/s | **8.41x @ 680 MB/s** | **12.42x @ 1 MB/s** | 1.89x @ 49 MB/s | **Baseline brotli-11** |"
}

# Apply updates
for old, new in canterbury_updates.items():
    content = content.replace(old, new)

for old, new in silesia_updates.items():
    content = content.replace(old, new)

# Update Canterbury key findings
content = content.replace(
    """**Key Findings:**
- ✅ **CPAC Gzip = gzip-9 parity** on alice29.txt (2.80x exact match)
- ✅ **brotli-11 wins on compression ratio** (3.27x best)
- ✅ **CPAC Brotli competitive** (2.97x vs 3.27x)

### Calgary""",
    """**Key Findings:**
- ✅ **CPAC Gzip = gzip-9 parity** on alice29.txt (2.80x exact match)
- ✅ **brotli-11 dominates** on text files (2.93x-16.75x ratios)
- ✅ **zstd-3 exceptional speed** on Excel files (472 MB/s @ 9.21x)
- ✅ **CPAC backends consistent** across all Canterbury files

### Calgary"""
)

# Update Silesia key findings
content = content.replace(
    """**Key Findings:**
- ✅ **brotli-11 achieves 3.57x** on dickens (best ratio)
- ✅ **zstd-3 shows 12x+ speedup** vs gzip-9 (256 vs 20 MB/s)
- ⚠️ **CPAC backends TBD** - encountered frame version errors

**Note:** Silesia CPAC benchmarks need investigation (invalid frame version error).""",
    """**Key Findings:**
- ✅ **brotli-11 exceptional on XML** (12.42x ratio)
- ✅ **zstd-3 fastest** (680 MB/s on XML, 351 MB/s on mozilla)
- ⚠️ **CPAC backend errors** on large files (>5 MB) - frame version issue being investigated
- ✅ **Baselines complete** for all Silesia files"""
)

# Update date
content = content.replace(
    "**Date**: March 2, 2026 | **Version**: 0.1.0 | **Mode**: Balanced (3 iterations)",
    "**Date**: March 3, 2026 | **Version**: 0.1.0 | **Mode**: Balanced (3 iterations) | **Build**: Phase 1+2 Optimizations"
)

content = content.replace(
    "**Last Updated**: 2026-03-02",
    "**Last Updated**: 2026-03-03"
)

# Write back
with open("BENCHMARKING.md", "w", encoding="utf-8") as f:
    f.write(content)

print("✅ BENCHMARKING.md updated successfully!")
print("Updated:")
print("  - Canterbury corpus: asyoulik.txt, kennedy.xls, lcet10.txt, plrabn12.txt")
print("  - Silesia corpus: mozilla, xml (with error markers)")
print("  - Key findings sections")
print("  - Dates and build information")
