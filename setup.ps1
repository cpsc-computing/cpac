# CPAC Rust Engine — Windows Setup
# Installs Rust toolchain and verifies the workspace builds.

$ErrorActionPreference = "Stop"

Write-Host "=== CPAC Rust Engine Setup (Windows) ===" -ForegroundColor Cyan

# Check for rustup
if (-not (Get-Command rustup -ErrorAction SilentlyContinue)) {
    Write-Host "Installing Rust via rustup..." -ForegroundColor Yellow
    $installer = "$env:TEMP\rustup-init.exe"
    Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile $installer
    & $installer -y --default-toolchain stable
    Remove-Item $installer -ErrorAction SilentlyContinue

    # Refresh PATH
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
} else {
    Write-Host "rustup found: $(rustup --version)" -ForegroundColor Green
}

# Verify toolchain
Write-Host ""
Write-Host "Rust toolchain:" -ForegroundColor Cyan
rustc --version
cargo --version

# Ensure components
rustup component add rustfmt clippy 2>$null

# Build workspace
Write-Host ""
Write-Host "Building workspace..." -ForegroundColor Cyan
Push-Location $PSScriptRoot
cargo build --workspace
Pop-Location

Write-Host ""
Write-Host "Setup complete! Run .\env.ps1 to activate the dev environment." -ForegroundColor Green
