#!/usr/bin/env pwsh
# PGO (Profile-Guided Optimization) build script for CPAC Rust engine.
#
# Usage: .\pgo-build.ps1
#
# Prerequisites:
#   - Rust nightly toolchain (for -Cprofile-generate / -Cprofile-use)
#   - llvm-profdata on PATH
#
# The script:
#   1. Builds an instrumented binary
#   2. Runs it on representative workloads to generate profile data
#   3. Merges profiles
#   4. Rebuilds with profile-guided optimizations

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProfileDir = Join-Path $Root "target\pgo-profiles"
$MergedProfile = Join-Path $Root "target\pgo-merged.profdata"

Write-Host "=== CPAC PGO Build ===" -ForegroundColor Cyan

# Step 1: Clean previous profile data
if (Test-Path $ProfileDir) { Remove-Item -Recurse -Force $ProfileDir }
New-Item -ItemType Directory -Path $ProfileDir -Force | Out-Null

# Step 2: Build instrumented binary
Write-Host "[1/4] Building instrumented binary..." -ForegroundColor Yellow
$env:RUSTFLAGS = "-Cprofile-generate=$ProfileDir"
cargo build --release --manifest-path "$Root\Cargo.toml" -p cpac-cli
if ($LASTEXITCODE -ne 0) { Write-Error "Instrumented build failed"; exit 1 }

$Binary = Join-Path $Root "target\release\cpac-cli.exe"
if (-not (Test-Path $Binary)) {
    $Binary = Join-Path $Root "target\release\cpac.exe"
}

# Step 3: Run representative workloads
Write-Host "[2/4] Running profiling workloads..." -ForegroundColor Yellow

# Generate test corpus
$CorpusDir = Join-Path $Root "target\pgo-corpus"
if (-not (Test-Path $CorpusDir)) { New-Item -ItemType Directory -Path $CorpusDir -Force | Out-Null }

# ASCII text
$TextFile = Join-Path $CorpusDir "text.txt"
"The quick brown fox jumps over the lazy dog. " * 10000 | Set-Content -Path $TextFile -NoNewline

# CSV data
$CsvFile = Join-Path $CorpusDir "data.csv"
$csv = "id,name,value,status`n"
for ($i = 0; $i -lt 5000; $i++) { $csv += "$i,item_$i,$($i * 7 % 1000),$(if ($i % 2 -eq 0) {'ok'} else {'err'})`n" }
$csv | Set-Content -Path $CsvFile -NoNewline

# Binary data
$BinFile = Join-Path $CorpusDir "binary.bin"
[byte[]]$bytes = 0..255 | ForEach-Object { [byte]$_ } * 100
[System.IO.File]::WriteAllBytes($BinFile, $bytes[0..25599])

# Run compress/decompress on each file with each backend
foreach ($file in @($TextFile, $CsvFile, $BinFile)) {
    foreach ($backend in @("zstd", "brotli", "raw")) {
        $out = "$file.cpac"
        & $Binary compress $file -o $out --backend $backend --force 2>$null
        if (Test-Path $out) {
            & $Binary decompress $out -o "$file.dec" --force 2>$null
            Remove-Item -Force "$file.dec" -ErrorAction SilentlyContinue
            Remove-Item -Force $out -ErrorAction SilentlyContinue
        }
    }
}

# Step 4: Merge profiles
Write-Host "[3/4] Merging profile data..." -ForegroundColor Yellow
$env:RUSTFLAGS = ""
llvm-profdata merge -o $MergedProfile $ProfileDir
if ($LASTEXITCODE -ne 0) {
    Write-Warning "llvm-profdata not found or merge failed. Falling back to standard release build."
    cargo build --release --manifest-path "$Root\Cargo.toml" -p cpac-cli
    exit 0
}

# Step 5: PGO-optimized build
Write-Host "[4/4] Building PGO-optimized binary..." -ForegroundColor Yellow
$env:RUSTFLAGS = "-Cprofile-use=$MergedProfile"
cargo build --release --manifest-path "$Root\Cargo.toml" -p cpac-cli
if ($LASTEXITCODE -ne 0) { Write-Error "PGO build failed"; exit 1 }

Write-Host "=== PGO build complete ===" -ForegroundColor Green
Write-Host "Binary: $Binary"
