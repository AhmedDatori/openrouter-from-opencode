#Requires -RunAsAdministrator
# Move an installed public gateway (GoRouterCaddy) to a DNS hostname. Caddy then
# presents a certificate only for that hostname, so the old address stops working.
param(
    [Parameter(Mandatory=$true)][string]$Domain,
    [switch]$NewPassword
)
$ErrorActionPreference = 'Stop'
$Domain = $Domain.Trim().ToLowerInvariant()
if ($Domain.Length -gt 253 -or $Domain -notmatch '^(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z](?:[a-z0-9-]{0,61}[a-z0-9])?$') {
    throw 'Supply a DNS hostname without https://, port, or path, for example api.example.com.'
}
$root = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $root
[Environment]::CurrentDirectory = $root
$caddy = Join-Path $root 'caddy/caddy.exe'
$config = Join-Path $root 'caddy/Caddyfile'
$envPath = Join-Path $root '.env'
$loginPath = Join-Path $root 'data/admin-login.txt'
if (!(Get-Service -Name GoRouterCaddy -ErrorAction SilentlyContinue)) { throw 'GoRouterCaddy is not installed. Use publish-windows.ps1 for a first installation.' }
foreach ($path in @($caddy, $config, $envPath)) {
    if (!(Test-Path -LiteralPath $path)) { throw "Missing $path." }
}

$addresses = @(Resolve-DnsName -Name $Domain -Type A -DnsOnly -ErrorAction SilentlyContinue | Where-Object { $_.Type -eq 'A' } | ForEach-Object { $_.IPAddress })
if ($addresses.Count -eq 0) { throw "$Domain has no DNS A record yet. Point it at this server's public IPv4 address and retry." }
Write-Host "$Domain resolves to $($addresses -join ', '). It must be this server's public IPv4 address."

$utf8 = New-Object Text.UTF8Encoding($false)
$envText = [IO.File]::ReadAllText($envPath)
$names = @('BIND','PUBLIC_URL','COOKIE_SECURE')
if ($NewPassword) { $names += 'ADMIN_PASSWORD' }
foreach ($name in $names) {
    if ([regex]::Matches($envText, "(?m)^$name=").Count -ne 1) { throw "Expected exactly one $name entry in .env." }
}
if ($envText -match '(?m)^BIND=(127\.0\.0\.1:\d+)\s*$') { $upstream = $Matches[1] } else { throw 'BIND in .env must be a 127.0.0.1 address behind Caddy.' }

