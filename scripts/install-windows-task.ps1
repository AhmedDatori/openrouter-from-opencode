#Requires -RunAsAdministrator
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if (!(Test-Path -LiteralPath (Join-Path $root '.env'))) { throw 'Run scripts\setup-windows.ps1 first.' }
if (Get-ScheduledTask -TaskName 'GoRouter' -ErrorAction SilentlyContinue) { throw 'GoRouter task already exists. Stop it and update the installation in place.' }
$runner = Join-Path $root 'scripts/run-windows.ps1'
if ($runner.Contains('"')) { throw 'Unsupported installation path.' }
$action = New-ScheduledTaskAction -Execute "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe" -Argument "-NoProfile -NonInteractive -ExecutionPolicy Bypass -File `"$runner`"" -WorkingDirectory $root
$trigger = New-ScheduledTaskTrigger -AtStartup
$principal = New-ScheduledTaskPrincipal -UserId 'SYSTEM' -LogonType ServiceAccount -RunLevel Highest
$settings = New-ScheduledTaskSettingsSet -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1) -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
Register-ScheduledTask -TaskName 'GoRouter' -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Description 'Go Router API gateway; private loopback listener; logs in data\server.log' | Out-Null
Start-ScheduledTask -TaskName 'GoRouter'
$ready = $false
for ($i = 0; $i -lt 30; $i++) {
    try { Invoke-RestMethod 'http://127.0.0.1:8080/readyz' -TimeoutSec 1 | Out-Null; $ready = $true; break }
    catch { Start-Sleep -Seconds 1 }
}
if (!$ready) { throw 'Task registered but port 8080 did not become ready. Inspect data\server.log and the task state.' }
Write-Host 'GoRouter is running and will start after reboot, including when you disconnect RDP.'
Write-Host 'Admin: http://127.0.0.1:8080/admin ; credentials: data\admin-login.txt'
