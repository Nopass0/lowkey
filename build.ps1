# ─── Lowkey VPN — Windows Build Script ─────────────────────────────────────
# Usage:
#   .\build.ps1              → build all Windows components
#   .\build.ps1 -Component web       → build Next.js web app only
#   .\build.ps1 -Component desktop   → build Tauri desktop client only
#   .\build.ps1 -Component server    → build Rust server/client only

param(
    [string]$Component = "all"
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$DistDir = Join-Path $ScriptDir "dist\windows"

function Write-Info([string]$msg)  { Write-Host "[INFO]  $msg" -ForegroundColor Cyan }
function Write-Ok([string]$msg)    { Write-Host "[ OK ]  $msg" -ForegroundColor Green }
function Write-Warn([string]$msg)  { Write-Host "[WARN]  $msg" -ForegroundColor Yellow }
function Write-Err([string]$msg)   { Write-Host "[ERR ]  $msg" -ForegroundColor Red }

function Install-NpmDeps {
    param([string]$TargetDir)

    Set-Location $TargetDir

    if (Test-Path "package-lock.json") {
        Write-Info "Installing npm dependencies via npm ci..."
        & npm ci --legacy-peer-deps
        if ($LASTEXITCODE -eq 0) { return }
        Write-Warn "npm ci failed, retrying with npm install..."
    }
    else {
        Write-Warn "package-lock.json not found, using npm install..."
    }

    & npm install --legacy-peer-deps
    if ($LASTEXITCODE -ne 0) { Write-Err "npm install failed"; exit 1 }
}


function Build-Server {
    Write-Host "`n══ Building Rust VPN client (Windows) ══" -ForegroundColor Cyan

    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Write-Err "Rust/cargo not found. Install from https://rustup.rs"
        exit 1
    }

    Set-Location $ScriptDir
    & cargo build --release -p vpn-client
    if ($LASTEXITCODE -ne 0) { Write-Err "Cargo build failed"; exit 1 }

    New-Item -ItemType Directory -Force $DistDir | Out-Null
    Copy-Item "target\release\vpn-client.exe" $DistDir
    Write-Ok "→ dist\windows\vpn-client.exe"
}

function Build-Web {
    Write-Host "`n══ Building Next.js web app ══" -ForegroundColor Cyan

    $WebDir = Join-Path $ScriptDir "web"
    if (-not (Test-Path $WebDir)) { Write-Err "web\ not found"; exit 1 }
    if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
        Write-Err "Node.js not found. Install from https://nodejs.org"
        exit 1
    }

    Install-NpmDeps -TargetDir $WebDir

    & npm run build
    if ($LASTEXITCODE -ne 0) { Write-Err "npm build failed"; exit 1 }

    $WebDist = Join-Path $ScriptDir "dist\web"
    New-Item -ItemType Directory -Force $WebDist | Out-Null
    Copy-Item -Recurse -Force ".next\standalone\*" $WebDist
    Copy-Item -Recurse -Force ".next\static" "$WebDist\.next\static"
    if (Test-Path "public") {
        Copy-Item -Recurse -Force "public" "$WebDist\public"
    }
    Write-Ok "→ dist\web\ (Next.js standalone)"
    Write-Info "Run: node dist\web\server.js"
}

function Build-Desktop {
    Write-Host "`n══ Building Tauri desktop client ══" -ForegroundColor Cyan

    $DesktopDir = Join-Path $ScriptDir "vpn-desktop"
    if (-not (Test-Path $DesktopDir)) { Write-Err "vpn-desktop\ not found"; exit 1 }
    if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
        Write-Err "Node.js not found. Install from https://nodejs.org"
        exit 1
    }

    Install-NpmDeps -TargetDir $DesktopDir

    # Try npm run tauri build first, then fall back to npx
    $tauriCmd = if (Get-Command "npx" -ErrorAction SilentlyContinue) { "npx tauri build" } else { "npm run tauri build" }
    Write-Info "Running: $tauriCmd"
    Invoke-Expression $tauriCmd
    if ($LASTEXITCODE -ne 0) { Write-Err "Tauri build failed"; exit 1 }

    $DesktopDist = Join-Path $ScriptDir "dist\desktop"
    New-Item -ItemType Directory -Force $DesktopDist | Out-Null

    # Copy MSI/NSIS installer if built
    Get-ChildItem "src-tauri\target\release\bundle" -Recurse -Include "*.msi","*.exe","*.nsis" -ErrorAction SilentlyContinue |
        ForEach-Object {
            Copy-Item $_.FullName $DesktopDist
            Write-Ok "→ dist\desktop\$($_.Name)"
        }
}

switch ($Component.ToLower()) {
    "server"  { Build-Server }
    "web"     { Build-Web }
    "desktop" { Build-Desktop }
    "all"     { Build-Server; Build-Web; Build-Desktop }
    default   {
        Write-Err "Unknown component: $Component"
        Write-Host "Valid options: all, server, web, desktop"
        exit 1
    }
}

Write-Host ""
Write-Ok "Build complete. Output in dist\"
