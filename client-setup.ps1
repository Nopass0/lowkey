<#
.SYNOPSIS
    Lowkey VPN Client — Full Setup & Connect Script (Windows / PowerShell)

.DESCRIPTION
    First-run script for the Lowkey VPN client on Windows.
    1. Checks and installs Rust toolchain (via rustup-init.exe)
    2. Builds vpn-client.exe in release mode
    3. Prompts for server address, username, password
    4. Registers a new account OR logs into an existing one
    5. Applies a promo / trial code for a free subscription (TRIAL30 by default)
    6. Shows subscription status
    7. Connects in SOCKS5 mode (set system proxy to SOCKS5 127.0.0.1:1080)

    On Windows, TUN mode is not available — only SOCKS5 proxy is supported.
    After connecting, configure your browser / system proxy:
        Protocol : SOCKS5
        Host     : 127.0.0.1
        Port     : 1080

.NOTES
    Run from the repository root with:
        powershell -ExecutionPolicy Bypass -File .\client-setup.ps1

    Flags:
        -Connect    Reconnect using saved credentials (skip setup prompts)
        -Build      Rebuild the binary only
        -Status     Show account and subscription info, then exit
        -SocksPort  Override the local SOCKS5 listen port (default 1080)

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File .\client-setup.ps1
    powershell -ExecutionPolicy Bypass -File .\client-setup.ps1 -Connect
    powershell -ExecutionPolicy Bypass -File .\client-setup.ps1 -Status
#>

[CmdletBinding()]
param(
    [switch]$Connect,
    [switch]$Build,
    [switch]$Status,
    [int]$SocksPort = 0   # 0 = read from saved config or default 1080
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# ── Helpers ───────────────────────────────────────────────────────────────────
function Write-Info  ([string]$msg) { Write-Host "[INFO]  $msg" -ForegroundColor Cyan    }
function Write-Ok    ([string]$msg) { Write-Host "[ OK ]  $msg" -ForegroundColor Green   }
function Write-Warn  ([string]$msg) { Write-Host "[WARN]  $msg" -ForegroundColor Yellow  }
function Write-Err   ([string]$msg) { Write-Host "[ERR ]  $msg" -ForegroundColor Red     }
function Write-Sec   ([string]$msg) {
    Write-Host ""
    Write-Host "══════════════════════════════════════════" -ForegroundColor Cyan
    Write-Host "  $msg" -ForegroundColor Cyan
    Write-Host "══════════════════════════════════════════" -ForegroundColor Cyan
}

# ── Paths ─────────────────────────────────────────────────────────────────────
$ScriptDir  = Split-Path -Parent $MyInvocation.MyCommand.Path
$ClientDir  = Join-Path $ScriptDir "vpn-client"
$Binary     = Join-Path $ClientDir "target\release\vpn-client.exe"
$ConfDir    = Join-Path $env:USERPROFILE ".config\lowkey"
$ConfFile   = Join-Path $ConfDir "client.conf"
$SessionFile= Join-Path $ConfDir "session.json"

# ── Load / save config ────────────────────────────────────────────────────────
function Load-Conf {
    $script:Conf = @{
        ServerAddr  = ""
        ApiPort     = 8080
        UdpPort     = 51820
        ProxyPort   = 8388
        SocksPort   = 1080
    }
    if (Test-Path $ConfFile) {
        Get-Content $ConfFile | ForEach-Object {
            if ($_ -match '^\s*(\w+)\s*=\s*"?([^"#]*)"?\s*$') {
                $script:Conf[$Matches[1]] = $Matches[2].Trim()
            }
        }
    }
}

function Save-Conf {
    New-Item -ItemType Directory -Force -Path $ConfDir | Out-Null
    @"
# Lowkey VPN Client — saved configuration
ServerAddr=$($script:Conf.ServerAddr)
ApiPort=$($script:Conf.ApiPort)
UdpPort=$($script:Conf.UdpPort)
ProxyPort=$($script:Conf.ProxyPort)
SocksPort=$($script:Conf.SocksPort)
"@ | Set-Content -Encoding UTF8 $ConfFile
}

Load-Conf

# Override SOCKS port if passed as parameter
if ($SocksPort -gt 0) { $script:Conf.SocksPort = $SocksPort }

# ── API helpers ───────────────────────────────────────────────────────────────

function Invoke-ApiAnon {
    param([string]$Path, [hashtable]$Body)
    $url = "http://$($script:Conf.ServerAddr):$($script:Conf.ApiPort)$Path"
    try {
        return Invoke-RestMethod -Uri $url -Method Post `
            -ContentType 'application/json' `
            -Body ($Body | ConvertTo-Json -Compress)
    } catch {
        $status = $_.Exception.Response.StatusCode.value__
        $detail = $_.ErrorDetails.Message
        throw "HTTP $status — $detail"
    }
}

function Invoke-ApiAuth {
    param([string]$Method = 'GET', [string]$Path, [string]$Token, [hashtable]$Body = @{})
    $url = "http://$($script:Conf.ServerAddr):$($script:Conf.ApiPort)$Path"
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
        $status = $_.Exception.Response.StatusCode.value__
        $detail = $_.ErrorDetails.Message
        throw "HTTP $status — $detail"
    }
}

