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


function Ensure-NodeToolchain {
    $requiredMajor = 20

    $nvmCmd = Get-Command nvm -ErrorAction SilentlyContinue
    if (-not $nvmCmd) {
        $nvmHome = if ($env:NVM_HOME) { $env:NVM_HOME } else { Join-Path $env:APPDATA "nvm" }
        $nvmExe = Join-Path $nvmHome "nvm.exe"
        if (Test-Path $nvmExe) {
            $env:Path = "$nvmHome;$env:Path"
            $nvmCmd = Get-Command nvm -ErrorAction SilentlyContinue
        }
    }

    $nodeCmd = Get-Command node -ErrorAction SilentlyContinue
    $currentMajor = $null
    if ($nodeCmd) {
        try {
            $nodeVersionRaw = (& node -v).Trim()
            if ($nodeVersionRaw.StartsWith("v")) { $nodeVersionRaw = $nodeVersionRaw.Substring(1) }
            $currentMajor = [int]($nodeVersionRaw.Split(".")[0])
        } catch {
            $currentMajor = $null
        }
    }

    if ($nvmCmd -and (-not $currentMajor -or $currentMajor -lt $requiredMajor)) {
        Write-Info "Switching Node.js to latest v$requiredMajor via nvm..."
        & nvm install $requiredMajor
        if ($LASTEXITCODE -ne 0) { Write-Err "nvm install failed"; exit 1 }
        & nvm use $requiredMajor
        if ($LASTEXITCODE -ne 0) { Write-Err "nvm use failed"; exit 1 }
    }
    elseif (-not $nvmCmd) {
        Write-Warn "nvm not found; using current system Node.js"
    }

    if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
        Write-Err "Node.js not found. Install nvm + Node.js 20+"
        exit 1
    }

    $currentVersionRaw = (& node --version).Trim()
    $currentVersionForCheck = if ($currentVersionRaw.StartsWith("v")) { $currentVersionRaw.Substring(1) } else { $currentVersionRaw }
    $currentMajorNow = [int]($currentVersionForCheck.Split(".")[0])
    if ($currentMajorNow -lt $requiredMajor) {
        Write-Err "Node.js $requiredMajor+ is required, found $currentVersionRaw"
        exit 1
    }

    if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
        Write-Err "npm not found. Install Node.js with npm"
        exit 1
    }

    Write-Ok "Node.js $currentVersionRaw"
    Write-Ok "npm $(& npm --version)"
}

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
    Write-Ok "-> dist\windows\vpn-client.exe"
}

function Build-Web {
    Write-Host "`n══ Building Next.js web app ══" -ForegroundColor Cyan

    $WebDir = Join-Path $ScriptDir "web"
    if (-not (Test-Path $WebDir)) { Write-Err "web\ not found"; exit 1 }
    Ensure-NodeToolchain

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
    Write-Ok "-> dist\web\ (Next.js standalone)"
    Write-Info "Run: node dist\web\server.js"
}

function Build-Desktop {
    Write-Host "`n══ Building Tauri desktop client ══" -ForegroundColor Cyan

    $DesktopDir = Join-Path $ScriptDir "vpn-desktop"
    if (-not (Test-Path $DesktopDir)) { Write-Err "vpn-desktop\ not found"; exit 1 }
    Ensure-NodeToolchain

    Install-NpmDeps -TargetDir $DesktopDir

    # Prompt for VPN server IP to bake into the binary as the default
    Write-Host ""
    Write-Warn "Enter the VPN server IP to embed as the default (e.g. 1.2.3.4)."
    Write-Warn "Leave blank to skip — users configure it in the app settings."
    $LowkeyServerIP = Read-Host "VPN server IP"
    if ($LowkeyServerIP -ne "") {
        $env:LOWKEY_SERVER_IP = $LowkeyServerIP
        $env:VITE_API_URL = "http://${LowkeyServerIP}:8080"
        Write-Info "Baking server IP: $LowkeyServerIP  (API URL: $($env:VITE_API_URL))"
    }

    Write-Info "Running: npm run tauri:build"
    & npm run tauri:build
    if ($LASTEXITCODE -ne 0) {
        if (Get-Command "npx" -ErrorAction SilentlyContinue) {
            Write-Warn "npm run tauri:build failed, retrying with npx tauri build..."
            & npx tauri build
        }
    }
    if ($LASTEXITCODE -ne 0) { Write-Err "Tauri build failed"; exit 1 }

    $DesktopDist = Join-Path $ScriptDir "dist\desktop"
    New-Item -ItemType Directory -Force $DesktopDist | Out-Null

    # Copy MSI/NSIS installer if built
    Get-ChildItem "src-tauri\target\release\bundle" -Recurse -Include "*.msi","*.exe","*.nsis" -ErrorAction SilentlyContinue |
        ForEach-Object {
            Copy-Item $_.FullName $DesktopDist
            Write-Ok "-> dist\desktop\$($_.Name)"
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
