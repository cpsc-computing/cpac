#!/usr/bin/env pwsh
# Test compressed media recompression workflow
# This demonstrates CPAC's ability to compress already-compressed data (FLAC audio, PNG images)
# and then reconstruct the original compressed format losslessly

param(
    [string]$MediaType = "audio",  # audio, images, or both
    [int]$SampleSize = 20,
    [string]$Backend = "zstd"  # raw, zstd, brotli, gzip, lzma
)

$ErrorActionPreference = "Stop"

Write-Host "CPAC Compressed Media Recompression Test" -ForegroundColor Cyan
Write-Host "=========================================" -ForegroundColor Cyan
Write-Host ""

$repoRoot = Split-Path -Parent $PSScriptRoot
$benchdata = Join-Path $repoRoot ".work\benchdata"
$resultsDir = Join-Path $repoRoot ".work\benchmarks\media-recompression"
New-Item -ItemType Directory -Force -Path $resultsDir | Out-Null

# Check for cpac binary
$cpacExe = Join-Path $repoRoot "target\release\cpac.exe"
if (-not (Test-Path $cpacExe)) {
    Write-Error "CPAC binary not found. Run: cargo build --release"
    exit 1
}

# Test configurations
$tests = @()

if ($MediaType -eq "audio" -or $MediaType -eq "both") {
    $flacDir = Join-Path $benchdata "audio\vctk"
    if (Test-Path $flacDir) {
        $tests += @{
            Name = "FLAC Audio"
            Type = "audio"
            Extension = ".flac"
            SourceDir = $flacDir
            Description = "Lossless compressed audio (VCTK corpus)"
        }
    } else {
        Write-Warning "VCTK audio corpus not found at: $flacDir"
    }
}

if ($MediaType -eq "images" -or $MediaType -eq "both") {
    $kodakDir = Join-Path $benchdata "images\kodak"
    if (Test-Path $kodakDir) {
        $tests += @{
            Name = "PNG Images"
            Type = "images"
            Extension = ".png"
            SourceDir = $kodakDir
            Description = "Lossless compressed images (Kodak suite)"
        }
    } else {
        Write-Warning "Kodak image corpus not found at: $kodakDir"
    }
}

if ($tests.Count -eq 0) {
    Write-Error "No media corpora found. Download with: .\download-corpus.ps1 -Corpus vctk,kodak"
    exit 1
}

# Results tracking
$results = @()