# ── Load existing session token ───────────────────────────────────────────────
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
    @{ token = $Token; server = $script:Conf.ServerAddr; api_port = $script:Conf.ApiPort } |
        ConvertTo-Json | Set-Content -Encoding UTF8 $SessionFile
}

# =============================================================================
# 1. RUST TOOLCHAIN
# =============================================================================
Write-Sec "Rust toolchain"

$CargoExe = $null
$CargoPaths = @(
    "$env:USERPROFILE\.cargo\bin\cargo.exe",
    (Get-Command cargo -ErrorAction SilentlyContinue)?.Source
) | Where-Object { $_ -and (Test-Path $_) }

if ($CargoPaths) {
    $CargoExe = $CargoPaths[0]
    Write-Ok "Rust found at $CargoExe"
} else {
    Write-Info "Rust not found — downloading rustup-init.exe..."
    $RustupInstaller = Join-Path $env:TEMP "rustup-init.exe"
    Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile $RustupInstaller -UseBasicParsing
    Write-Info "Running rustup installer (this takes a few minutes)..."
    Start-Process -FilePath $RustupInstaller -ArgumentList "-y", "--no-modify-path" -Wait -NoNewWindow
    $CargoExe = "$env:USERPROFILE\.cargo\bin\cargo.exe"
    if (-not (Test-Path $CargoExe)) {
        Write-Err "Rust installation failed. Install manually from https://rustup.rs and re-run."
        exit 1
    }
    Write-Ok "Rust installed."
}

$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

# =============================================================================
# 2. BUILD
# =============================================================================
Write-Sec "Building vpn-client (release)"
Write-Info "Running: cargo build --release"
Write-Info "(first build may take a few minutes)"

Push-Location $ClientDir
try {
    & $CargoExe build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed (exit $LASTEXITCODE)" }
} finally {
    Pop-Location
}

Write-Ok "Build complete: $Binary"

if ($Build) {
    Write-Ok "Build-only mode — done."
    exit 0
}

# =============================================================================
# 3. SERVER CONFIGURATION
# =============================================================================
Write-Sec "Server configuration"

Write-Host ""
Write-Host "Enter the VPN server details. Press Enter to keep current values." -ForegroundColor Yellow
Write-Host ""

$curServer = $script:Conf.ServerAddr
if ($curServer) {
    $inp = Read-Host "  Server IP or hostname [$curServer]"
    if ($inp) { $script:Conf.ServerAddr = $inp.Trim() }
} else {
    do {
        $inp = Read-Host "  Server IP or hostname (required)"
    } while (-not $inp)
    $script:Conf.ServerAddr = $inp.Trim()
}

# Check server reachability
Write-Info "Checking connectivity to $($script:Conf.ServerAddr)..."
$StatusResp = $null
try {
    $StatusResp = Invoke-RestMethod `
        -Uri "http://$($script:Conf.ServerAddr):$($script:Conf.ApiPort)/api/status" `
        -TimeoutSec 5
} catch {
    Write-Warn "Could not reach port $($script:Conf.ApiPort). Try a different API port."
    $inp = Read-Host "  API port [$($script:Conf.ApiPort)]"
    if ($inp) { $script:Conf.ApiPort = [int]$inp }

    try {
        $StatusResp = Invoke-RestMethod `
            -Uri "http://$($script:Conf.ServerAddr):$($script:Conf.ApiPort)/api/status" `
            -TimeoutSec 5
    } catch {
        Write-Err "Still cannot reach the server. Check the IP, port and firewall."
        exit 1
    }
}

Write-Ok "Server is reachable."

# Use advertised ports from the server status response
if ($StatusResp.udp_port)   { $script:Conf.UdpPort   = $StatusResp.udp_port   }
if ($StatusResp.proxy_port) { $script:Conf.ProxyPort  = $StatusResp.proxy_port }

