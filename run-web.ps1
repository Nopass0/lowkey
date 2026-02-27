# ─── Lowkey VPN — Web App Runner (Windows) ──────────────────────────────────
# Usage:
#   .\run-web.ps1             → start dev server
#   .\run-web.ps1 -Prod       → start production (must build first)
#   .\run-web.ps1 -BuildProd  → build + start production

param(
    [switch]$Prod,
    [switch]$BuildProd
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$WebDir = Join-Path $ScriptDir "web"

function Write-Info([string]$m) { Write-Host "[INFO]  $m" -ForegroundColor Cyan }
function Write-Ok([string]$m)   { Write-Host "[ OK ]  $m" -ForegroundColor Green }
function Write-Err([string]$m)  { Write-Host "[ERR ]  $m" -ForegroundColor Red }

if (-not (Test-Path $WebDir)) { Write-Err "web\ not found"; exit 1 }
if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
    Write-Err "Node.js not found. Install from https://nodejs.org"
    exit 1
}

Set-Location $WebDir

# Load .env for API URL
$EnvFile = Join-Path $ScriptDir ".env"
if (Test-Path $EnvFile) {
    Get-Content $EnvFile | ForEach-Object {
        if ($_ -match '^\s*([^#][^=]+)=(.*)') {
            $key = $matches[1].Trim().Trim('"')
            $val = $matches[2].Trim().Trim('"')
            [System.Environment]::SetEnvironmentVariable($key, $val, "Process")
        }
    }
}

if (-not $env:NEXT_PUBLIC_API_URL) {
    $port = if ($env:API_PORT) { $env:API_PORT } else { "8080" }
    $env:NEXT_PUBLIC_API_URL = "http://localhost:$port"
    Write-Info "NEXT_PUBLIC_API_URL=$($env:NEXT_PUBLIC_API_URL)"
}

if (-not (Test-Path "node_modules")) {
    Write-Info "Installing npm dependencies..."
    & npm ci --legacy-peer-deps
}

if ($BuildProd) {
    Write-Info "Building production..."
    & npm run build
    $env:PORT = "3000"
    Write-Ok "Starting production server on port 3000..."
    node .next\standalone\server.js
} elseif ($Prod) {
    if (-not (Test-Path ".next\standalone")) {
        Write-Err ".next\standalone not found. Run: .\run-web.ps1 -BuildProd"
        exit 1
    }
    $env:PORT = "3000"
    Write-Ok "Starting production server on port 3000..."
    node .next\standalone\server.js
} else {
    Write-Ok "Starting development server on port 3000..."
    & npm run dev
}
