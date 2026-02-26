<#
.SYNOPSIS
    Lowkey VPN Client -- Launch Script (Windows / PowerShell)

.DESCRIPTION
    Minimal connect script. Loads saved config and session from
    %USERPROFILE%\.config\lowkey\ and starts vpn-client.exe in SOCKS5 mode.
    Run client-setup.ps1 first to register and save config.

.PARAMETER SocksPort
    Override the local SOCKS5 listen port (default: value from saved config).

.PARAMETER Background
    Run the client as a background PowerShell Job.

.PARAMETER Stop
    Stop a running background job and clear the system proxy.

.PARAMETER Status
    Print config / session / job info and exit.

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

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
function Write-Info ([string]$m) { Write-Host "[INFO]  $m" -ForegroundColor Cyan   }
function Write-Ok   ([string]$m) { Write-Host "[ OK ]  $m" -ForegroundColor Green  }
function Write-Warn ([string]$m) { Write-Host "[WARN]  $m" -ForegroundColor Yellow }
function Write-Err  ([string]$m) { Write-Host "[ERR ]  $m" -ForegroundColor Red    }

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
$ScriptDir    = Split-Path -Parent $MyInvocation.MyCommand.Path
$Binary       = Join-Path $ScriptDir "vpn-client\target\release\vpn-client.exe"
$ConfDir      = Join-Path $env:USERPROFILE ".config\lowkey"
$ConfFile     = Join-Path $ConfDir "client.conf"
$SessionFile  = Join-Path $ConfDir "session.json"
$JobFile      = Join-Path $ScriptDir "vpn-client.job"
$LogFile      = Join-Path $ScriptDir "vpn-client.log"

# ---------------------------------------------------------------------------
# Load config
# ---------------------------------------------------------------------------
$Conf = [ordered]@{
    ServerAddr = ""
    ApiPort    = "8080"
    UdpPort    = "51820"
    ProxyPort  = "8388"
    SocksPort  = "1080"
}

if (Test-Path $ConfFile) {
    foreach ($line in (Get-Content $ConfFile)) {
        if ($line -match '^\s*(\w+)\s*=\s*"?([^"#]*)"?\s*$') {
            $Conf[$Matches[1]] = $Matches[2].Trim()
        }
    }
} else {
    Write-Err "Config not found: $ConfFile"
    Write-Err "Run client-setup.ps1 first."
    exit 1
}

if ($SocksPort -gt 0) {
    $Conf.SocksPort = "$SocksPort"
}

$listenPort = [int]$Conf.SocksPort

# ---------------------------------------------------------------------------
# Validate prerequisites
# ---------------------------------------------------------------------------
if (-not (Test-Path $Binary)) {
    Write-Err "Binary not found: $Binary"
    Write-Err "Run: powershell -ExecutionPolicy Bypass -File .\client-setup.ps1 -Build"
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

# ---------------------------------------------------------------------------
# Registry helpers -- Windows system SOCKS5 proxy
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

# ---------------------------------------------------------------------------
# --Stop
# ---------------------------------------------------------------------------
if ($Stop) {
    if (Test-Path $JobFile) {
        $jobId = (Get-Content $JobFile -Raw).Trim()
        $job   = Get-Job -Id ([int]$jobId) -ErrorAction SilentlyContinue
        if ($job) {
            Stop-Job  -Id ([int]$jobId)
            Remove-Job -Id ([int]$jobId)
            Write-Ok "Background VPN job $jobId stopped."
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

# ---------------------------------------------------------------------------
# --Status
# ---------------------------------------------------------------------------
if ($Status) {
    $jobInfo = "none"
    if (Test-Path $JobFile) {
        $jobId  = (Get-Content $JobFile -Raw).Trim()
        $job    = Get-Job -Id ([int]$jobId) -ErrorAction SilentlyContinue
        $state  = if ($job) { $job.State } else { "not running" }
        $jobInfo = "Job $jobId ($state)"
    }
    $sessionOk = if (Test-Path $SessionFile) { "found" } else { "MISSING" }

    Write-Banner @(
        "Lowkey VPN Client -- Status",
        "",
        "Server      : $($Conf.ServerAddr):$($Conf.ApiPort)",
        "Proxy port  : $($Conf.ProxyPort)",
        "SOCKS5      : 127.0.0.1:$listenPort",
        "Binary      : $Binary",
        "Session     : $sessionOk",
        "Background  : $jobInfo"
    )
    exit 0
}

# ---------------------------------------------------------------------------
# Banner
# ---------------------------------------------------------------------------
if ($Background) {
    Write-Banner @(
        "Lowkey VPN Client (SOCKS5) -- Background",
        "",
        "Server      : $($Conf.ServerAddr)",
        "VPN proxy   : $($Conf.ServerAddr):$($Conf.ProxyPort)",
        "Local SOCKS5: 127.0.0.1:$listenPort",
        "",
        "Logs : Get-Content '$LogFile' -Wait",
        "Stop : .\client-run.ps1 -Stop"
    )
} else {
    Write-Banner @(
        "Lowkey VPN Client (SOCKS5)",
        "",
        "Server      : $($Conf.ServerAddr)",
        "VPN proxy   : $($Conf.ServerAddr):$($Conf.ProxyPort)",
        "Local SOCKS5: 127.0.0.1:$listenPort",
        "",
        "Set system proxy -> SOCKS5 127.0.0.1:$listenPort",
        "Press Ctrl-C to disconnect."
    )
}
Write-Host ""

# ---------------------------------------------------------------------------
# Build argument list
# ---------------------------------------------------------------------------
$BinaryArgs = @(
    "connect",
    "--server",     $Conf.ServerAddr,
    "--api-port",   $Conf.ApiPort,
    "--proxy-port", $Conf.ProxyPort,
    "--mode",       "socks5",
    "--socks-port", "$listenPort"
)

# ---------------------------------------------------------------------------
# Background mode
# ---------------------------------------------------------------------------
if ($Background) {
    # Stop any previous background job
    if (Test-Path $JobFile) {
        $oldId  = (Get-Content $JobFile -Raw).Trim()
        $oldJob = Get-Job -Id ([int]$oldId) -ErrorAction SilentlyContinue
        if ($oldJob) {
            Stop-Job  -Id ([int]$oldId)
            Remove-Job -Id ([int]$oldId)
        }
        Remove-Item $JobFile -Force
    }

    $binPath   = $Binary
    $binArgs   = $BinaryArgs
    $logPath   = $LogFile

    $job = Start-Job -ScriptBlock {
        & $using:binPath $using:binArgs 2>&1 | Tee-Object -FilePath $using:logPath -Append
    }

    "$($job.Id)" | Set-Content $JobFile
    Enable-SystemProxy -Port $listenPort
    Write-Ok "VPN started as background Job $($job.Id)."
    Write-Info "Logs : Get-Content '$LogFile' -Wait"
    Write-Info "Stop : .\client-run.ps1 -Stop"
    exit 0
}

# ---------------------------------------------------------------------------
# Foreground mode
# ---------------------------------------------------------------------------
Enable-SystemProxy -Port $listenPort

try {
    & $Binary $BinaryArgs
} finally {
    Disable-SystemProxy
}