Write-Info "UDP port  : $($script:Conf.UdpPort)"
Write-Info "Proxy port: $($script:Conf.ProxyPort)"

Save-Conf

# =============================================================================
# 4. ACCOUNT — REGISTER OR LOGIN
# =============================================================================
Write-Sec "Account setup"

$Token     = $null
$LoggedIn  = $false
$SavedToken = Get-SavedToken

if ($SavedToken) {
    try {
        $MeResp = Invoke-ApiAuth -Path "/auth/me" -Token $SavedToken
        Write-Ok "Already logged in as '$($MeResp.login)'."
        $Token    = $SavedToken
        $LoggedIn = $true
    } catch {
        Write-Warn "Saved session expired — please log in again."
    }
}

if (-not $LoggedIn) {
    Write-Host ""
    $hasAccount = Read-Host "  Do you have an existing account on this server? [y/N]"
    Write-Host ""
    $loginName = Read-Host "  Login (username)"
    $loginPass = Read-Host "  Password" -AsSecureString
    $plainPass = [Runtime.InteropServices.Marshal]::PtrToStringAuto(
                    [Runtime.InteropServices.Marshal]::SecureStringToBSTR($loginPass))

    if ($hasAccount -match '^[Yy]') {
        Write-Info "Logging in as '$loginName'..."
        try {
            $AuthResp = Invoke-ApiAnon -Path "/auth/login" `
                -Body @{ login = $loginName; password = $plainPass }
        } catch {
            Write-Err "Login failed: $_"
            exit 1
        }
    } else {
        Write-Info "Creating account '$loginName'..."
        try {
            $AuthResp = Invoke-ApiAnon -Path "/auth/register" `
                -Body @{ login = $loginName; password = $plainPass }
        } catch {
            Write-Err "Registration failed: $_"
            exit 1
        }
    }

    $Token = $AuthResp.token
    if (-not $Token) {
        Write-Err "No token received from the server."
        exit 1
    }

    Save-Session -Token $Token
    Write-Ok "Session saved to $SessionFile"
}

# =============================================================================
# 5. SUBSCRIPTION CHECK & TRIAL ACTIVATION
# =============================================================================
Write-Sec "Subscription"

$SubResp   = $null
$SubStatus = "unknown"
try {
    $SubResp   = Invoke-ApiAuth -Path "/subscription/status" -Token $Token
    $SubStatus = $SubResp.sub_status
} catch { }

Write-Host ""
Write-Host "  Current subscription: " -NoNewline
Write-Host $SubStatus -ForegroundColor Yellow
if ($SubResp -and $SubResp.sub_expires_at) {
    Write-Host "  Expires: $($SubResp.sub_expires_at)"
}
Write-Host ""

