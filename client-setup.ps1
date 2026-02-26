<#
.SYNOPSIS
    Lowkey VPN Client -- Full Setup & Connect Script (Windows / PowerShell)

.DESCRIPTION
    First-run script for the Lowkey VPN client on Windows.
      1. Checks / installs Rust toolchain (via rustup-init.exe)
      2. Builds vpn-client.exe in release mode
      3. Prompts for server address, username, password
      4. Registers a new account OR logs into an existing one
      5. Applies a promo / trial code for a free subscription (TRIAL30)
      6. Shows subscription status
      7. Connects via SOCKS5 and sets the Windows system proxy automatically

    On Windows only SOCKS5 mode is available (TUN requires Linux/macOS).
    After connecting, all apps that honour the system proxy will use the VPN.

.PARAMETER Connect
    Skip setup prompts and reconnect using saved credentials.

.PARAMETER Build
    Rebuild the binary only, do not connect.

.PARAMETER Status
    Show account and subscription info, then exit.

.PARAMETER SocksPort
    Override the local SOCKS5 listen port (default 1080).

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File .\client-setup.ps1
    powershell -ExecutionPolicy Bypass -File .\client-setup.ps1 -Connect
    powershell -ExecutionPolicy Bypass -File .\client-setup.ps1 -Status
    powershell -ExecutionPolicy Bypass -File .\client-setup.ps1 -Build
#>

