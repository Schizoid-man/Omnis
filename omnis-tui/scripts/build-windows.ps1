#Requires -Version 5.1
<#
.SYNOPSIS
    Native Windows build script for Omnis Desktop (Tauri + Next.js).
    Run this from the omnis-tui\ directory on your Windows PC.

.OUTPUTS
    NSIS installer: src-tauri\target\release\bundle\nsis\
    MSI installer:  src-tauri\target\release\bundle\msi\
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# ── Preflight checks ──────────────────────────────────────────────────────────
Write-Host "Checking prerequisites..." -ForegroundColor Cyan

if (-not (Get-Command "node" -ErrorAction SilentlyContinue)) {
    Write-Error "Node.js is not installed. Download from https://nodejs.org/"
}
if (-not (Get-Command "npm" -ErrorAction SilentlyContinue)) {
    Write-Error "npm is not available."
}
if (-not (Get-Command "cargo" -ErrorAction SilentlyContinue)) {
    Write-Error "Rust is not installed. Download from https://rustup.rs/"
}

$nodeVer  = node --version
$cargoVer = cargo --version
Write-Host "  Node.js : $nodeVer" -ForegroundColor Green
Write-Host "  Cargo   : $cargoVer" -ForegroundColor Green

# ── Install frontend dependencies ─────────────────────────────────────────────
Write-Host "`nInstalling frontend dependencies..." -ForegroundColor Cyan
npm --prefix web ci
if ($LASTEXITCODE -ne 0) { throw "npm ci failed" }

# ── Tauri build (NSIS + MSI) ──────────────────────────────────────────────────
Write-Host "`nBuilding Omnis Desktop for Windows..." -ForegroundColor Cyan
npm --prefix web run tauri -- build --bundles nsis,msi
if ($LASTEXITCODE -ne 0) { throw "tauri build failed" }

# ── Report output paths ───────────────────────────────────────────────────────
Write-Host "`nBuild complete! Artifacts:" -ForegroundColor Green

$bundleRoot = "src-tauri\target\release\bundle"
$nsis = Get-ChildItem -Path "$bundleRoot\nsis" -Filter "*.exe" -ErrorAction SilentlyContinue
$msi  = Get-ChildItem -Path "$bundleRoot\msi"  -Filter "*.msi"  -ErrorAction SilentlyContinue

foreach ($f in @($nsis) + @($msi)) {
    Write-Host "  $($f.FullName)" -ForegroundColor Yellow
}
