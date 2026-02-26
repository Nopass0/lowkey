<#
.SYNOPSIS
    Lowkey VPN Client — Launch Script (Windows / PowerShell)

.DESCRIPTION
    Minimal connect script for the Lowkey VPN client on Windows.
    Loads saved config and session from ~/.config/lowkey/,
    then starts vpn-client.exe in SOCKS5 mode.

    Run client-setup.ps1 first to register, get a subscription and save config.

.NOTES
    Run from the repository root with:
        powershell -ExecutionPolicy Bypass -File .\client-run.ps1

    Flags:
        -SocksPort <int>   Override local SOCKS5 port (default: saved config)
        -Background        Run in a background job (detached)
        -Stop              Stop a running background job
        -Status            Print current server + proxy info and exit

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File .\client-run.ps1
    powershell -ExecutionPolicy Bypass -File .\client-run.ps1 -SocksPort 1081
    powershell -ExecutionPolicy Bypass -File .\client-run.ps1 -Background
    powershell -ExecutionPolicy Bypass -File .\client-run.ps1 -Stop
#>

[CmdletBinding()]
param(
    [int]   $SocksPort  = 0,
    [switch]$Background,
    [switch]$Stop,
    [switch]$Status
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# ── Helpers ───────────────────────────────────────────────────────────────────
function Write-Info ([string]$m) { Write-Host "[INFO]  $m" -ForegroundColor Cyan   }
function Write-Ok   ([string]$m) { Write-Host "[ OK ]  $m" -ForegroundColor Green  }
function Write-Warn ([string]$m) { Write-Host "[WARN]  $m" -ForegroundColor Yellow }
function Write-Err  ([string]$m) { Write-Host "[ERR ]  $m" -ForegroundColor Red    }

# ── Paths ─────────────────────────────────────────────────────────────────────
$ScriptDir   = Split-Path -Parent $MyInvocation.MyCommand.Path
$Binary      = Join-Path $ScriptDir "vpn-client\target\release\vpn-client.exe"
$ConfDir     = Join-Path $env:USERPROFILE ".config\lowkey"
$ConfFile    = Join-Path $ConfDir "client.conf"
$SessionFile = Join-Path $ConfDir "session.json"
$JobFile     = Join-Path $ScriptDir "vpn-client.job"   # stores background job ID
$LogFile     = Join-Path $ScriptDir "vpn-client.log"

# ── Load config ───────────────────────────────────────────────────────────────
$Conf = @{
    ServerAddr = ""
    ApiPort    = 8080
    UdpPort    = 51820
    ProxyPort  = 8388
    SocksPort  = 1080
}

if (Test-Path $ConfFile) {
    Get-Content $ConfFile | ForEach-Object {
        if ($_ -match '^\s*(\w+)\s*=\s*"?([^"#]*)"?\s*$') {
            $Conf[$Matches[1]] = $Matches[2].Trim()
        }
    }
} else {
    Write-Err "Config not found at $ConfFile"
    Write-Err "Run client-setup.ps1 first."
    exit 1
}

if ($SocksPort -gt 0) { $Conf.SocksPort = $SocksPort }

# ── Validate prerequisites ────────────────────────────────────────────────────
if (-not (Test-Path $Binary)) {
    Write-Err "Binary not found: $Binary"
    Write-Err "Run client-setup.ps1 -Build first."
    exit 1
}

if (-not (Test-Path $SessionFile)) {
    Write-Err "Session not found: $SessionFile"
    Write-Err "Run client-setup.ps1 to register / log in."
    exit 1
}

if (-not $Conf.ServerAddr) {
    Write-Err "No server address in config. Run client-setup.ps1."
    exit 1
}

# ── Registry helpers for system proxy ────────────────────────────────────────
$RegProxy = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings"

function Enable-SystemProxy ([int]$Port) {
    Set-ItemProperty -Path $RegProxy -Name ProxyServer -Value "socks=127.0.0.1:$Port"
    Set-ItemProperty -Path $RegProxy -Name ProxyEnable  -Value 1
    Write-Ok "System SOCKS5 proxy → 127.0.0.1:$Port"
}

function Disable-SystemProxy {
    Set-ItemProperty -Path $RegProxy -Name ProxyEnable -Value 0
    Write-Info "System proxy cleared."
}

# ── Stop mode ─────────────────────────────────────────────────────────────────
if ($Stop) {
    if (Test-Path $JobFile) {
        $jobId = Get-Content $JobFile -Raw | ForEach-Object { $_.Trim() }
        $job   = Get-Job -Id $jobId -ErrorAction SilentlyContinue
        if ($job) {
            Stop-Job  -Id $jobId
            Remove-Job -Id $jobId
            Write-Ok "Background VPN job ($jobId) stopped."
        } else {
            Write-Warn "Job $jobId not found (may have already exited)."
        }
        Remove-Item $JobFile -Force
    } else {
        Write-Warn "No background job file found at $JobFile"
    }
    Disable-SystemProxy
    exit 0
}

# ── Status mode ───────────────────────────────────────────────────────────────
if ($Status) {
    Write-Host ""
    Write-Host "╔══════════════════════════════════════════════════╗" -ForegroundColor Green
    Write-Host "║          Lowkey VPN Client — Status              ║" -ForegroundColor Green
    Write-Host "╠══════════════════════════════════════════════════╣" -ForegroundColor Green
    Write-Host "║  Server     : $($Conf.ServerAddr):$($Conf.ApiPort)"
    Write-Host "║  Proxy port : $($Conf.ProxyPort)"
    Write-Host "║  SOCKS5     : 127.0.0.1:$($Conf.SocksPort)"
    Write-Host "║  Binary     : $Binary"
    Write-Host "║  Session    : $(if (Test-Path $SessionFile) { 'found' } else { 'MISSING' })"

    if (Test-Path $JobFile) {
        $jobId = (Get-Content $JobFile -Raw).Trim()
        $job   = Get-Job -Id $jobId -ErrorAction SilentlyContinue
        $state = if ($job) { $job.State } else { "not running" }
        Write-Host "║  Background job: $jobId ($state)"
    } else {
        Write-Host "║  Background job: none"
    }
    Write-Host "╚══════════════════════════════════════════════════╝" -ForegroundColor Green
    Write-Host ""
    exit 0
}

# ── Print banner ──────────────────────────────────────────────────────────────
Write-Host ""
Write-Host "╔══════════════════════════════════════════════════════╗" -ForegroundColor Green
Write-Host "║           Lowkey VPN Client (SOCKS5)                 ║" -ForegroundColor Green
Write-Host "╠══════════════════════════════════════════════════════╣" -ForegroundColor Green
Write-Host "║  Server       : $($Conf.ServerAddr)"
Write-Host "║  VPN proxy    : $($Conf.ServerAddr):$($Conf.ProxyPort)"
Write-Host "║  Local SOCKS5 : 127.0.0.1:$($Conf.SocksPort)"
Write-Host "╠══════════════════════════════════════════════════════╣" -ForegroundColor Green

if ($Background) {
Write-Host "║  Mode: background (logs → vpn-client.log)            ║" -ForegroundColor Green
Write-Host "║  Stop: .\client-run.ps1 -Stop                        ║" -ForegroundColor Green
} else {
Write-Host "║  Set system proxy → SOCKS5 127.0.0.1:$($Conf.SocksPort)         " -ForegroundColor Yellow
Write-Host "║  Press Ctrl-C to disconnect.                         ║" -ForegroundColor Green
}

Write-Host "╚══════════════════════════════════════════════════════╝" -ForegroundColor Green
Write-Host ""

# Build argument list for the binary
$Args = @(
    "connect",
    "--server",     $Conf.ServerAddr,
    "--api-port",   $Conf.ApiPort,
    "--proxy-port", $Conf.ProxyPort,
    "--mode",       "socks5",
    "--socks-port", $Conf.SocksPort
)

# ── Background mode ───────────────────────────────────────────────────────────
if ($Background) {
    # Stop any previous background job
    if (Test-Path $JobFile) {
        $oldId = (Get-Content $JobFile -Raw).Trim()
        $oldJob = Get-Job -Id $oldId -ErrorAction SilentlyContinue
        if ($oldJob) { Stop-Job $oldId; Remove-Job $oldId }
        Remove-Item $JobFile -Force
    }

    $binaryLocal = $Binary
    $argsLocal   = $Args
    $logLocal    = $LogFile

    $job = Start-Job -ScriptBlock {
        & $using:binaryLocal $using:argsLocal *>> $using:logLocal
    }

    $job.Id | Set-Content $JobFile
    Enable-SystemProxy -Port $Conf.SocksPort
    Write-Ok "VPN started in background (Job ID $($job.Id))."
    Write-Info "Logs : Get-Content '$LogFile' -Wait"
    Write-Info "Stop : .\client-run.ps1 -Stop"
    exit 0
}

# ── Foreground mode ───────────────────────────────────────────────────────────
Enable-SystemProxy -Port $Conf.SocksPort

try {
    & $Binary $Args
} finally {
    Disable-SystemProxy
}
