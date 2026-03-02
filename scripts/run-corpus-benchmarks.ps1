#!/usr/bin/env pwsh
# Run comprehensive benchmarks across all downloaded corpora

param(
    [int]$Iterations = 3,
    [string]$OutputDir = ".work\benchmarks\corpus-results"
)

$ErrorActionPreference = "Stop"

Write-Host "CPAC Comprehensive Corpus Benchmark" -ForegroundColor Cyan
Write-Host "====================================" -ForegroundColor Cyan
Write-Host ""

$repoRoot = Split-Path -Parent $PSScriptRoot
$cpacExe = Join-Path $repoRoot "target\release\cpac.exe"
$benchdata = Join-Path $repoRoot ".work\benchdata"
$resultsDir = Join-Path $repoRoot $OutputDir

if (-not (Test-Path $cpacExe)) {
    Write-Error "CPAC binary not found. Run: cargo build --release"
    exit 1
}

New-Item -ItemType Directory -Force -Path $resultsDir | Out-Null

# Test configurations - representative files from each corpus
$tests = @(
    @{Name = "Canterbury - alice29.txt"; Path = "canterbury\alice29.txt"; Type = "Text"},
    @{Name = "Calgary - paper1"; Path = "calgary\paper1"; Type = "Text"},
    @{Name = "Silesia - dickens"; Path = "silesia\dickens"; Type = "Text"},
    @{Name = "Loghub - Linux"; Path = "logs\loghub-2.0\2k\Linux_2k.log"; Type = "System Logs"},
    @{Name = "Loghub - Apache"; Path = "logs\loghub-2.0\2k\Apache_2k.log"; Type = "Web Logs"},
    @{Name = "Loghub - HDFS"; Path = "logs\loghub-2.0\2k\HDFS_2k.log"; Type = "Big Data Logs"},
    @{Name = "Loghub - OpenStack"; Path = "logs\loghub-2.0\2k\OpenStack_2k.log"; Type = "Cloud Logs"}
)

# Discover available enwik files
$enwikDir = Join-Path $benchdata "enwik"
if (Test-Path $enwikDir) {
    Get-ChildItem $enwikDir -Filter "enwik*" | ForEach-Object {
        $tests += @{Name = "Wikipedia - $($_.Name)"; Path = "enwik\$($_.Name)"; Type = "Wikipedia XML"}
    }
}

$allResults = @()

foreach ($test in $tests) {
    $filePath = Join-Path $benchdata $test.Path
    
    if (-not (Test-Path $filePath)) {
        Write-Host "Skipping: $($test.Name) (file not found)" -ForegroundColor Gray
        continue
    }
    
    $fileSize = (Get-Item $filePath).Length / 1MB
    Write-Host "Testing: $($test.Name)" -ForegroundColor Yellow
    Write-Host "  Type: $($test.Type)" -ForegroundColor Gray
    Write-Host "  Size: $([math]::Round($fileSize, 2)) MB" -ForegroundColor Gray
    Write-Host ""
    
    # Run benchmark
    $output = & $cpacExe benchmark $filePath --iterations $Iterations 2>&1 | Out-String
    
    Write-Host $output
    Write-Host ""
    
    # Parse results (simple extraction)
    $lines = $output -split "`n"
    foreach ($line in $lines) {
        if ($line -match '^\s*([\w-]+)\s+ratio:\s+([\d.]+)x\s+compress:\s+([\d.]+)\s+MB/s\s+decompress:\s+([\d.]+)\s+MB/s') {
            $backend = $matches[1]
            $ratio = [double]$matches[2]
            $compressSpeed = [double]$matches[3]
            $decompressSpeed = [double]$matches[4]
            
            $allResults += [PSCustomObject]@{
                Corpus = $test.Name
                Type = $test.Type
                SizeMB = [math]::Round($fileSize, 2)
                Backend = $backend
                Ratio = $ratio
                CompressMBps = $compressSpeed
                DecompressMBps = $decompressSpeed
            }
        }
    }
}

Write-Host "====================================" -ForegroundColor Cyan
Write-Host "Summary Results" -ForegroundColor Cyan
Write-Host "====================================" -ForegroundColor Cyan
Write-Host ""

# Group by type
$allResults | Group-Object Type | ForEach-Object {
    Write-Host "$($_.Name):" -ForegroundColor Yellow
    $_.Group | Sort-Object Backend | Format-Table -AutoSize Corpus, Backend, Ratio, CompressMBps, DecompressMBps
    Write-Host ""
}

# Save results
$timestamp = Get-Date -Format 'yyyyMMdd_HHmmss'
$csvPath = Join-Path $resultsDir "corpus_benchmark_$timestamp.csv"
$allResults | Export-Csv -Path $csvPath -NoTypeInformation
Write-Host "Results saved to: $csvPath" -ForegroundColor Green

# Generate summary statistics
Write-Host ""
Write-Host "Top Performers:" -ForegroundColor Cyan
Write-Host ""

Write-Host "Best Compression Ratios:" -ForegroundColor Yellow
$allResults | Sort-Object -Property Ratio -Descending | Select-Object -First 5 Corpus, Backend, Ratio | Format-Table -AutoSize

Write-Host "Fastest Compression:" -ForegroundColor Yellow
$allResults | Sort-Object -Property CompressMBps -Descending | Select-Object -First 5 Corpus, Backend, CompressMBps | Format-Table -AutoSize

Write-Host "Fastest Decompression:" -ForegroundColor Yellow
$allResults | Sort-Object -Property DecompressMBps -Descending | Select-Object -First 5 Corpus, Backend, DecompressMBps | Format-Table -AutoSize
