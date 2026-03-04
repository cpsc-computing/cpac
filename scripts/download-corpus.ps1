#!/usr/bin/env pwsh
# Corpus downloader for CPAC benchmark infrastructure
# Usage: .\download-corpus.ps1 -Corpus vctk,loghub2_2k,kodak

param(
    [string]$Corpus = "canterbury,calgary,silesia,vctk,loghub2_2k,kodak,digitalcorpora",
    [string]$TargetDir = ".work/benchdata"
)

$ErrorActionPreference = "Stop"

# Helper function for YAML parsing (basic)
function ConvertFrom-Yaml {
    param([Parameter(ValueFromPipeline)]$InputObject)
    
    try {
        # Use PowerShell-Yaml module if available
        if (Get-Module -ListAvailable -Name powershell-yaml) {
            Import-Module powershell-yaml -ErrorAction Stop
            return ConvertFrom-Yaml $InputObject
        }
    } catch {}
    
    # Fallback: basic parsing
    $lines = $InputObject -split "`n"
    $result = @{}
    
    foreach ($line in $lines) {
        if ($line -match '^\s*([^:#]+):\s*(.+)$') {
            $key = $matches[1].Trim()
            $value = $matches[2].Trim().Trim('"').Trim("'")
            $result[$key] = $value
        } elseif ($line -match '^\s*-\s*(.+)$') {
            # Array item
            if (-not $result.ContainsKey('download_url')) {
                $result['download_url'] = @()
            }
            if ($result['download_url'] -isnot [array]) {
                $result['download_url'] = @($result['download_url'])
            }
            $result['download_url'] += $matches[1].Trim().Trim('"').Trim("'")
        }
    }
    
    return [PSCustomObject]$result
}

Write-Host "CPAC Corpus Downloader" -ForegroundColor Cyan
Write-Host "======================" -ForegroundColor Cyan
Write-Host ""

$repoRoot = Split-Path -Parent $PSScriptRoot
$configDir = Join-Path $repoRoot "benches\configs"
$targetDir = Join-Path $repoRoot $TargetDir

if (-not (Test-Path $configDir)) {
    Write-Error "Config directory not found: $configDir"
    exit 1
}

New-Item -ItemType Directory -Force -Path $targetDir | Out-Null
Write-Host "Target directory: $targetDir" -ForegroundColor Green
Write-Host ""

$corpusList = $Corpus -split ','

foreach ($corpusId in $corpusList) {
    $corpusId = $corpusId.Trim()
    $configPath = Join-Path $configDir "corpus_$corpusId.yaml"
    
    if (-not (Test-Path $configPath)) {
        Write-Warning "Config not found: $configPath - Skipping"
        continue
    }
    
    Write-Host "Processing: $corpusId" -ForegroundColor Yellow
    
    # Parse YAML (basic parsing for our simple format)
    $config = Get-Content $configPath | Out-String | ConvertFrom-Yaml -ErrorAction SilentlyContinue
    
    if (-not $config) {
        Write-Warning "Failed to parse $configPath - Skipping"
        continue
    }
    
    $subdir = $config.target_subdir
    if (-not $subdir) {
        $subdir = $corpusId
    }
    
    $corpusTargetDir = Join-Path $targetDir $subdir
    
    if (Test-Path $corpusTargetDir) {
        Write-Host "  Already downloaded: $corpusTargetDir" -ForegroundColor Gray
        continue
    }
    
    Write-Host "  Downloading to: $corpusTargetDir" -ForegroundColor Green
    New-Item -ItemType Directory -Force -Path $corpusTargetDir | Out-Null
    
    # Use Rust corpus downloader if compiled
    $exePath = Join-Path $repoRoot "target\release\cpac.exe"
    if (Test-Path $exePath) {
        Write-Host "  Using Rust corpus downloader..." -ForegroundColor Cyan
        & $exePath corpus download $corpusId --target $targetDir
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "  Download failed for $corpusId"
        }
    } else {
        Write-Warning "  Rust binary not found. Run: cargo build --release --features download"
        Write-Host "  Attempting direct download..." -ForegroundColor Yellow
        
        # Fallback: direct download
        $urls = $config.download_url
        if ($urls -is [array]) {
            $count = 0
            foreach ($url in $urls) {
                $count++
                $filename = [System.IO.Path]::GetFileName($url)
                $destPath = Join-Path $corpusTargetDir $filename
                Write-Host "  [$count/$($urls.Count)] $filename" -ForegroundColor Gray
                
                try {
                    Invoke-WebRequest -Uri $url -OutFile $destPath -UseBasicParsing -TimeoutSec 300
                } catch {
                    Write-Warning "    Failed to download: $url"
                }
            }
        } else {
            Write-Host "  Downloading: $urls" -ForegroundColor Gray
            try {
                $tempFile = New-TemporaryFile
                Invoke-WebRequest -Uri $urls -OutFile $tempFile -UseBasicParsing -TimeoutSec 600
                
                # Extract if needed
                $kind = $config.download_kind
                if ($kind -eq "http_zip") {
                    Expand-Archive -Path $tempFile -DestinationPath $corpusTargetDir -Force
                    Write-Host "  Extracted ZIP archive" -ForegroundColor Green
                } elseif ($kind -eq "http_targz") {
                    # Requires tar (Windows 10+)
                    tar -xzf $tempFile -C $corpusTargetDir
                    Write-Host "  Extracted TAR.GZ archive" -ForegroundColor Green
                } else {
                    $filename = [System.IO.Path]::GetFileName($urls)
                    Copy-Item $tempFile (Join-Path $corpusTargetDir $filename)
                }
                
                Remove-Item $tempFile -ErrorAction SilentlyContinue
            } catch {
                Write-Warning "  Download failed: $_"
            }
        }
    }
    
    Write-Host ""
}

Write-Host "Corpus download complete!" -ForegroundColor Green
Write-Host ""
Write-Host "Downloaded corpora:"
Get-ChildItem $targetDir -Directory | ForEach-Object {
    $size = (Get-ChildItem $_.FullName -Recurse -File | Measure-Object -Property Length -Sum).Sum / 1MB
    Write-Host "  $($_.Name): $([math]::Round($size, 2)) MB" -ForegroundColor Cyan
}
