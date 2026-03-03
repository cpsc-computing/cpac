#!/usr/bin/env pwsh
# Fill TBD entries in BENCHMARKING.md with complete benchmark data

param(
    [ValidateSet('quick', 'balanced', 'full')]
    [string]$Mode = 'balanced',
    [string]$OutputFile = "BENCHMARKING.md"
)

$ErrorActionPreference = "Stop"

Write-Host "CPAC TBD Benchmark Fill Script" -ForegroundColor Cyan
Write-Host "Mode: $Mode" -ForegroundColor Yellow
Write-Host "==============================" -ForegroundColor Cyan
Write-Host ""

$repoRoot = Split-Path -Parent $PSScriptRoot
$cpacExe = Join-Path $repoRoot "target\release\cpac.exe"
$benchdata = Join-Path $repoRoot ".work\benchdata"

if (-not (Test-Path $cpacExe)) {
    Write-Error "CPAC binary not found. Run: cargo build --release"
    exit 1
}

# Iteration counts based on mode
$iterations = switch ($Mode) {
    'quick' { 1 }
    'balanced' { 3 }
    'full' { 10 }
}

Write-Host "Iterations: $iterations" -ForegroundColor Gray
Write-Host ""

# Files with TBDs in BENCHMARKING.md
$missingBenchmarks = @(
    @{Corpus = "Canterbury"; File = "asyoulik.txt"; Path = "canterbury\asyoulik.txt"},
    @{Corpus = "Canterbury"; File = "kennedy.xls"; Path = "canterbury\kennedy.xls"},
    @{Corpus = "Canterbury"; File = "lcet10.txt"; Path = "canterbury\lcet10.txt"},
    @{Corpus = "Canterbury"; File = "plrabn12.txt"; Path = "canterbury\plrabn12.txt"},
    @{Corpus = "Silesia"; File = "mozilla"; Path = "silesia\mozilla"},
    @{Corpus = "Silesia"; File = "xml"; Path = "silesia\xml"}
)

$results = @{}

foreach ($item in $missingBenchmarks) {
    $filePath = Join-Path $benchdata $item.Path
    
    if (-not (Test-Path $filePath)) {
        Write-Host "SKIP: $($item.Corpus) - $($item.File) (not found)" -ForegroundColor Gray
        continue
    }
    
    $fileSize = (Get-Item $filePath).Length
    $fileSizeMB = [math]::Round($fileSize / 1MB, 2)
    
    Write-Host "Benchmarking: $($item.Corpus) - $($item.File) ($fileSizeMB MB)" -ForegroundColor Yellow
    
    try {
        $output = & $cpacExe benchmark $filePath --iterations $iterations 2>&1 | Out-String
        
        # Parse results for each backend
        $backendResults = @{}
        $lines = $output -split "`n"
        
        foreach ($line in $lines) {
            # Parse CPAC backend results
            if ($line -match '^\s*(\w+)\s+ratio:\s+([\d.]+)x\s+compress:\s+([\d.]+)\s+MB/s\s+decompress:\s+([\d.]+)\s+MB/s\s+verified:\s+(\w+)') {
                $backend = $matches[1]
                $backendResults["CPAC_$backend"] = @{
                    Ratio = [double]$matches[2]
                    CompressSpeed = [double]$matches[3]
                    DecompressSpeed = [double]$matches[4]
                    Verified = $matches[5]
                }
            }
            # Parse baseline results (gzip-9, zstd-3, brotli-11, lzma-6)
            elseif ($line -match '^\s*(gzip-9|zstd-3|brotli-11|lzma-6)\s+ratio:\s+([\d.]+)x\s+compress:\s+([\d.]+)\s+MB/s\s+decompress:\s+([\d.]+)\s+MB/s') {
                $baseline = $matches[1]
                $backendResults["Baseline_$baseline"] = @{
                    Ratio = [double]$matches[2]
                    CompressSpeed = [double]$matches[3]
                    DecompressSpeed = [double]$matches[4]
                }
            }
        }
        
        $key = "$($item.Corpus)|$($item.File)"
        $results[$key] = $backendResults
        
        Write-Host "  ✓ Completed" -ForegroundColor Green
        Write-Host ""
        
    } catch {
        Write-Host "  ✗ Error: $_" -ForegroundColor Red
        Write-Host ""
    }
}

