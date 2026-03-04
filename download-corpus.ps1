#!/usr/bin/env pwsh
# Copyright (c) 2026 BitConcepts, LLC
# download-corpus.ps1 — Download real-world datasets for CPAC benchmarking.
#
# Usage:
#   .\download-corpus.ps1               # Download all datasets
#   .\download-corpus.ps1 -DataSet enwik8   # Download specific dataset
#
# Datasets:
#   enwik8    — First 10^8 bytes of enwiki XML dump (~100 MB, text+XML)
#   silesia   — Silesia corpus (~211 MB, mixed)
#   calgary   — Calgary corpus classic (~3 MB)
#   loghub    — Sample log files for log domain testing

param(
    [string]$DataSet = "all",
    [string]$OutDir = "corpus-external"
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$CorpusDir = Join-Path $ScriptDir $OutDir
New-Item -ItemType Directory -Path $CorpusDir -Force | Out-Null

function Download-File {
    param([string]$Url, [string]$Dest, [string]$Description)
    if (Test-Path $Dest) {
        Write-Host "  [skip] $Description (already exists)" -ForegroundColor DarkGray
        return
    }
    Write-Host "  Downloading $Description..." -ForegroundColor Yellow
    try {
        Invoke-WebRequest -Uri $Url -OutFile $Dest -UseBasicParsing
        $SizeKB = [math]::Round((Get-Item $Dest).Length / 1024, 1)
        Write-Host "    OK ($SizeKB KB)" -ForegroundColor Green
    } catch {
        Write-Host "    FAILED: $_" -ForegroundColor Red
    }
}

function Extract-GZ {
    param([string]$GzFile, [string]$OutFile)
    if (Test-Path $OutFile) { return }
    Write-Host "  Extracting $(Split-Path -Leaf $GzFile)..." -ForegroundColor Yellow
    $stream = [System.IO.File]::OpenRead($GzFile)
    $gz = [System.IO.Compression.GZipStream]::new($stream, [System.IO.Compression.CompressionMode]::Decompress)
    $out = [System.IO.File]::Create($OutFile)
    $gz.CopyTo($out)
    $out.Close(); $gz.Close(); $stream.Close()
    Write-Host "    OK" -ForegroundColor Green
}

# enwik8: first 10^8 bytes of English Wikipedia XML dump
if ($DataSet -in @("all","enwik8")) {
    Write-Host "=== enwik8 (100 MB Wikipedia XML) ===" -ForegroundColor Cyan
    $enwikDir = Join-Path $CorpusDir "enwik"
    New-Item -ItemType Directory -Path $enwikDir -Force | Out-Null
    $enwikGz = Join-Path $enwikDir "enwik8.gz"
    $enwik = Join-Path $enwikDir "enwik8"
    Download-File "https://mattmahoney.net/dc/enwik8.zip" $enwikGz "enwik8 (100 MB)"
    # Note: enwik8.zip contains enwik8, not gzip — user should unzip manually or use Expand-Archive
    if ((Test-Path $enwikGz) -and $enwikGz.EndsWith(".zip")) {
        if (-not (Test-Path $enwik)) {
            Write-Host "  Expanding enwik8.zip..." -ForegroundColor Yellow
            Expand-Archive -Path $enwikGz -DestinationPath $enwikDir -Force
            Write-Host "    OK" -ForegroundColor Green
        }
    }
}

# Calgary corpus
if ($DataSet -in @("all","calgary")) {
    Write-Host "=== Calgary Corpus ===" -ForegroundColor Cyan
    $calgDir = Join-Path $CorpusDir "calgary"
    New-Item -ItemType Directory -Path $calgDir -Force | Out-Null
    $calgaryGz = Join-Path $calgDir "calgary.tar.gz"
    Download-File "http://www.data-compression.info/files/corpora/largecalgarycorpus.zip" $calgaryGz "Calgary corpus"
    if (Test-Path $calgaryGz) {
        Write-Host "  Note: Expand $calgaryGz manually to $calgDir" -ForegroundColor DarkGray
    }
}

# Silesia corpus
if ($DataSet -in @("all","silesia")) {
    Write-Host "=== Silesia Corpus ===" -ForegroundColor Cyan
    $silDir = Join-Path $CorpusDir "silesia"
    New-Item -ItemType Directory -Path $silDir -Force | Out-Null
    # Individual files from GitHub mirror
    $silFiles = @(
        @{ name="dickens";   url="https://raw.githubusercontent.com/MiloszKrajewski/SilesiaCorpus/master/dickens" },
        @{ name="mozilla";   url="https://raw.githubusercontent.com/MiloszKrajewski/SilesiaCorpus/master/mozilla" },
        @{ name="xml";       url="https://raw.githubusercontent.com/MiloszKrajewski/SilesiaCorpus/master/xml" },
        @{ name="mr";        url="https://raw.githubusercontent.com/MiloszKrajewski/SilesiaCorpus/master/mr" },
        @{ name="samba";     url="https://raw.githubusercontent.com/MiloszKrajewski/SilesiaCorpus/master/samba" }
    )
    foreach ($f in $silFiles) {
        Download-File $f.url (Join-Path $silDir $f.name) "silesia/$($f.name)"
    }
}

# Synthetic JSON/JSONL for domain testing
if ($DataSet -in @("all","json")) {
    Write-Host "=== Synthetic JSON/JSONL ===" -ForegroundColor Cyan
    $jsonDir = Join-Path $CorpusDir "json"
    New-Item -ItemType Directory -Path $jsonDir -Force | Out-Null

    # Generate a 1 MB JSONL file with repetitive records
    $jsonlPath = Join-Path $jsonDir "events_1mb.jsonl"
    if (-not (Test-Path $jsonlPath)) {
        Write-Host "  Generating events_1mb.jsonl..." -ForegroundColor Yellow
        $sb = [System.Text.StringBuilder]::new()
        $domains = @("example.com","test.org","acme.io","widgets.net")
        $actions = @("login","logout","click","view","purchase","search")
        for ($i = 0; $i -lt 8000; $i++) {
            $ts = "2026-01-{0:D2}T{1:D2}:{2:D2}:{3:D2}Z" -f (($i % 28)+1),(($i % 24)),(($i*7)%60),(($i*13)%60)
            $dom = $domains[$i % $domains.Count]
            $act = $actions[$i % $actions.Count]
            $null = $sb.AppendLine("{`"timestamp`":`"$ts`",`"user_id`":$i,`"action`":`"$act`",`"domain`":`"$dom`",`"session`":$($i/10 -as [int]),`"score`":$($i % 100)}")
        }
        $sb.ToString() | Set-Content $jsonlPath -NoNewline
        $SizeKB = [math]::Round((Get-Item $jsonlPath).Length / 1024, 1)
        Write-Host "    OK ($SizeKB KB)" -ForegroundColor Green
    } else {
        Write-Host "  [skip] events_1mb.jsonl (already exists)" -ForegroundColor DarkGray
    }
}

Write-Host ""
Write-Host "Corpus ready in: $CorpusDir" -ForegroundColor Cyan
Write-Host "Use: cpac benchmark <file> --balanced" -ForegroundColor White
