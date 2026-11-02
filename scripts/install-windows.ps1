#Requires -RunAsAdministrator
# First installation on Windows Server. Run from the cloned repository:
#   powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-windows.ps1 -Domain api.example.com
# It downloads the gateway release, creates the private .env and admin password,
# registers the GoRouter startup task, and installs Caddy as the GoRouterCaddy
# HTTPS service with an automatic Let's Encrypt certificate for -Domain.
param(
    [Parameter(Mandatory=$true)][string]$Domain,
    # Use when another program (often IIS or HTTP.sys) already owns port 80.
    [switch]$HttpsOnly
)
$ErrorActionPreference = 'Stop'
$Domain = $Domain.Trim().ToLowerInvariant()
if ($Domain.Length -gt 253 -or $Domain -notmatch '^(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z](?:[a-z0-9-]{0,61}[a-z0-9])?$') {
    throw 'Supply a DNS hostname without https://, port, or path, for example api.example.com.'
}
$root = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $root
[Environment]::CurrentDirectory = $root
$caddyRoot = Join-Path $root 'caddy'
$caddy = Join-Path $caddyRoot 'caddy.exe'
$config = Join-Path $caddyRoot 'Caddyfile'
if (Get-Service -Name GoRouterCaddy -ErrorAction SilentlyContinue) {
    throw 'Go Router is already installed. Use update-windows.ps1 to upgrade or set-domain-windows.ps1 to change the domain.'
}

# 1. Preconditions: DNS and free ports.
$addresses = @(Resolve-DnsName -Name $Domain -Type A -DnsOnly -ErrorAction SilentlyContinue | Where-Object { $_.Type -eq 'A' } | ForEach-Object { $_.IPAddress })
if ($addresses.Count -eq 0) { throw "$Domain has no DNS A record yet. Point it at this server's public IPv4 address and retry." }
Write-Host "$Domain resolves to $($addresses -join ', '). It must be this server's public IPv4 address."
$ports = if ($HttpsOnly) { @(443) } else { @(80,443) }
foreach ($port in $ports) {
    if (Get-NetTCPConnection -State Listen -LocalPort $port -ErrorAction SilentlyContinue) {
        if ($port -eq 80) { throw 'Port 80 is already in use. Retry with -HttpsOnly to use port 443 only.' }
        throw "Port $port is already in use. Check the existing web server before proceeding."
    }
}

# 2. Gateway executable from the latest GitHub release (checksum verified).
if (!(Test-Path -LiteralPath (Join-Path $root 'go-router.exe'))) {
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'update-windows.ps1')
    if ($LASTEXITCODE -ne 0) { throw 'Downloading the gateway failed.' }
}

# 3. Caddy, pinned to an official release and verified by SHA512.
New-Item -ItemType Directory -Path $caddyRoot -Force | Out-Null
if (!(Test-Path -LiteralPath $caddy)) {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    $version = '2.11.7'
    $archive = Join-Path $caddyRoot "caddy_$($version)_windows_amd64.zip"
    $url = "https://github.com/caddyserver/caddy/releases/download/v$version/caddy_$($version)_windows_amd64.zip"
    # SHA512 from the official caddy_2.11.7_checksums.txt release asset.
    $expectedHash = 'c308154504e53755958ffc62d8a2657ab17eefd6dd25c45dbb2c3d93d29519971f77ca652f7d0f2bcb7f980b9ba3822478865e29969d0272d7613550a43152b1'
    for ($attempt = 1; $attempt -le 3; $attempt++) {
        try {
            Invoke-WebRequest -UseBasicParsing -TimeoutSec 180 -UserAgent 'go-router-setup' $url -OutFile ($archive + '.download')
            $actualHash = (Get-FileHash -LiteralPath ($archive + '.download') -Algorithm SHA512).Hash.ToLowerInvariant()
            if ($actualHash -ne $expectedHash) { throw 'Caddy download checksum mismatch.' }
            Move-Item -LiteralPath ($archive + '.download') -Destination $archive -Force
            break
        } catch {
            if ($attempt -eq 3) { throw }
            Write-Warning "Caddy download attempt $attempt failed; retrying."
            Start-Sleep -Seconds (2 * $attempt)
        }
    }
    $unpacked = Join-Path $caddyRoot 'unpacked'
    Expand-Archive -LiteralPath $archive -DestinationPath $unpacked -Force
    Copy-Item -LiteralPath (Join-Path $unpacked 'caddy.exe') -Destination $caddy
    Copy-Item -LiteralPath (Join-Path $unpacked 'LICENSE') -Destination (Join-Path $caddyRoot 'LICENSE.caddy')
    Remove-Item -LiteralPath $unpacked, $archive -Recurse -Force
}
& $caddy version
if ($LASTEXITCODE -ne 0) { throw 'The Caddy executable could not run.' }