Write-Host "==============================" -ForegroundColor Cyan
Write-Host "Generating Markdown Tables" -ForegroundColor Cyan
Write-Host "==============================" -ForegroundColor Cyan
Write-Host ""

# Generate markdown table rows for each corpus
foreach ($key in $results.Keys) {
    $parts = $key -split '\|'
    $corpus = $parts[0]
    $file = $parts[1]
    $data = $results[$key]
    
    Write-Host "${corpus}: ${file}" -ForegroundColor Yellow
    
    # Extract values for table
    $cpacZstd = $data["CPAC_zstd"]
    $cpacBrotli = $data["CPAC_brotli"]
    $cpacGzip = $data["CPAC_gzip"]
    $cpacLzma = $data["CPAC_lzma"]
    $gzip9 = $data["Baseline_gzip-9"]
    $zstd3 = $data["Baseline_zstd-3"]
    $brotli11 = $data["Baseline_brotli-11"]
    $lzma6 = $data["Baseline_lzma-6"]
    
    # Find best ratio
    $allRatios = @()
    if ($cpacZstd) { $allRatios += @{Name = "CPAC Zstd"; Ratio = $cpacZstd.Ratio} }
    if ($cpacBrotli) { $allRatios += @{Name = "CPAC Brotli"; Ratio = $cpacBrotli.Ratio} }
    if ($cpacGzip) { $allRatios += @{Name = "CPAC Gzip"; Ratio = $cpacGzip.Ratio} }
    if ($cpacLzma) { $allRatios += @{Name = "CPAC Lzma"; Ratio = $cpacLzma.Ratio} }
    if ($gzip9) { $allRatios += @{Name = "Baseline gzip-9"; Ratio = $gzip9.Ratio} }
    if ($zstd3) { $allRatios += @{Name = "Baseline zstd-3"; Ratio = $zstd3.Ratio} }
    if ($brotli11) { $allRatios += @{Name = "Baseline brotli-11"; Ratio = $brotli11.Ratio} }
    if ($lzma6) { $allRatios += @{Name = "Baseline lzma-6"; Ratio = $lzma6.Ratio} }
    
    $best = ($allRatios | Sort-Object -Property Ratio -Descending | Select-Object -First 1)
    
    # Format table row
    $row = "| $file | "
    $row += if ($cpacZstd) { "$([math]::Round($cpacZstd.Ratio, 2))x @ $([math]::Round($cpacZstd.CompressSpeed, 0)) MB/s" } else { "TBD" }
    $row += " | "
    $row += if ($cpacBrotli) { "$([math]::Round($cpacBrotli.Ratio, 2))x @ $([math]::Round($cpacBrotli.CompressSpeed, 0)) MB/s" } else { "TBD" }
    $row += " | "
    $row += if ($cpacGzip) { "$([math]::Round($cpacGzip.Ratio, 2))x @ $([math]::Round($cpacGzip.CompressSpeed, 0)) MB/s" } else { "TBD" }
    $row += " | "
    $row += if ($cpacLzma) { "$([math]::Round($cpacLzma.Ratio, 2))x @ $([math]::Round($cpacLzma.CompressSpeed, 0)) MB/s" } else { "TBD" }
    $row += " | "
    $row += if ($gzip9) { "$([math]::Round($gzip9.Ratio, 2))x @ $([math]::Round($gzip9.CompressSpeed, 0)) MB/s" } else { "TBD" }
    $row += " | "
    $row += if ($zstd3) { "$([math]::Round($zstd3.Ratio, 2))x @ $([math]::Round($zstd3.CompressSpeed, 0)) MB/s" } else { "TBD" }
    $row += " | "
    $row += if ($brotli11) { "$([math]::Round($brotli11.Ratio, 2))x @ $([math]::Round($brotli11.CompressSpeed, 0)) MB/s" } else { "TBD" }
    $row += " | "
    $row += if ($lzma6) { "$([math]::Round($lzma6.Ratio, 2))x @ $([math]::Round($lzma6.CompressSpeed, 0)) MB/s" } else { "TBD" }
    $row += " | **$($best.Name)** |"
    
    Write-Host $row
    Write-Host ""
}

Write-Host ""
Write-Host "Copy the rows above and update BENCHMARKING.md" -ForegroundColor Green
Write-Host "Replace TBD rows in the appropriate corpus sections" -ForegroundColor Gray
Write-Host ""
