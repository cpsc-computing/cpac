#!/usr/bin/env pwsh
# Copyright (c) 2026 BitConcepts, LLC
# SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
#
# download-corpus.ps1 — Download benchmark corpora for CPAC.
#
# Usage:
#   .\scripts\download-corpus.ps1                          # default set
#   .\scripts\download-corpus.ps1 -Corpus canterbury       # single corpus
#   .\scripts\download-corpus.ps1 -Corpus "silesia,calgary,loghub2_2k"
#
# YAML config files live in benches/configs/corpus_<id>.yaml
# Data is downloaded to .work/benchdata/<target_subdir>/

param(
    [string]$Corpus = "canterbury,calgary,silesia,loghub2_2k",
    [string]$TargetDir = ".work/benchdata"
)

$ErrorActionPreference = "Stop"

# ---------------------------------------------------------------------------
# Minimal YAML parser for corpus_*.yaml files.
# Handles: scalar fields and multi-line download_url lists.
# ---------------------------------------------------------------------------
function Parse-CorpusYaml {
    param([string]$Path)
    $raw   = Get-Content $Path -Raw
    $lines = $raw -split "`n"
    $cfg   = @{ download_url = [System.Collections.Generic.List[string]]::new() }
    $inUrlBlock = $false

    foreach ($line in $lines) {
        $line = $line.TrimEnd("`r")

        if ($line -match '^download_url:\s*$') {
            $inUrlBlock = $true; continue
        }
        if ($inUrlBlock -and $line -match '^\s+-\s+(.+)$') {
            $url = $matches[1].Trim().Split('#')[0].Trim().Trim('"').Trim("'")
            if ($url) { $cfg.download_url.Add($url) }
            continue
        }
        if ($inUrlBlock -and $line -notmatch '^\s') { $inUrlBlock = $false }

        if ($line -match '^([A-Za-z_][A-Za-z0-9_]*):\s+(.+)$') {
            $key   = $matches[1]
            $value = $matches[2].Trim().Trim('"').Trim("'")
            if ($key -eq 'download_url' -and $cfg.download_url.Count -eq 0) {
                $cfg.download_url.Add($value)
            } else {
                $cfg[$key] = $value
            }
        }
    }
    return $cfg
}

Write-Host "CPAC Corpus Downloader" -ForegroundColor Cyan
Write-Host "======================" -ForegroundColor Cyan
Write-Host ""

$repoRoot = Split-Path -Parent $PSScriptRoot
$configDir = Join-Path $repoRoot "benches\configs"
$resolvedTarget = Join-Path $repoRoot $TargetDir

if (-not (Test-Path $configDir)) {
    Write-Error "Config directory not found: $configDir"
    exit 1
}

New-Item -ItemType Directory -Force -Path $resolvedTarget | Out-Null
Write-Host "Target directory: $resolvedTarget" -ForegroundColor Green
Write-Host ""

$corpusList = $Corpus -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ }

foreach ($corpusId in $corpusList) {
    $configPath = Join-Path $configDir "corpus_$corpusId.yaml"

    if (-not (Test-Path $configPath)) {
        Write-Warning "Config not found: $configPath — skipping '$corpusId'"
        continue
    }

    Write-Host "[$corpusId]" -ForegroundColor Yellow

    $cfg    = Parse-CorpusYaml -Path $configPath
    $subdir = if ($cfg.target_subdir) { $cfg.target_subdir } else { $corpusId }
    $dest   = Join-Path $resolvedTarget $subdir

    # Skip if already populated
    if (Test-Path $dest) {
        $n = (Get-ChildItem $dest -Recurse -File -ErrorAction SilentlyContinue | Measure-Object).Count
        if ($n -gt 0) {
            $mb = [math]::Round((Get-ChildItem $dest -Recurse -File -ErrorAction SilentlyContinue |
                                  Measure-Object Length -Sum).Sum / 1MB, 1)
            Write-Host "  Already present: $dest ($n files, $mb MB)" -ForegroundColor DarkGray
            Write-Host ""
            continue
        }
    }

    New-Item -ItemType Directory -Force -Path $dest | Out-Null
    Write-Host "  Downloading to: $dest" -ForegroundColor Green

    $kind = $cfg.download_kind
    $urls = @($cfg.download_url)

    try {
        if ($urls.Count -gt 1 -or $kind -eq 'http_file_multi') {
            # Multiple individual files
            $i = 0
            foreach ($url in $urls) {
                $i++
                $filename = [System.IO.Path]::GetFileName($url.Split('?')[0])
                $outPath  = Join-Path $dest $filename
                Write-Host "  [$i/$($urls.Count)] $filename" -ForegroundColor Gray
                try {
                    Invoke-WebRequest -Uri $url -OutFile $outPath -UseBasicParsing -TimeoutSec 300
                } catch {
                    Write-Warning "    FAILED: $url`n    $_"
                }
            }
        } else {
            # Single archive
            $url = $urls[0]
            Write-Host "  Downloading: $url" -ForegroundColor Gray
            $tmpExt  = if ($kind -eq 'http_targz' -or $url -match '\.tar\.gz$|\.tgz$') { '.tar.gz' }
                       elseif ($kind -eq 'http_zip' -or $url -match '\.zip$') { '.zip' }
                       else { [System.IO.Path]::GetExtension($url) }
            $tmpFile = [System.IO.Path]::GetTempFileName() + $tmpExt
            Invoke-WebRequest -Uri $url -OutFile $tmpFile -UseBasicParsing -TimeoutSec 600
            if ($tmpExt -eq '.zip') {
                Write-Host "  Extracting ZIP..." -ForegroundColor Gray
                Expand-Archive -Path $tmpFile -DestinationPath $dest -Force
            } elseif ($tmpExt -eq '.tar.gz') {
                Write-Host "  Extracting TAR.GZ..." -ForegroundColor Gray
                tar -xzf $tmpFile -C $dest
            } else {
                Copy-Item $tmpFile (Join-Path $dest ([System.IO.Path]::GetFileName($url.Split('?')[0])))
            }
            Remove-Item $tmpFile -ErrorAction SilentlyContinue
        }

        $n  = (Get-ChildItem $dest -Recurse -File -ErrorAction SilentlyContinue | Measure-Object).Count
        $mb = [math]::Round((Get-ChildItem $dest -Recurse -File -ErrorAction SilentlyContinue |
                              Measure-Object Length -Sum).Sum / 1MB, 1)
        Write-Host "  Done: $n files, $mb MB" -ForegroundColor Green
    } catch {
        Write-Warning "  Download failed for '${corpusId}': $_"
        if ((Get-ChildItem $dest -ErrorAction SilentlyContinue | Measure-Object).Count -eq 0) {
            Remove-Item $dest -Force -Recurse -ErrorAction SilentlyContinue
        }
    }
    Write-Host ""
}

# Summary
Write-Host "Summary:" -ForegroundColor Cyan
Get-ChildItem $resolvedTarget -Directory -ErrorAction SilentlyContinue | Sort-Object Name | ForEach-Object {
    $mb    = [math]::Round((Get-ChildItem $_.FullName -Recurse -File -ErrorAction SilentlyContinue |
                             Measure-Object Length -Sum).Sum / 1MB, 1)
    $files = (Get-ChildItem $_.FullName -Recurse -File -ErrorAction SilentlyContinue | Measure-Object).Count
    Write-Host "  $($_.Name.PadRight(22)) $($mb.ToString().PadLeft(8)) MB   $files files" -ForegroundColor Cyan
}
