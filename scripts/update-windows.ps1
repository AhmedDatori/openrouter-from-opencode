#Requires -RunAsAdministrator
# Download a gateway release and replace go-router.exe. Keeps .env, the database,
# the admin password and Caddy. Usage:
#   powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\update-windows.ps1            # latest release
#   powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\update-windows.ps1 -Version v0.4.0
param([string]$Version = 'latest')
$ErrorActionPreference = 'Stop'
if ($Version -ne 'latest' -and $Version -notmatch '^v[0-9]+\.[0-9]+\.[0-9]+$') { throw 'Version must be "latest" or look like v0.4.0.' }
$root = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $root
[Environment]::CurrentDirectory = $root
$directory = Join-Path $env:TEMP ('go-router-update-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $directory -Force | Out-Null
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    $repository = 'https://github.com/AhmedDatori/openrouter-from-opencode/releases'
    $release = if ($Version -eq 'latest') { "$repository/latest/download" } else { "$repository/download/$Version" }
    $zip = Join-Path $directory 'go-router-windows-x86_64.zip'
    Invoke-WebRequest -UseBasicParsing "$release/go-router-windows-x86_64.zip" -OutFile $zip
    Invoke-WebRequest -UseBasicParsing "$release/go-router-windows-x86_64.zip.sha256" -OutFile "$zip.sha256"
    $expected = (Get-Content "$zip.sha256" -Raw).Trim().Split(' ')[0]
    if ((Get-FileHash $zip -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expected) { throw 'Download checksum mismatch.' }
    Expand-Archive -LiteralPath $zip -DestinationPath (Join-Path $directory 'unpacked')
    $newBinary = Join-Path $directory 'unpacked/go-router/go-router.exe'
    $reported = & $newBinary --version
    $wanted = if ($Version -eq 'latest') { '^go-router [0-9]+\.[0-9]+\.[0-9]+$' } else { '^go-router ' + [regex]::Escape($Version.Substring(1)) + '$' }
    if ($LASTEXITCODE -ne 0 -or $reported -notmatch $wanted) { throw 'Downloaded executable version check failed.' }

    $task = Get-ScheduledTask -TaskName GoRouter -ErrorAction SilentlyContinue
    $restart = $task -and $task.State -eq 'Running'
    if ($task) { Stop-ScheduledTask -TaskName GoRouter }
    $destination = Join-Path $root 'go-router.exe'
    for ($i = 0; $i -lt 30; $i++) {
        $running = Get-CimInstance Win32_Process -Filter "Name='go-router.exe'" | Where-Object { $_.ExecutablePath -eq $destination }
        if (!$running) { break }
        Start-Sleep -Milliseconds 200
    }
    if ($running) { throw 'The gateway is still running. Stop it, then retry.' }
    New-Item -ItemType Directory -Path (Join-Path $root 'data') -Force | Out-Null
    if (Test-Path -LiteralPath $destination) {
        Copy-Item -LiteralPath $destination -Destination (Join-Path $root 'data/go-router-before-update.exe') -Force
    }
    Copy-Item -LiteralPath $newBinary -Destination $destination -Force
    if ($restart) { Start-ScheduledTask -TaskName GoRouter }
    Write-Host "Installed $reported. Configuration, database and admin password were kept."
} finally {
    Remove-Item -LiteralPath $directory -Recurse -Force -ErrorAction SilentlyContinue
}
