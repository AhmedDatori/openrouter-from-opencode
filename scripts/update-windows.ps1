#Requires -RunAsAdministrator
# Update only the executable; preserve .env, the database, login file, and Caddy.
param([string]$Version = 'v0.2.0')
$ErrorActionPreference = 'Stop'
if ($Version -notmatch '^v[0-9]+\.[0-9]+\.[0-9]+$') { throw 'Version must look like v0.2.0.' }
$root = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $root
[Environment]::CurrentDirectory = $root
$directory = Join-Path $env:TEMP ('go-router-update-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $directory -Force | Out-Null
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    $release = "https://github.com/AhmedDatori/openrouter-from-opencode/releases/download/$Version"
    $zip = Join-Path $directory 'go-router-windows-x86_64.zip'
    Invoke-WebRequest -UseBasicParsing "$release/go-router-windows-x86_64.zip" -OutFile $zip
    Invoke-WebRequest -UseBasicParsing "$release/go-router-windows-x86_64.zip.sha256" -OutFile "$zip.sha256"
    $expected = (Get-Content "$zip.sha256" -Raw).Trim().Split(' ')[0]
    if ((Get-FileHash $zip -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expected) { throw 'Update checksum mismatch.' }
    Expand-Archive -LiteralPath $zip -DestinationPath (Join-Path $directory 'unpacked')
    $newBinary = Join-Path $directory 'unpacked/go-router/go-router.exe'
    $reported = & $newBinary --version
    if ($LASTEXITCODE -ne 0 -or $reported -ne ('go-router ' + $Version.Substring(1))) { throw 'Downloaded executable version check failed.' }
    $task = Get-ScheduledTask -TaskName GoRouter -ErrorAction SilentlyContinue
    $restart = $task -and $task.State -eq 'Running'
    if ($task) { Stop-ScheduledTask -TaskName GoRouter }
    $destination = Join-Path $root 'go-router.exe'
    for ($i = 0; $i -lt 30; $i++) {
        $running = Get-CimInstance Win32_Process -Filter "Name='go-router.exe'" | Where-Object { $_.ExecutablePath -eq $destination }
        if (!$running) { break }
        Start-Sleep -Milliseconds 200
    }
    if ($running) { throw 'Gateway is still running. Stop its foreground window or startup task, then retry.' }
    New-Item -ItemType Directory -Path (Join-Path $root 'data') -Force | Out-Null
    if (Test-Path -LiteralPath $destination) {
        Copy-Item -LiteralPath $destination -Destination (Join-Path $root 'data/go-router-before-update.exe') -Force
    }
    Copy-Item -LiteralPath $newBinary -Destination $destination -Force
    if ($restart) { Start-ScheduledTask -TaskName GoRouter }
    Write-Host "Installed $reported. Configuration, database, and dashboard password preserved."
    Write-Host 'Run publish-windows.ps1 to finish an incomplete first installation.'
} finally {
    Remove-Item -LiteralPath $directory -Recurse -Force -ErrorAction SilentlyContinue
}
