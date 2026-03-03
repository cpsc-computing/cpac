#!/usr/bin/env pwsh
# CPAC Batch Benchmark Runner
# Runs benchmarks across industry-standard corpora and generates reports

param(
    [Parameter(Mandatory=$false)]
    [ValidateSet("quick", "balanced", "full")]
    [string]$Mode = "balanced",
    
    [Parameter(Mandatory=$false)]
    [string]$OutputDir = ".work/benchmark_results",
    
    [Parameter(Mandatory=$false)]
    [switch]$SkipBaselines
)

$ErrorActionPreference = "Stop"

# Color output helpers
function Write-Header { param($Text) Write-Host "`n=== $Text ===" -ForegroundColor Cyan }
function Write-Success { param($Text) Write-Host "✓ $Text" -ForegroundColor Green }
function Write-Info { param($Text) Write-Host "→ $Text" -ForegroundColor Yellow }

# Corpus configurations based on mode
$CorpusConfig = @{
    quick = @{
        canterbury = @("alice29.txt", "lcet10.txt", "plrabn12.txt")
        silesia = @("dickens", "xml")
        iterations = 3
    }
    balanced = @{
        canterbury = @("alice29.txt", "asyoulik.txt", "kennedy.xls", "lcet10.txt", "plrabn12.txt")
        silesia = @("dickens", "mozilla", "xml", "samba")
        calgary = @("book1", "geo", "news", "pic")
        iterations = 10
    }
    full = @{
        canterbury = "all"
        silesia = "all"
        calgary = "all"
        iterations = 50
    }
}

$Config = $CorpusConfig[$Mode]
$Timestamp = Get-Date -Format "yyyyMMdd_HHmmss"
$ResultsFile = "$OutputDir/results_$Timestamp.csv"
$SummaryFile = "$OutputDir/summary_$Timestamp.md"

# Create output directory
New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null

Write-Header "CPAC Benchmark Runner - $Mode mode"
Write-Info "Output: $OutputDir"
Write-Info "Iterations: $($Config.iterations)"
Write-Info ""

# CSV header
"Corpus,File,Size_Bytes,CPAC_Backend,Ratio,Compress_MBps,Decompress_MBps,Baseline_Engine,Baseline_Ratio,Baseline_Compress_MBps" | Out-File -FilePath $ResultsFile -Encoding UTF8

$AllResults = @()

function Run-Benchmark {
    param($CorpusName, $FilePath)
    
    if (-not (Test-Path $FilePath)) {
        Write-Warning "File not found: $FilePath"
        return
    }
    
    $FileName = Split-Path $FilePath -Leaf
    $FileSize = (Get-Item $FilePath).Length
    
    Write-Info "Benchmarking: $CorpusName/$FileName ($([math]::Round($FileSize/1MB, 2)) MB)"
    
    # Run benchmark
    $BenchArgs = @("benchmark", $FilePath)
    if ($Mode -eq "quick") {
        $BenchArgs += "--quick"
    } elseif ($Mode -eq "full") {
        $BenchArgs += "--full"
    }
    if ($SkipBaselines) {
        $BenchArgs += "--skip-baselines"
    }
    
    $Output = & cargo run -p cpac-cli --release -- @BenchArgs 2>&1 | Out-String
    
    # Parse results (simplified - could be enhanced with structured output)
    $Lines = $Output -split "`n"
    foreach ($Line in $Lines) {
        if ($Line -match '^\s+(\w+)\s+ratio:\s+([\d.]+)x\s+compress:\s+([\d.]+)\s+MB/s\s+decompress:\s+([\d.]+)\s+MB/s') {
            $Engine = $Matches[1]
            $Ratio = $Matches[2]
            $CompressMBps = $Matches[3]
            $DecompressMBps = $Matches[4]
            
            $IsBaseline = $Engine -in @("gzip-9", "zstd-3", "brotli-11", "lzma-6")
            $Backend = if ($IsBaseline) { "" } else { $Engine }
            $BaselineEngine = if ($IsBaseline) { $Engine } else { "" }
            
            "$CorpusName,$FileName,$FileSize,$Backend,$Ratio,$CompressMBps,$DecompressMBps,$BaselineEngine,," | 
                Out-File -FilePath $ResultsFile -Append -Encoding UTF8
            
            $AllResults = $AllResults + [PSCustomObject]@{
                Corpus = $CorpusName
                File = $FileName
                Size = $FileSize
                Engine = $Engine
                Ratio = [double]$Ratio
                CompressMBps = [double]$CompressMBps
                DecompressMBps = [double]$DecompressMBps
            }
        }
    }
}