if ($SubStatus -ne "active") {
    Write-Host "  No active subscription." -ForegroundColor Yellow
    Write-Host ""
    Write-Host "  Options:" -ForegroundColor Cyan
    Write-Host "    1) Apply a promo / trial code" -ForegroundColor Green
    Write-Host "    2) View plans and buy a subscription" -ForegroundColor Green
    Write-Host "    3) Skip (the server will reject the VPN connection)" -ForegroundColor Green
    Write-Host ""
    $choice = Read-Host "  Choice [1]"
    if (-not $choice) { $choice = "1" }

    switch ($choice) {
        "1" {
            Write-Host ""
            Write-Host "  If the server admin ran server-setup.sh, try: " -NoNewline
            Write-Host "TRIAL30" -ForegroundColor Green
            $promoCode = Read-Host "  Enter promo code [TRIAL30]"
            if (-not $promoCode) { $promoCode = "TRIAL30" }

            try {
                $PromoResp = Invoke-ApiAuth -Method Post -Path "/promo/apply" `
                    -Token $Token -Body @{ code = $promoCode }
                Write-Ok "Promo applied: $($PromoResp.message)"
                if ($PromoResp.sub_expires_at) {
                    Write-Info "Subscription active until: $($PromoResp.sub_expires_at)"
                }
            } catch {
                Write-Warn "Could not apply promo code: $_"
            }
        }
        "2" {
            try {
                $Plans = Invoke-RestMethod `
                    -Uri "http://$($script:Conf.ServerAddr):$($script:Conf.ApiPort)/subscription/plans"
                Write-Host ""
                Write-Host "  Available plans:" -ForegroundColor Cyan
                foreach ($p in $Plans) {
                    Write-Host ("  {0,-10} {1,-35} {2} RUB / {3} days" -f `
                        $p.id, $p.name, $p.price_rub, $p.duration_days)
                }
                Write-Host ""
            } catch { Write-Warn "Could not fetch plans." }

            $planId = Read-Host "  Plan ID to buy [standard]"
            if (-not $planId) { $planId = "standard" }
            try {
                $BuyResp = Invoke-ApiAuth -Method Post -Path "/subscription/buy" `
                    -Token $Token -Body @{ plan_id = $planId }
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

# Refresh subscription status
try {
    $SubResp   = Invoke-ApiAuth -Path "/subscription/status" -Token $Token
    $SubStatus = $SubResp.sub_status
} catch { }

$speedLabel = if ($SubResp -and $SubResp.sub_speed_mbps -eq 0) { "unlimited" } `
              elseif ($SubResp) { "$($SubResp.sub_speed_mbps) Mbit/s" } `
              else { "unknown" }

Write-Host ""
Write-Host "╔══════════════════════════════════════════════════════╗" -ForegroundColor Green
Write-Host "║               Account Summary                        ║" -ForegroundColor Green
Write-Host "╠══════════════════════════════════════════════════════╣" -ForegroundColor Green
Write-Host "║  Server       : $($script:Conf.ServerAddr):$($script:Conf.ApiPort)"
Write-Host "║  Subscription : $SubStatus"
if ($SubResp -and $SubResp.sub_expires_at) {
    Write-Host "║  Expires      : $($SubResp.sub_expires_at)"
}
Write-Host "║  Speed limit  : $speedLabel"
Write-Host "╚══════════════════════════════════════════════════════╝" -ForegroundColor Green
Write-Host ""

if ($Status) {
    Write-Ok "Status check complete."
    exit 0
}

# =============================================================================
# 6. CONNECT (SOCKS5)
# =============================================================================
Write-Sec "Connecting (SOCKS5)"

if ($SubStatus -ne "active") {
    Write-Warn "Subscription is not active. Server will likely reject the connection."
    $force = Read-Host "  Continue anyway? [y/N]"
    if ($force -notmatch '^[Yy]') { exit 0 }
}

$socksListenPort = $script:Conf.SocksPort
$inp = Read-Host "  Local SOCKS5 port [$socksListenPort]"
if ($inp -match '^\d+$') {
    $socksListenPort = [int]$inp
    $script:Conf.SocksPort = $socksListenPort
    Save-Conf
}

Write-Host ""
Write-Host "╔══════════════════════════════════════════════════════╗" -ForegroundColor Green
Write-Host "║         Lowkey VPN — SOCKS5 Mode                     ║" -ForegroundColor Green
Write-Host "╠══════════════════════════════════════════════════════╣" -ForegroundColor Green
Write-Host "║  Server      : $($script:Conf.ServerAddr)"
Write-Host "║  Proxy port  : $($script:Conf.ProxyPort)  →  SOCKS5 127.0.0.1:$socksListenPort"
Write-Host "╠══════════════════════════════════════════════════════╣" -ForegroundColor Green
Write-Host "║  Set system proxy:                                    ║" -ForegroundColor Green
Write-Host "║    Settings → Network → Proxy → Manual               ║" -ForegroundColor Green
Write-Host "║    SOCKS Host: 127.0.0.1   Port: $socksListenPort" -ForegroundColor Yellow
Write-Host "╠══════════════════════════════════════════════════════╣" -ForegroundColor Green
Write-Host "║  Press Ctrl-C to disconnect.                         ║" -ForegroundColor Green
Write-Host "╚══════════════════════════════════════════════════════╝" -ForegroundColor Green
Write-Host ""

# Set the Windows system SOCKS5 proxy automatically
$regPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings"
$proxyStr = "socks=127.0.0.1:$socksListenPort"
Set-ItemProperty -Path $regPath -Name ProxyServer -Value $proxyStr
Set-ItemProperty -Path $regPath -Name ProxyEnable  -Value 1
Write-Ok "System proxy set to SOCKS5 127.0.0.1:$socksListenPort"
Write-Info "(will be cleared when you close this window or Ctrl-C)"

# Restore proxy on exit
$restoreProxy = {
    $regPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings"
    Set-ItemProperty -Path $regPath -Name ProxyEnable -Value 0
    Write-Host "`n[INFO]  System proxy cleared." -ForegroundColor Cyan
}

try {
    & $Binary connect `
        --server       $script:Conf.ServerAddr `
        --api-port     $script:Conf.ApiPort `
        --proxy-port   $script:Conf.ProxyPort `
        --mode         socks5 `
        --socks-port   $socksListenPort
} finally {
    & $restoreProxy
}
