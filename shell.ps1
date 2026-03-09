#!/usr/bin/env pwsh
# CPAC Unified Shell Script (Windows)
# Handles both bootstrap (first-time setup) and command execution.
#
# Usage:
#   .\shell.ps1                    # Enter interactive venv shell
#   .\shell.ps1 build --release    # Run cpac.py build --release
#   .\shell.ps1 test               # Run cpac.py test
#   .\shell.ps1 bench file --quick # Run cpac.py bench file --quick

$ErrorActionPreference = "Stop"

$RepoRoot = $PSScriptRoot
$WorkDir = Join-Path $RepoRoot ".work"
$VenvDir = Join-Path $WorkDir "env"
$VenvPython = Join-Path (Join-Path $VenvDir "Scripts") "python.exe"
$VenvActivate = Join-Path (Join-Path $VenvDir "Scripts") "Activate.ps1"

# Check if venv exists
$needsBootstrap = -not (Test-Path $VenvPython)

if ($needsBootstrap) {
    Write-Host "========================================" -ForegroundColor Cyan
    Write-Host "CPAC First-Time Setup" -ForegroundColor Cyan
    Write-Host "========================================" -ForegroundColor Cyan

    # 1. Check Python 3
    Write-Host "`n[1/3] Checking Python..." -ForegroundColor Yellow

    $pythonCmd = Get-Command python -ErrorAction SilentlyContinue
    if ($pythonCmd) {
        $version = & python --version 2>&1
        Write-Host "Found: $version"

        if ($version -notmatch "Python 3\.") {
            Write-Host "ERROR: Python 3 required" -ForegroundColor Red
            Write-Host "Install: winget install Python.Python.3.12" -ForegroundColor Yellow
            exit 1
        }
    } else {
        Write-Host "ERROR: Python not found" -ForegroundColor Red
        Write-Host "Install: winget install Python.Python.3.12" -ForegroundColor Yellow
        exit 1
    }

    # 2. Create venv
    Write-Host "`n[2/3] Creating virtual environment..." -ForegroundColor Yellow
    New-Item -ItemType Directory -Force -Path $WorkDir | Out-Null
    & python -m venv $VenvDir
    Write-Host "Created venv at $VenvDir" -ForegroundColor Green

    # 3. Install requirements
    Write-Host "`n[3/3] Installing dependencies..." -ForegroundColor Yellow
    $requirementsFile = Join-Path $RepoRoot "requirements.txt"

    & $VenvPython -m pip install --quiet --upgrade pip
    if (Test-Path $requirementsFile) {
        & $VenvPython -m pip install --quiet -r $requirementsFile
    }
    Write-Host "Dependencies installed" -ForegroundColor Green

    Write-Host "`n========================================" -ForegroundColor Cyan
    Write-Host "Setup Complete!" -ForegroundColor Green
    Write-Host "========================================`n" -ForegroundColor Cyan
}

# Ensure Rust toolchain is on PATH (work around broken rustup shim)
$toolchainBin = Join-Path $env:USERPROFILE ".rustup\toolchains\stable-x86_64-pc-windows-msvc\bin"
if (Test-Path $toolchainBin) {
    $env:PATH = "$toolchainBin;$env:PATH"
}

# Execute command in venv
if ($args.Count -eq 0) {
    # No args: enter interactive shell inside the venv
    Write-Host "Entering CPAC venv shell. Type 'exit' to leave." -ForegroundColor Cyan
    Write-Host "  Python: $VenvPython" -ForegroundColor DarkGray
    Write-Host "  Cargo:  $(Get-Command cargo -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source)" -ForegroundColor DarkGray
    Write-Host ""
    & $VenvActivate
} else {
    # Run command via cpac.py
    & $VenvPython (Join-Path $RepoRoot "scripts\cpac.py") @args
    exit $LASTEXITCODE
}