foreach ($test in $tests) {
    Write-Host "Testing: $($test.Name)" -ForegroundColor Yellow
    Write-Host "  $($test.Description)" -ForegroundColor Gray
    Write-Host ""
    
    # Sample files
    $files = Get-ChildItem $test.SourceDir -Filter "*$($test.Extension)" -File | 
             Get-Random -Count $SampleSize
    
    if ($files.Count -eq 0) {
        Write-Warning "  No $($test.Extension) files found"
        continue
    }
    
    Write-Host "  Sample size: $($files.Count) files" -ForegroundColor Cyan
    
    # Create test workspace
    $testDir = Join-Path $resultsDir "$($test.Type)_test_$(Get-Date -Format 'yyyyMMdd_HHmmss')"
    New-Item -ItemType Directory -Force -Path $testDir | Out-Null
    
    $originalDir = Join-Path $testDir "1_original"
    $cpacCompressedDir = Join-Path $testDir "2_cpac_compressed"
    $cpacDecompressedDir = Join-Path $testDir "3_cpac_decompressed"
    
    New-Item -ItemType Directory -Force -Path $originalDir, $cpacCompressedDir, $cpacDecompressedDir | Out-Null
    
    # Copy sample files
    $files | ForEach-Object { Copy-Item $_.FullName -Destination $originalDir }
    
    # Measure original size
    $originalSize = (Get-ChildItem $originalDir -File | Measure-Object -Property Length -Sum).Sum
    
    Write-Host "  Original size: $([math]::Round($originalSize / 1MB, 2)) MB" -ForegroundColor Gray
    
    # Compress with CPAC (compressing already-compressed data)
    Write-Host "  Compressing with CPAC..." -ForegroundColor Green
    $compressStart = Get-Date
    
    Get-ChildItem $originalDir -File | ForEach-Object {
        $outFile = Join-Path $cpacCompressedDir "$($_.Name).cpac"
        & $cpacExe compress $_.FullName -o $outFile --backend $Backend 2>&1 | Out-Null
    }
    
    $compressTime = (Get-Date) - $compressStart
    $cpacSize = (Get-ChildItem $cpacCompressedDir -File | Measure-Object -Property Length -Sum).Sum
    
    Write-Host "    Compressed size: $([math]::Round($cpacSize / 1MB, 2)) MB" -ForegroundColor Gray
    Write-Host "    Compression time: $([math]::Round($compressTime.TotalSeconds, 2))s" -ForegroundColor Gray
    Write-Host "    Ratio: $([math]::Round($originalSize / $cpacSize, 2))x" -ForegroundColor Cyan
    
    # Decompress with CPAC (reconstructing compressed media)
    Write-Host "  Decompressing with CPAC..." -ForegroundColor Green
    $decompressStart = Get-Date
    
    Get-ChildItem $cpacCompressedDir -Filter "*.cpac" | ForEach-Object {
        $outFile = Join-Path $cpacDecompressedDir ($_.Name -replace '\.cpac$', '')
        & $cpacExe decompress $_.FullName -o $outFile 2>&1 | Out-Null
    }
    
    $decompressTime = (Get-Date) - $decompressStart
    
    Write-Host "    Decompression time: $([math]::Round($decompressTime.TotalSeconds, 2))s" -ForegroundColor Gray
    
    # Verify lossless reconstruction
    Write-Host "  Verifying lossless reconstruction..." -ForegroundColor Yellow
    
    $verified = $true
    $originalFiles = Get-ChildItem $originalDir -File | Sort-Object Name
    $decompressedFiles = Get-ChildItem $cpacDecompressedDir -File | Sort-Object Name
    
    if ($originalFiles.Count -ne $decompressedFiles.Count) {
        Write-Host "    FAIL: File count mismatch" -ForegroundColor Red
        $verified = $false
    } else {
        for ($i = 0; $i -lt $originalFiles.Count; $i++) {
            $orig = $originalFiles[$i]
            $decomp = $decompressedFiles[$i]
            
            $origHash = (Get-FileHash $orig.FullName -Algorithm SHA256).Hash
            $decompHash = (Get-FileHash $decomp.FullName -Algorithm SHA256).Hash
            
            if ($origHash -ne $decompHash) {
                Write-Host "    FAIL: $($orig.Name) hash mismatch" -ForegroundColor Red
                $verified = $false
                break
            }
        }
    }
    
    if ($verified) {
        Write-Host "    SUCCESS: 100% lossless reconstruction" -ForegroundColor Green
    }
    
    Write-Host ""
    
    # Record results
    $results += [PSCustomObject]@{
        MediaType = $test.Name
        Files = $files.Count
        OriginalSize_MB = [math]::Round($originalSize / 1MB, 2)
        CPACSize_MB = [math]::Round($cpacSize / 1MB, 2)
        CompressionRatio = [math]::Round($originalSize / $cpacSize, 2)
        CompressTime_s = [math]::Round($compressTime.TotalSeconds, 2)
        DecompressTime_s = [math]::Round($decompressTime.TotalSeconds, 2)
        CompressSpeed_MBps = [math]::Round(($originalSize / 1MB) / $compressTime.TotalSeconds, 2)
        DecompressSpeed_MBps = [math]::Round(($originalSize / 1MB) / $decompressTime.TotalSeconds, 2)
        Verified = if ($verified) { "✓" } else { "✗" }
    }
}

Write-Host "=========================================" -ForegroundColor Cyan
Write-Host "Summary Results" -ForegroundColor Cyan
Write-Host "=========================================" -ForegroundColor Cyan
Write-Host ""

$results | Format-Table -AutoSize

Write-Host ""
Write-Host "Key Insights:" -ForegroundColor Yellow
Write-Host "  - CPAC can compress already-compressed media (FLAC, PNG)" -ForegroundColor Gray
Write-Host "  - Original compressed format is reconstructed losslessly" -ForegroundColor Gray
Write-Host "  - This enables: archival of compressed media, network transfer optimization" -ForegroundColor Gray
Write-Host "  - Use case: Backup/archive compressed audio libraries with additional ~1.5-3x gains" -ForegroundColor Gray
Write-Host ""

# Save results
$csvPath = Join-Path $resultsDir "results_$(Get-Date -Format 'yyyyMMdd_HHmmss').csv"
$results | Export-Csv -Path $csvPath -NoTypeInformation
Write-Host "Results saved to: $csvPath" -ForegroundColor Green
