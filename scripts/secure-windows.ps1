#Requires -RunAsAdministrator
# Lock down an installation and run it with least privilege. Safe to run again at any time.
#   powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\secure-windows.ps1
#
# 1. Folder permissions: only Administrators and SYSTEM can change files; Administrators
#    own everything (files planted earlier by other users lose their access); the
#    LocalService account may read the program and write only to data\.
# 2. Runs the Caddy service and the gateway task as LocalService instead of SYSTEM, so a
#    compromise of either internet-facing process does not control the whole server.
# 3. Verifies the gateway and https://<PUBLIC_URL>/healthz, and switches back to SYSTEM
#    automatically if they do not come up.
# -AclOnly applies step 1 only (used by install-windows.ps1 before downloading anything).
param([switch]$AclOnly)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $root
[Environment]::CurrentDirectory = $root
$admins = '*S-1-5-32-544'
$system = '*S-1-5-18'
$localService = '*S-1-5-19'

function Invoke-Icacls {
    & icacls.exe @args | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "icacls $($args -join ' ') failed." }
}

# --- 1. Permissions --------------------------------------------------------------
Invoke-Icacls $root /inheritance:r /grant:r "$($admins):(OI)(CI)F" "$($system):(OI)(CI)F" "$($localService):(OI)(CI)RX"
Invoke-Icacls $root /setowner $admins /T /C /Q
$children = Get-ChildItem -LiteralPath $root -Force
if ($children) { Invoke-Icacls (Join-Path $root '*') /reset /T /C /Q }
$data = Join-Path $root 'data'
New-Item -ItemType Directory -Path $data -Force | Out-Null
Invoke-Icacls $data /grant "$($localService):(OI)(CI)M"
$login = Join-Path $data 'admin-login.txt'
if (Test-Path -LiteralPath $login) {
    # The plain-text password is for administrators only, not for the service account.
    Invoke-Icacls $login /inheritance:r /grant:r "$($admins):F" "$($system):F"
}
Write-Host 'Permissions: Administrators and SYSTEM full control; LocalService read-only except data\.'
if ($AclOnly) { return }

# --- 2. Least-privilege accounts ---------------------------------------------------
$task = Get-ScheduledTask -TaskName GoRouter -ErrorAction SilentlyContinue
$service = Get-Service -Name GoRouterCaddy -ErrorAction SilentlyContinue
if (!$task -or !$service) { throw 'GoRouter task or GoRouterCaddy service not found. Run install-windows.ps1 first.' }
$envText = [IO.File]::ReadAllText((Join-Path $root '.env'))
if ($envText -notmatch '(?m)^PUBLIC_URL=(https://[^\s/]+)\s*$') { throw 'PUBLIC_URL in .env must be the https:// address of this server.' }
$publicUrl = $Matches[1]

function Set-Accounts([bool]$leastPrivilege) {
    Stop-ScheduledTask -TaskName GoRouter
    Stop-Service GoRouterCaddy
    if ($leastPrivilege) {
        & sc.exe config GoRouterCaddy obj= 'NT AUTHORITY\LocalService' password= '' | Out-Null
        $principal = New-ScheduledTaskPrincipal -UserId 'NT AUTHORITY\LOCAL SERVICE' -LogonType ServiceAccount -RunLevel Limited
    } else {
        & sc.exe config GoRouterCaddy obj= LocalSystem | Out-Null
        $principal = New-ScheduledTaskPrincipal -UserId 'SYSTEM' -LogonType ServiceAccount -RunLevel Highest
    }
    if ($LASTEXITCODE -ne 0) { throw 'Could not change the Caddy service account.' }
    Set-ScheduledTask -TaskName GoRouter -Principal $principal | Out-Null
    Start-Sleep -Seconds 1
    Start-ScheduledTask -TaskName GoRouter
    Start-Service GoRouterCaddy
}

function Test-Healthy {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    for ($i = 0; $i -lt 30; $i++) {
        Start-Sleep -Seconds 2
        try {
            Invoke-RestMethod 'http://127.0.0.1:8080/readyz' -TimeoutSec 2 | Out-Null
            if ((Invoke-WebRequest -UseBasicParsing -TimeoutSec 10 "$publicUrl/healthz").StatusCode -eq 200) { return $true }
        } catch { }
    }
    return $false
}

Write-Host 'Switching Caddy and the gateway to the LocalService account...'
Set-Accounts $true
if (Test-Healthy) {
    Write-Host "Secured. Caddy and the gateway run as LocalService. $publicUrl is healthy."
} else {
    Write-Warning 'The gateway did not become healthy as LocalService. Switching back to SYSTEM.'
    Get-Content (Join-Path $data 'server.log') -Tail 15 -ErrorAction SilentlyContinue
    Get-Content (Join-Path $data 'caddy\process.log') -Tail 15 -ErrorAction SilentlyContinue
    Set-Accounts $false
    if (Test-Healthy) { Write-Host 'Restored: running as SYSTEM again. Please report the log lines above.' }
    else { throw 'The gateway is not healthy even as SYSTEM. Check data\server.log and data\caddy\process.log.' }
}
