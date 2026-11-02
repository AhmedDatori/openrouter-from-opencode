# Called by install-windows.ps1. Creates the private .env and data\admin-login.txt with a
# random admin password.
param(
    [Parameter(Mandatory=$true)][string]$Origin,
    [string]$Bind = '127.0.0.1:8080'
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $root
[Environment]::CurrentDirectory = $root
if (Test-Path -LiteralPath '.env') { throw '.env already exists. Setup never overwrites credentials.' }
if ($Origin -notmatch '^https?://[^/]+$') { throw 'Origin must be an HTTP(S) origin without a path or trailing slash.' }
if ($Bind -notmatch '^127\.0\.0\.1:\d+$') { throw 'This setup helper binds to loopback. Use an SSH tunnel or HTTPS reverse proxy.' }

# Folder permissions are applied by secure-windows.ps1 before this runs.
$bytes = New-Object byte[] 32
$rng = [Security.Cryptography.RandomNumberGenerator]::Create()
try { $rng.GetBytes($bytes) } finally { $rng.Dispose() }
$password = [Convert]::ToBase64String($bytes).TrimEnd('=').Replace('+','-').Replace('/','_')
$text = [IO.File]::ReadAllText((Join-Path $root '.env.example'))
$text = $text.Replace('ADMIN_PASSWORD=replace-with-a-long-random-password', 'ADMIN_PASSWORD=' + $password)
$text = $text.Replace('BIND=127.0.0.1:8080', 'BIND=' + $Bind)
$text = $text.Replace('PUBLIC_URL=http://localhost:8080', 'PUBLIC_URL=' + $Origin)
$text = $text.Replace('COOKIE_SECURE=false', 'COOKIE_SECURE=' + $Origin.StartsWith('https://').ToString().ToLowerInvariant())
$utf8 = New-Object Text.UTF8Encoding($false)
[IO.File]::WriteAllText((Join-Path $root '.env'), $text, $utf8)
New-Item -ItemType Directory -Path (Join-Path $root 'data') -Force | Out-Null
$login = "Go Router admin`r`nURL: $Origin/admin`r`nPassword: $password`r`n`r`nAPI base URL: $Origin/api/v1`r`nCreate application keys in the dashboard. Do not use the Go upstream key in applications.`r`n"
[IO.File]::WriteAllText((Join-Path $root 'data/admin-login.txt'), $login, $utf8)
Write-Host 'Created private .env and data\admin-login.txt. No credentials printed.'
& (Join-Path $root 'go-router.exe') --check
if ($LASTEXITCODE -ne 0) { throw 'Gateway configuration check failed.' }

Write-Host 'Add your OpenCode Go API key after signing in: Dashboard > Setup & settings.'