# 4. Caddyfile: HTTPS for the domain only, proxied to the loopback gateway.
$utf8 = New-Object Text.UTF8Encoding($false)
$storage = (Join-Path $root 'data/caddy').Replace('\','/')
$httpsConfig = ''
$tlsConfig = ''
if ($HttpsOnly) {
    # No HTTP redirects or HTTP challenge; certificates are validated over TLS-ALPN on 443.
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
    reverse_proxy 127.0.0.1:8080 {
        flush_interval -1
    }
}
"@
[IO.File]::WriteAllText($config, $caddyfile, $utf8)
& $caddy validate --config $config --adapter caddyfile
if ($LASTEXITCODE -ne 0) { throw 'Caddy configuration is invalid.' }

# 5. Private configuration and admin password (never overwrites an existing .env).
if (!(Test-Path -LiteralPath (Join-Path $root '.env'))) {
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'setup-windows.ps1') -Origin "https://$Domain"
    if ($LASTEXITCODE -ne 0) { throw 'Gateway setup failed.' }
} else {
    Write-Host '.env already exists and was left unchanged. Use set-domain-windows.ps1 if the domain differs.'
}

# 6. Gateway startup task (runs as SYSTEM, survives logoff and reboot).
if (Get-ScheduledTask -TaskName GoRouter -ErrorAction SilentlyContinue) {
    Start-ScheduledTask -TaskName GoRouter
} else {
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'install-windows-task.ps1')
    if ($LASTEXITCODE -ne 0) { throw 'Gateway startup-task installation failed.' }
}

# 7. Windows Firewall rules (Caddy only) and the Caddy service.
foreach ($port in $ports) {
    $name = "GoRouter-HTTPS-TCP-$port"
    if (!(Get-NetFirewallRule -Name $name -ErrorAction SilentlyContinue)) {
        New-NetFirewallRule -Name $name -DisplayName "Go Router HTTPS TCP $port" -Direction Inbound -Action Allow -Protocol TCP -LocalPort $port -Program $caddy | Out-Null
    }
}
$serviceCommand = '"' + $caddy + '" run --config "' + $config + '" --adapter caddyfile'
New-Service -Name GoRouterCaddy -BinaryPathName $serviceCommand -StartupType Automatic -DisplayName 'Go Router HTTPS (Caddy)' | Out-Null
& sc.exe failure GoRouterCaddy reset= 86400 actions= restart/5000/restart/15000/restart/60000 | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Could not set Caddy service recovery settings.' }
Start-Service GoRouterCaddy

# 8. Wait for the certificate and a healthy response through HTTPS.
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$ok = $false
for ($i = 0; $i -lt 24 -and !$ok; $i++) {
    Start-Sleep -Seconds 5
    try { $ok = (Invoke-WebRequest -UseBasicParsing -TimeoutSec 10 "https://$Domain/healthz").StatusCode -eq 200 } catch { }
}
if ($ok) {
    Write-Host "Installed. Admin: https://$Domain/admin ; API base URL: https://$Domain/api/v1"
    Write-Host 'Read the admin password with: notepad .\data\admin-login.txt'
} else {
    Write-Warning "https://$Domain/healthz did not answer with a trusted certificate within 2 minutes."
    Get-Content (Join-Path $root 'data/caddy/process.log') -Tail 20 -ErrorAction SilentlyContinue
    Write-Host "Check that DNS points here and that the hosting provider's firewall allows TCP $($ports -join ', '). Then: Restart-Service GoRouterCaddy"
}
