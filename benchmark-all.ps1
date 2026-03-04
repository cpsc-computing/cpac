#!/usr/bin/env pwsh
# Copyright (c) 2026 BitConcepts, LLC
# benchmark-all.ps1 — Run the full CPAC benchmark suite and save timestamped results.
#
# Usage:
#   .\benchmark-all.ps1                      # Balanced mode
#   .\benchmark-all.ps1 -Mode quick          # Quick mode
#   .\benchmark-all.ps1 -Mode full           # Full mode (slow)
#   .\benchmark-all.ps1 -Json               # Include per-file JSON output
#   .\benchmark-all.ps1 -SkipBuild          # Skip cargo build step

param(
    [ValidateSet("quick","balanced","full")]
    [string]$Mode = "balanced",
    [switch]$Json,
    [switch]$SkipBuild,
    [switch]$SkipBaselines
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

# Timestamped output directory
$Timestamp = Get-Date -Format "yyyy-MM-dd_HH-mm"
$OutDir = Join-Path $ScriptDir "benchmark-results\$Timestamp"
New-Item -ItemType Directory -Path $OutDir -Force | Out-Null

Write-Host "CPAC Benchmark Automation" -ForegroundColor Cyan
Write-Host "  Mode:    $Mode" -ForegroundColor White
Write-Host "  Output:  $OutDir" -ForegroundColor White
Write-Host ""

# Build release binary
if (-not $SkipBuild) {
    Write-Host "Building release binary..." -ForegroundColor Yellow
    cargo build --release --package cpac-cli 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Build failed" }
    Write-Host "  Done." -ForegroundColor Green
}

$CpacBin = Join-Path $ScriptDir "target\release\cpac.exe"
if (-not (Test-Path $CpacBin)) {
    $CpacBin = "cargo run --release --package cpac-cli --"
}

# Find corpus files
$CorpusDir = Join-Path $ScriptDir "crates\cpac-engine\corpus"
$CorpusFiles = @()
if (Test-Path $CorpusDir) {
    $CorpusFiles = Get-ChildItem -Path $CorpusDir -File -Recurse |
        Where-Object { $_.Length -gt 0 } |
        Select-Object -ExpandProperty FullName
}

if ($CorpusFiles.Count -eq 0) {
    Write-Host "No corpus files found in $CorpusDir" -ForegroundColor Yellow
    Write-Host "Run: cargo test --package cpac-engine generate_corpus to create them" -ForegroundColor Yellow
    exit 0
}

Write-Host "Found $($CorpusFiles.Count) corpus files." -ForegroundColor White

$ModeFlag = "--$Mode"
$BaselineFlag = if ($SkipBaselines) { "--skip-baselines" } else { "" }

$AllResults = @()
$SummaryPath = Join-Path $OutDir "summary.txt"
$JsonPath = Join-Path $OutDir "results.json"

"CPAC Benchmark Results — $Timestamp ($Mode mode)" | Set-Content $SummaryPath
"=" * 70 | Add-Content $SummaryPath
"" | Add-Content $SummaryPath

$JsonResults = @()

foreach ($File in $CorpusFiles) {
    $FileName = Split-Path -Leaf $File
    $FileSizeKB = [math]::Round((Get-Item $File).Length / 1024, 1)
    Write-Host "  Benchmarking: $FileName ($FileSizeKB KB)..." -ForegroundColor White

    $LogFile = Join-Path $OutDir "$([System.IO.Path]::GetFileNameWithoutExtension($File)).txt"

    try {
        if ($SkipBuild) {
            $Args = @("benchmark", $File, $ModeFlag)
        } else {
            $Args = @("benchmark", $File, $ModeFlag)
        }
        if ($BaselineFlag) { $Args += $BaselineFlag }
        if ($Json) { $Args += "--json" }

        $Output = & $CpacBin @Args 2>&1
        $Output | Set-Content $LogFile
        $Output | Add-Content $SummaryPath

        if ($Json) {
            # Extract JSON array lines from output
            $JsonLines = $Output | Where-Object { $_ -match "^\s*[\[{]" -or $_ -match "^\s*\]" }
            if ($JsonLines) {
                $JsonResults += @{ file = $FileName; results = ($JsonLines -join "`n") }
            }
        }
        Write-Host "    OK" -ForegroundColor Green
    } catch {
        Write-Host "    FAILED: $_" -ForegroundColor Red
        "ERROR: $_" | Add-Content $SummaryPath
    }

    "" | Add-Content $SummaryPath
}

# Run Criterion benchmarks (workspace)
Write-Host ""
Write-Host "Running Criterion micro-benchmarks..." -ForegroundColor Yellow
$CriterionLog = Join-Path $OutDir "criterion.txt"
cargo bench --workspace 2>&1 | Tee-Object -FilePath $CriterionLog | Select-Object -Last 30
Write-Host "  Criterion output: $CriterionLog" -ForegroundColor White

# Write JSON summary
if ($Json -and $JsonResults.Count -gt 0) {
    $JsonResults | ConvertTo-Json -Depth 5 | Set-Content $JsonPath
    Write-Host "  JSON results: $JsonPath" -ForegroundColor White
}

Write-Host ""
Write-Host "Benchmark complete." -ForegroundColor Cyan
Write-Host "Results saved to: $OutDir" -ForegroundColor White
Write-Host "Summary: $SummaryPath" -ForegroundColor White