# Keep the challenge mode chosen at installation: HTTPS-only installs leave port 80
# to another web server and validate certificates over TLS-ALPN on port 443.
$httpsOnly = [IO.File]::ReadAllText($config) -match 'disable_http_challenge'
$storage = (Join-Path $root 'data/caddy').Replace('\','/')
$httpsConfig = ''
$tlsConfig = ''
if ($httpsOnly) {
    $httpsConfig = 'auto_https disable_redirects'
    $tlsConfig = @"
    tls {
        issuer acme {
            dir https://acme-v02.api.letsencrypt.org/directory
            disable_http_challenge
        }
    }
"@
}
$caddyfile = @"
{
    admin off
    $httpsConfig
    storage file_system {
        root "$storage/storage"
    }
    log default {
        output file "$storage/process.log" {
            roll_size 10MiB
            roll_keep 5
        }
    }
}
$Domain {
$tlsConfig
    reverse_proxy $upstream {
        flush_interval -1
    }
}
"@
$candidate = Join-Path $root 'data/Caddyfile.new'
[IO.File]::WriteAllText($candidate, $caddyfile, $utf8)
& $caddy validate --config $candidate --adapter caddyfile
if ($LASTEXITCODE -ne 0) { throw 'The new Caddy configuration is invalid; nothing was changed.' }

Copy-Item -LiteralPath $config -Destination (Join-Path $root 'data/Caddyfile-before-domain.txt') -Force
Copy-Item -LiteralPath $envPath -Destination (Join-Path $root 'data/env-before-domain-backup.txt') -Force
Move-Item -LiteralPath $candidate -Destination $config -Force

$envText = [regex]::Replace($envText, '(?m)^PUBLIC_URL=[^\r\n]*', "PUBLIC_URL=https://$Domain")
$envText = [regex]::Replace($envText, '(?m)^COOKIE_SECURE=[^\r\n]*', 'COOKIE_SECURE=true')
$password = $null
if ($NewPassword) {
    $bytes = New-Object byte[] 32
    $rng = [Security.Cryptography.RandomNumberGenerator]::Create()
    try { $rng.GetBytes($bytes) } finally { $rng.Dispose() }
    $password = [Convert]::ToBase64String($bytes).TrimEnd('=').Replace('+','-').Replace('/','_')
    $envText = [regex]::Replace($envText, '(?m)^ADMIN_PASSWORD=[^\r\n]*', "ADMIN_PASSWORD=$password")
}
[IO.File]::WriteAllText($envPath, $envText, $utf8)

if (Test-Path -LiteralPath $loginPath) {
    $login = [IO.File]::ReadAllText($loginPath)
    $login = [regex]::Replace($login, '(?m)^URL:[^\r\n]*', "URL: https://$Domain/admin")
    $login = [regex]::Replace($login, '(?m)^API base URL:[^\r\n]*', "API base URL: https://$Domain/api/v1")
    if ($password) { $login = [regex]::Replace($login, '(?m)^Password:[^\r\n]*', "Password: $password") }
} else {
    $shown = if ($password) { $password } else { '(unchanged; see ADMIN_PASSWORD in .env)' }
    $login = "Go Router admin`r`nURL: https://$Domain/admin`r`nPassword: $shown`r`n`r`nAPI base URL: https://$Domain/api/v1`r`n"
}
[IO.File]::WriteAllText($loginPath, $login, $utf8)
Write-Host 'Updated caddy\Caddyfile, .env and data\admin-login.txt. Backups are in data\. No credentials printed.'

# The gateway reads PUBLIC_URL and ADMIN_PASSWORD at startup.
$task = Get-ScheduledTask -TaskName GoRouter -ErrorAction SilentlyContinue
if ($task) {
    Stop-ScheduledTask -TaskName GoRouter
    $binary = Join-Path $root 'go-router.exe'
    for ($i = 0; $i -lt 50; $i++) {
        $running = Get-CimInstance Win32_Process -Filter "Name='go-router.exe'" | Where-Object { $_.ExecutablePath -eq $binary }
        if (!$running) { break }
        Start-Sleep -Milliseconds 200
    }
    Start-ScheduledTask -TaskName GoRouter
} else {
    Write-Warning 'No GoRouter startup task found. Restart the gateway yourself so the new settings take effect.'
}
Restart-Service GoRouterCaddy

[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$ok = $false
for ($i = 0; $i -lt 24 -and !$ok; $i++) {
    Start-Sleep -Seconds 5
    try {
        $response = Invoke-WebRequest -UseBasicParsing -TimeoutSec 10 "https://$Domain/healthz"
        $ok = $response.StatusCode -eq 200
    } catch { }
}
if ($ok) {
    Write-Host "Done. Admin: https://$Domain/admin ; API base URL: https://$Domain/api/v1"
    Write-Host 'Clients that still use the old address must switch to the new URL.'
    if ($password) { Write-Host 'The admin password changed. Read it with: notepad .\data\admin-login.txt' }
} else {
    Write-Warning "https://$Domain/healthz did not answer with a trusted certificate within 2 minutes."
    Get-Content (Join-Path $root 'data/caddy/process.log') -Tail 20
    Write-Host 'Check that DNS points here and TCP 443 is open, then: Restart-Service GoRouterCaddy'
    Write-Host 'To undo: copy data\Caddyfile-before-domain.txt to caddy\Caddyfile and data\env-before-domain-backup.txt to .env, then restart GoRouterCaddy and the GoRouter task.'
}
