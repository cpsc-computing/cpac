#!/usr/bin/env pwsh
# Profile-Guided Optimization (PGO) Build Script for CPAC
# Generates optimized binaries using runtime profiling data

param(
    [Parameter(Mandatory=$false)]
    [string]$Target = "cpac-cli",
    
    [Parameter(Mandatory=$false)]
    [string]$ProfileData = "pgo-data"
)

$ErrorActionPreference = "Stop"

Write-Host "`n=== CPAC PGO Build ===" -ForegroundColor Cyan
Write-Host "Target: $Target"
Write-Host "Profile Data: $ProfileData`n"

# Step 1: Build instrumented binary
Write-Host "Step 1: Building instrumented binary..." -ForegroundColor Yellow
$env:RUSTFLAGS = "-Cprofile-generate=$ProfileData"
cargo build --release -p $Target
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# Step 2: Run training workload
Write-Host "`nStep 2: Running training workload..." -ForegroundColor Yellow

$BinaryPath = "target\release\cpac.exe"

# Training corpus: Canterbury + Silesia samples
$TrainingFiles = @(
    ".work\benchdata\canterbury\alice29.txt",
    ".work\benchdata\canterbury\kennedy.xls",
    ".work\benchdata\silesia\dickens",
    ".work\benchdata\silesia\xml"
)

foreach ($File in $TrainingFiles) {
    if (Test-Path $File) {
        Write-Host "  Training on: $File"
        & $BinaryPath compress $File --output "$File.pgo.cpac" --force | Out-Null
        & $BinaryPath decompress "$File.pgo.cpac" --output "$File.pgo.out" --force | Out-Null
        & $BinaryPath benchmark $File --quick | Out-Null
        Remove-Item "$File.pgo.cpac", "$File.pgo.out" -ErrorAction SilentlyContinue
    }
}

Write-Host "  Generated $(Get-ChildItem $ProfileData\*.profraw | Measure-Object | Select-Object -ExpandProperty Count) profile files"

# Step 3: Merge profiles
Write-Host "`nStep 3: Merging profile data..." -ForegroundColor Yellow
$MergedProfile = "$ProfileData\merged.profdata"

# Find llvm-profdata
$LlvmProfdata = Get-Command llvm-profdata -ErrorAction SilentlyContinue
if (-not $LlvmProfdata) {
    # Try rustup path
    $RustToolchain = rustc --version | Select-String -Pattern "(\d+\.\d+\.\d+)" | ForEach-Object { $_.Matches.Groups[1].Value }
    $LlvmProfdata = "$env:USERPROFILE\.rustup\toolchains\stable-x86_64-pc-windows-msvc\lib\rustlib\x86_64-pc-windows-msvc\bin\llvm-profdata.exe"
}

if (Test-Path $LlvmProfdata) {
    & $LlvmProfdata merge -o $MergedProfile "$ProfileData\*.profraw"
    Write-Host "  Merged to: $MergedProfile"
} else {
    Write-Host "  Warning: llvm-profdata not found, using raw profiles" -ForegroundColor Yellow
    $MergedProfile = $ProfileData
}

# Step 4: Build optimized binary
Write-Host "`nStep 4: Building PGO-optimized binary..." -ForegroundColor Yellow
$env:RUSTFLAGS = "-Cprofile-use=$MergedProfile -Cllvm-args=-pgo-warn-missing-function"
cargo build --release -p $Target
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "`n=== PGO Build Complete ===" -ForegroundColor Green
Write-Host "Optimized binary: target\release\cpac.exe"
Write-Host "`nCleanup profile data: Remove-Item -Recurse $ProfileData"