[CmdletBinding()]
param(
    [switch]$Connect,
    [switch]$Build,
    [switch]$Status,
    [int]   $SocksPort = 0
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
function Write-Info ([string]$m) { Write-Host "[INFO]  $m" -ForegroundColor Cyan   }
function Write-Ok   ([string]$m) { Write-Host "[ OK ]  $m" -ForegroundColor Green  }
function Write-Warn ([string]$m) { Write-Host "[WARN]  $m" -ForegroundColor Yellow }
function Write-Err  ([string]$m) { Write-Host "[ERR ]  $m" -ForegroundColor Red    }

function Write-Section ([string]$title) {
    Write-Host ""
    Write-Host ("=" * 50) -ForegroundColor Cyan
    Write-Host "  $title" -ForegroundColor Cyan
    Write-Host ("=" * 50) -ForegroundColor Cyan
}

function Write-Banner ([string[]]$lines) {
    $width = ($lines | Measure-Object -Maximum -Property Length).Maximum + 4
    $bar   = '+' + ('-' * $width) + '+'
    Write-Host $bar -ForegroundColor Green
    foreach ($l in $lines) {
        $pad = $width - $l.Length
        Write-Host ("| " + $l + (' ' * $pad) + " |") -ForegroundColor Green
    }
    Write-Host $bar -ForegroundColor Green
}

# ---------------------------------------------------------------------------
# Paths
# ---------------------------------------------------------------------------
$ScriptDir   = Split-Path -Parent $MyInvocation.MyCommand.Path
$ClientDir   = Join-Path $ScriptDir "vpn-client"
# Cargo workspace puts the binary in the workspace root target\, not the crate's target\
$Binary      = Join-Path $ScriptDir "target\release\vpn-client.exe"
$ConfDir     = Join-Path $env:USERPROFILE ".config\lowkey"
$ConfFile    = Join-Path $ConfDir "client.conf"
$SessionFile = Join-Path $ConfDir "session.json"

# ---------------------------------------------------------------------------
# Config load / save
# ---------------------------------------------------------------------------
$Conf = [ordered]@{
    ServerAddr = ""
    ApiPort    = "8080"
    UdpPort    = "51820"
    ProxyPort  = "8388"
    SocksPort  = "1080"
}

function Load-Conf {
    if (Test-Path $ConfFile) {
        foreach ($line in (Get-Content $ConfFile)) {
            if ($line -match '^\s*(\w+)\s*=\s*"?([^"#]*)"?\s*$') {
                $Conf[$Matches[1]] = $Matches[2].Trim()
            }
        }
    }
}

function Save-Conf {
    New-Item -ItemType Directory -Force -Path $ConfDir | Out-Null
    @"
# Lowkey VPN Client -- saved configuration
ServerAddr=$($Conf.ServerAddr)
ApiPort=$($Conf.ApiPort)
UdpPort=$($Conf.UdpPort)
ProxyPort=$($Conf.ProxyPort)
SocksPort=$($Conf.SocksPort)
"@ | Set-Content -Encoding UTF8 $ConfFile
}

Load-Conf

if ($SocksPort -gt 0) {
    $Conf.SocksPort = "$SocksPort"
}

# ---------------------------------------------------------------------------
# API helpers (no jq, no curl -- pure PowerShell)
# ---------------------------------------------------------------------------
function Invoke-ApiAnon {
    param([string]$Path, [hashtable]$Body)
    $url = "http://$($Conf.ServerAddr):$($Conf.ApiPort)$Path"
    try {
        return Invoke-RestMethod -Uri $url -Method Post `
            -ContentType 'application/json' `
            -Body ($Body | ConvertTo-Json -Compress)
    } catch {
        $code   = $_.Exception.Response.StatusCode.value__
        $detail = $_.ErrorDetails.Message
        throw "HTTP $code -- $detail"
    }
}

function Invoke-ApiAuth {
    param(
        [string]   $Method = 'GET',
        [string]   $Path,
        [string]   $Token,
        [hashtable]$Body = @{}
    )
    $url     = "http://$($Conf.ServerAddr):$($Conf.ApiPort)$Path"
    $headers = @{ Authorization = "Bearer $Token" }
    try {
        if ($Method -eq 'GET') {
            return Invoke-RestMethod -Uri $url -Method Get -Headers $headers
        } else {
            return Invoke-RestMethod -Uri $url -Method $Method `
                -Headers $headers `
                -ContentType 'application/json' `
                -Body ($Body | ConvertTo-Json -Compress)
        }
    } catch {
        $code   = $_.Exception.Response.StatusCode.value__
        $detail = $_.ErrorDetails.Message
        throw "HTTP $code -- $detail"
    }
}

# ---------------------------------------------------------------------------
# Session helpers
# ---------------------------------------------------------------------------
function Get-SavedToken {
    if (Test-Path $SessionFile) {
        try {
            $s = Get-Content $SessionFile -Raw | ConvertFrom-Json
            return $s.token
        } catch { }
    }
    return $null
}

function Save-Session ([string]$Token) {
    New-Item -ItemType Directory -Force -Path $ConfDir | Out-Null
    [pscustomobject]@{
        token    = $Token
        server   = $Conf.ServerAddr
        api_port = [int]$Conf.ApiPort
    } | ConvertTo-Json | Set-Content -Encoding UTF8 $SessionFile
}

# ---------------------------------------------------------------------------
# Registry -- Windows system SOCKS5 proxy
# ---------------------------------------------------------------------------
$RegProxy = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings"

function Enable-SystemProxy ([int]$Port) {
    Set-ItemProperty -Path $RegProxy -Name ProxyServer -Value "socks=127.0.0.1:$Port"
    Set-ItemProperty -Path $RegProxy -Name ProxyEnable -Value 1
    Write-Ok "System proxy set: SOCKS5 127.0.0.1:$Port"
}

function Disable-SystemProxy {
    Set-ItemProperty -Path $RegProxy -Name ProxyEnable -Value 0
    Write-Info "System proxy cleared."
}

# ===========================================================================
# 1. RUST TOOLCHAIN
# ===========================================================================
Write-Section "Rust toolchain"

$CargoExe = $null
$candidates = @("$env:USERPROFILE\.cargo\bin\cargo.exe")
$fromPath = (Get-Command cargo -ErrorAction SilentlyContinue)
if ($fromPath) { $candidates += $fromPath.Source }

foreach ($c in $candidates) {
    if ($c -and (Test-Path $c)) {
        $CargoExe = $c
        break
    }
}

if ($CargoExe) {
    Write-Ok "Rust found at $CargoExe"
} else {
    Write-Info "Rust not found -- downloading rustup-init.exe ..."
    $installer = Join-Path $env:TEMP "rustup-init.exe"
    Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" `
        -OutFile $installer -UseBasicParsing
    Write-Info "Running rustup installer (takes a few minutes) ..."
    Start-Process -FilePath $installer `
        -ArgumentList "-y", "--no-modify-path" `
        -Wait -NoNewWindow
    $CargoExe = "$env:USERPROFILE\.cargo\bin\cargo.exe"
    if (-not (Test-Path $CargoExe)) {
        Write-Err "Rust installation failed."
        Write-Err "Install manually from https://rustup.rs and re-run this script."
        exit 1
    }
    Write-Ok "Rust installed."
}

$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

# ===========================================================================
# 2. BUILD
# ===========================================================================
Write-Section "Building vpn-client (release)"
Write-Info "Running: cargo build --release"
Write-Info "(first build may take a few minutes)"

# Build from workspace root so the binary lands in <workspace>\target\release\
Push-Location $ScriptDir
try {
    & $CargoExe build --release -p vpn-client
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build failed with exit code $LASTEXITCODE"
    }
} finally {
    Pop-Location
}

if (-not (Test-Path $Binary)) {
    Write-Err "Binary not found after build: $Binary"
    Write-Err "Check cargo output above for errors."
    exit 1
}
Write-Ok "Build complete: $Binary"

if ($Build) {
    Write-Ok "Build-only mode -- done."
    exit 0
}

# ===========================================================================
# 3. SERVER CONFIGURATION
# ===========================================================================
Write-Section "Server configuration"
Write-Host ""
Write-Host "Enter VPN server details. Press Enter to keep current values." `
    -ForegroundColor Yellow
Write-Host ""

# Server address
$curServer = $Conf.ServerAddr
if ($curServer) {
    $inp = Read-Host "  Server IP or hostname [$curServer]"
    if ($inp.Trim()) { $Conf.ServerAddr = $inp.Trim() }
} else {
    do {
        $inp = Read-Host "  Server IP or hostname (required)"
    } while (-not $inp.Trim())
    $Conf.ServerAddr = $inp.Trim()
}

# Reachability check
Write-Info "Checking $($Conf.ServerAddr):$($Conf.ApiPort) ..."
$StatusResp = $null

try {
    $StatusResp = Invoke-RestMethod `
        -Uri "http://$($Conf.ServerAddr):$($Conf.ApiPort)/api/status" `
        -TimeoutSec 5
} catch {
    Write-Warn "Could not reach port $($Conf.ApiPort). Try a different API port."
    $inp = Read-Host "  API port [$($Conf.ApiPort)]"
    if ($inp -match '^\d+$') { $Conf.ApiPort = $inp.Trim() }

    try {
        $StatusResp = Invoke-RestMethod `
            -Uri "http://$($Conf.ServerAddr):$($Conf.ApiPort)/api/status" `
            -TimeoutSec 5
    } catch {
        Write-Err "Still cannot reach the server. Check the IP, port and firewall."
        exit 1
    }
}

Write-Ok "Server is reachable."

# Use advertised ports
if ($StatusResp.udp_port)   { $Conf.UdpPort   = "$($StatusResp.udp_port)"   }
if ($StatusResp.proxy_port) { $Conf.ProxyPort  = "$($StatusResp.proxy_port)" }
Write-Info "UDP port   : $($Conf.UdpPort)"
Write-Info "Proxy port : $($Conf.ProxyPort)"

Save-Conf

# ===========================================================================
# 4. ACCOUNT -- REGISTER OR LOGIN
# ===========================================================================
Write-Section "Account setup"

$Token    = $null
$LoggedIn = $false

if (-not $Connect) {
    $savedToken = Get-SavedToken
    if ($savedToken) {
        try {
            $me = Invoke-ApiAuth -Path "/auth/me" -Token $savedToken
            Write-Ok "Already logged in as '$($me.login)'."
            $Token    = $savedToken
            $LoggedIn = $true
        } catch {
            Write-Warn "Saved session expired -- please log in again."
        }
    }
}

if (-not $LoggedIn) {
    Write-Host ""
    $hasAcc = Read-Host "  Do you have an existing account on this server? [y/N]"
    Write-Host ""
    $loginName = Read-Host "  Username"
    $secPass   = Read-Host "  Password" -AsSecureString
    $plainPass = [Runtime.InteropServices.Marshal]::PtrToStringAuto(
                    [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secPass))

    if ($hasAcc -match '^[Yy]') {
        Write-Info "Logging in as '$loginName' ..."
        try {
            $authResp = Invoke-ApiAnon -Path "/auth/login" `
                -Body @{ login = $loginName; password = $plainPass }
        } catch {
            Write-Err "Login failed: $_"
            exit 1
        }
        Write-Ok "Logged in as '$loginName'."
    } else {
        Write-Info "Creating account '$loginName' ..."
        try {
            $authResp = Invoke-ApiAnon -Path "/auth/register" `
                -Body @{ login = $loginName; password = $plainPass }
        } catch {
            Write-Err "Registration failed: $_"
            exit 1
        }
        Write-Ok "Account '$loginName' created."
    }

    $Token = $authResp.token
    if (-not $Token) {
        Write-Err "No token received. Check server logs."
        exit 1
    }

    Save-Session -Token $Token
    Write-Ok "Session saved to $SessionFile"
}

# ===========================================================================
# 5. SUBSCRIPTION CHECK & TRIAL ACTIVATION
# ===========================================================================
Write-Section "Subscription"

$subStatus = "unknown"
$subResp   = $null

try {
    $subResp   = Invoke-ApiAuth -Path "/subscription/status" -Token $Token
    $subStatus = $subResp.sub_status
} catch {
    Write-Warn "Could not fetch subscription status: $_"
}

Write-Host ""
Write-Host "  Subscription : " -NoNewline
Write-Host $subStatus -ForegroundColor Yellow
if ($subResp -and $subResp.sub_expires_at) {
    Write-Host "  Expires      : $($subResp.sub_expires_at)"
}
Write-Host ""

if ($subStatus -ne "active") {
    Write-Host "  No active subscription." -ForegroundColor Yellow
    Write-Host ""
    Write-Host "  Options:" -ForegroundColor Cyan
    Write-Host "    1) Apply a promo / trial code  (TRIAL30 = 30 free days)" `
        -ForegroundColor Green
    Write-Host "    2) View available plans and buy" -ForegroundColor Green
    Write-Host "    3) Skip (server will reject the VPN connection)" `
        -ForegroundColor Green
    Write-Host ""
    $choice = Read-Host "  Choice [1]"
    if (-not $choice) { $choice = "1" }

    switch ($choice.Trim()) {
        "1" {
            Write-Host ""
            Write-Host "  If the server admin ran server-setup.sh the code is: " `
                -NoNewline
            Write-Host "TRIAL30" -ForegroundColor Green
            $promoCode = Read-Host "  Enter promo code [TRIAL30]"
            if (-not $promoCode.Trim()) { $promoCode = "TRIAL30" }

            try {
                $pr = Invoke-ApiAuth -Method Post -Path "/promo/apply" `
                    -Token $Token -Body @{ code = $promoCode.Trim() }
                Write-Ok "Promo applied: $($pr.message)"
                if ($pr.sub_expires_at) {
                    Write-Info "Active until: $($pr.sub_expires_at)"
                }
            } catch {
                Write-Warn "Could not apply promo: $_"
            }
        }
        "2" {
            try {
                $plans = Invoke-RestMethod `
                    -Uri "http://$($Conf.ServerAddr):$($Conf.ApiPort)/subscription/plans"
                Write-Host ""
                Write-Host "  Available plans:" -ForegroundColor Cyan
                foreach ($p in $plans) {
                    Write-Host ("    {0,-10}  {1,-36}  {2} RUB / {3} days" -f `
                        $p.id, $p.name, $p.price_rub, $p.duration_days)
                }
                Write-Host ""
            } catch {
                Write-Warn "Could not fetch plans."
            }

            $planId = Read-Host "  Plan ID to buy [standard]"
            if (-not $planId.Trim()) { $planId = "standard" }

            try {
                $null = Invoke-ApiAuth -Method Post -Path "/subscription/buy" `
                    -Token $Token -Body @{ plan_id = $planId.Trim() }
                Write-Ok "Subscription activated!"
            } catch {
                Write-Warn "Purchase failed (balance too low?): $_"
            }
        }
        default {
            Write-Warn "Skipping subscription setup."
        }
    }
}

# Refresh
try {
    $subResp   = Invoke-ApiAuth -Path "/subscription/status" -Token $Token
    $subStatus = $subResp.sub_status
} catch { }

$speedLabel = "unknown"
if ($subResp) {
    if ($subResp.sub_speed_mbps -eq 0) {
        $speedLabel = "unlimited"
    } else {
        $speedLabel = "$($subResp.sub_speed_mbps) Mbit/s"
    }
}

$expiryLine = ""
if ($subResp -and $subResp.sub_expires_at) {
    $expiryLine = "Expires      : $($subResp.sub_expires_at)"
}

Write-Host ""
Write-Banner @(
    "Account Summary",
    "",
    "Server       : $($Conf.ServerAddr):$($Conf.ApiPort)",
    "Subscription : $subStatus",
    $(if ($expiryLine) { $expiryLine } else { "Expires      : -" }),
    "Speed limit  : $speedLabel"
)
Write-Host ""

if ($Status) {
    Write-Ok "Status check complete."
    exit 0
}

# ===========================================================================
# 6. CONNECT (SOCKS5)
# ===========================================================================
Write-Section "Connecting (SOCKS5)"

if ($subStatus -ne "active") {
    Write-Warn "Subscription is not active. Server will likely reject the connection."
    $force = Read-Host "  Continue anyway? [y/N]"
    if ($force -notmatch '^[Yy]') {
        exit 0
    }
}

$listenPort = [int]$Conf.SocksPort
$inp = Read-Host "  Local SOCKS5 port [$listenPort]"
if ($inp -match '^\d+$') {
    $listenPort     = [int]$inp
    $Conf.SocksPort = "$listenPort"
    Save-Conf
}

Write-Host ""
Write-Banner @(
    "Lowkey VPN -- SOCKS5 Mode",
    "",
    "Server      : $($Conf.ServerAddr)",
    "VPN proxy   : $($Conf.ServerAddr):$($Conf.ProxyPort)",
    "Local SOCKS5: 127.0.0.1:$listenPort",
    "",
    "Windows Settings -> Network -> Proxy -> Manual",
    "SOCKS Host: 127.0.0.1   Port: $listenPort",
    "",
    "Press Ctrl-C to disconnect."
)
Write-Host ""

Enable-SystemProxy -Port $listenPort

try {
    & $Binary connect `
        --server     $Conf.ServerAddr `
        --api-port   $Conf.ApiPort `
        --proxy-port $Conf.ProxyPort `
        --mode       socks5 `
        --socks-port $listenPort
} finally {
    Disable-SystemProxy
}