# Run benchmarks for each corpus
foreach ($CorpusName in $Config.Keys) {
    if ($CorpusName -eq "iterations") { continue }
    
    $CorpusPath = ".work/benchdata/$CorpusName"
    if (-not (Test-Path $CorpusPath)) {
        Write-Warning "Corpus not found: $CorpusName (skipping)"
        continue
    }
    
    Write-Header "Corpus: $CorpusName"
    
    $Files = $Config[$CorpusName]
    if ($Files -eq "all") {
        $Files = Get-ChildItem $CorpusPath -File | Select-Object -ExpandProperty Name
    }
    
    foreach ($File in $Files) {
        Run-Benchmark $CorpusName "$CorpusPath/$File"
    }
}

Write-Header "Generating Summary Report"

# Generate Markdown summary
$Summary = @"
# CPAC Benchmark Results
**Date**: $(Get-Date -Format "yyyy-MM-dd HH:mm:ss")  
**Mode**: $Mode  
**Iterations**: $($Config.iterations)  
**Host**: $env:COMPUTERNAME  

## Summary Statistics

"@

# Calculate aggregate statistics
$CPACResults = $AllResults | Where-Object { $_.Engine -notin @("gzip-9", "zstd-3", "brotli-11", "lzma-6") }
$BaselineResults = $AllResults | Where-Object { $_.Engine -in @("gzip-9", "zstd-3", "brotli-11", "lzma-6") }

if ($CPACResults.Count -gt 0) {
    $AvgRatio = ($CPACResults | Measure-Object -Property Ratio -Average).Average
    $AvgCompressSpeed = ($CPACResults | Measure-Object -Property CompressMBps -Average).Average
    $AvgDecompressSpeed = ($CPACResults | Measure-Object -Property DecompressMBps -Average).Average
    
    $Summary += @"

### CPAC Performance
- **Average Compression Ratio**: $([math]::Round($AvgRatio, 2))x
- **Average Compression Speed**: $([math]::Round($AvgCompressSpeed, 2)) MB/s
- **Average Decompression Speed**: $([math]::Round($AvgDecompressSpeed, 2)) MB/s

"@
}

# Add per-corpus breakdown
$Summary += "`n## Results by Corpus`n`n"

foreach ($CorpusName in ($AllResults | Select-Object -ExpandProperty Corpus -Unique)) {
    $CorpusResults = $AllResults | Where-Object { $_.Corpus -eq $CorpusName }
    $Summary += "### $CorpusName`n`n"
    $Summary += "| File | Engine | Ratio | Compress (MB/s) | Decompress (MB/s) |`n"
    $Summary += "|------|--------|-------|----------------|------------------|`n"
    
    foreach ($Result in $CorpusResults) {
        $Summary += "| $($Result.File) | $($Result.Engine) | $([math]::Round($Result.Ratio, 2))x | $([math]::Round($Result.CompressMBps, 2)) | $([math]::Round($Result.DecompressMBps, 2)) |`n"
    }
    $Summary += "`n"
}

# Add corpus citations
$Summary += @"

## Corpus Citations

### Canterbury Corpus
Ross Arnold and Timothy Bell, "A corpus for the evaluation of lossless compression algorithms,"  
Proceedings of Data Compression Conference (DCC'97), Snowbird, Utah, March 1997.  
Available at: https://corpus.canterbury.ac.nz/

### Silesia Corpus
Silesian University of Technology, "Silesia Compression Corpus"  
Available at: https://sun.aei.polsl.pl/~sdeor/index.php?page=silesia

### Calgary Corpus
University of Calgary, "Calgary Compression Corpus"  
Available at: https://corpus.canterbury.ac.nz/resources/calgary.tar.gz

---
*Generated by CPAC Benchmark Runner*
"@

$Summary | Out-File -FilePath $SummaryFile -Encoding UTF8

Write-Success "Results saved to: $ResultsFile"
Write-Success "Summary saved to: $SummaryFile"
Write-Header "Benchmark Complete!"
