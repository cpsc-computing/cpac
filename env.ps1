# CPAC Rust Engine — Dev Environment Activation (Windows)
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
$env:RUST_LOG = "info"
Write-Host "CPAC Rust dev environment activated." -ForegroundColor Green
