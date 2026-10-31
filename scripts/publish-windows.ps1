#Requires -RunAsAdministrator
param(
    [Parameter(Mandatory=$true)][Alias('Address')][string]$Domain,
    [switch]$HttpsOnly
)
$ErrorActionPreference = 'Stop'
$Domain = $Domain.Trim().ToLowerInvariant()
$ipAddress = $null
$isIp = [Net.IPAddress]::TryParse($Domain, [ref]$ipAddress)
if ($isIp) {
    if ($ipAddress.AddressFamily -ne [Net.Sockets.AddressFamily]::InterNetwork) { throw 'This helper currently supports IPv4 addresses and DNS hostnames.' }
    $Domain = $ipAddress.ToString()
    $octets = $ipAddress.GetAddressBytes()
    if ($octets[0] -in @(0,10,127) -or $octets[0] -ge 224 -or ($octets[0] -eq 169 -and $octets[1] -eq 254) -or ($octets[0] -eq 172 -and $octets[1] -ge 16 -and $octets[1] -le 31) -or ($octets[0] -eq 192 -and $octets[1] -eq 168) -or ($octets[0] -eq 100 -and $octets[1] -ge 64 -and $octets[1] -le 127)) {
        throw 'A publicly reachable IPv4 address is required for a trusted public IP certificate.'
    }
} elseif ($Domain.Length -gt 253 -or $Domain -notmatch '^(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z](?:[a-z0-9-]{0,61}[a-z0-9])?$') {
    throw 'Supply a public DNS hostname or IPv4 address without https://, port, or path.'
}
$root = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $root
$caddyRoot = Join-Path $root 'caddy'
$caddy = Join-Path $caddyRoot 'caddy.exe'
$config = Join-Path $caddyRoot 'Caddyfile'
if (Get-Service -Name GoRouterCaddy -ErrorAction SilentlyContinue) {
    throw 'GoRouterCaddy already exists. Update its configuration and restart it instead of reinstalling.'
}
$ports = if ($HttpsOnly) { @(443) } else { @(80,443) }
foreach ($port in $ports) {
    if (Get-NetTCPConnection -State Listen -LocalPort $port -ErrorAction SilentlyContinue) {
        if ($port -eq 80) { throw 'Port 80 is already in use. Retry with -HttpsOnly to use port 443 and TLS-ALPN certificate validation without touching port 80.' }
        throw "Port $port is already in use. Check the existing web server before proceeding."
    }
}
New-Item -ItemType Directory -Path $caddyRoot -Force | Out-Null
if (!(Test-Path -LiteralPath $caddy)) {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    # Pin the standard official release. The custom-build download endpoint may
    # close connections from older Windows/.NET clients before returning a binary.
    $version = '2.11.7'
    $archive = Join-Path $caddyRoot "caddy_$($version)_windows_amd64.zip"
    $url = "https://github.com/caddyserver/caddy/releases/download/v$version/caddy_$($version)_windows_amd64.zip"
    # SHA512 from the official caddy_2.11.7_checksums.txt release asset.
    $expectedHash = 'c308154504e53755958ffc62d8a2657ab17eefd6dd25c45dbb2c3d93d29519971f77ca652f7d0f2bcb7f980b9ba3822478865e29969d0272d7613550a43152b1'
    for ($attempt = 1; $attempt -le 3; $attempt++) {
        try {
            Invoke-WebRequest -UseBasicParsing -TimeoutSec 180 -UserAgent 'go-router-setup/0.1.0' $url -OutFile ($archive + '.download')
            $actualHash = (Get-FileHash -LiteralPath ($archive + '.download') -Algorithm SHA512).Hash.ToLowerInvariant()
            if ($actualHash -ne $expectedHash) { throw 'Caddy download checksum mismatch.' }
            Move-Item -LiteralPath ($archive + '.download') -Destination $archive -Force
            break
        } catch {
            if ($attempt -eq 3) { throw }
            Write-Warning "Caddy download attempt $attempt failed; retrying from the official GitHub release."
            Start-Sleep -Seconds (2 * $attempt)
        }
    }
    $unpacked = Join-Path $caddyRoot 'unpacked'
    Expand-Archive -LiteralPath $archive -DestinationPath $unpacked -Force
    Copy-Item -LiteralPath (Join-Path $unpacked 'caddy.exe') -Destination $caddy
    Copy-Item -LiteralPath (Join-Path $unpacked 'LICENSE') -Destination (Join-Path $caddyRoot 'LICENSE.caddy')
}
& $caddy version
if ($LASTEXITCODE -ne 0) { throw 'The downloaded Caddy executable could not run.' }
$utf8 = New-Object Text.UTF8Encoding($false)
$storage = (Join-Path $root 'data/caddy').Replace('\','/')
$sniConfig = ''
$tlsConfig = ''
$httpsConfig = ''
$profileConfig = ''
$challengeConfig = ''
if ($isIp) {
    # Most TLS clients omit SNI for an IP URL. Select the public address explicitly.
    $sniConfig = "default_sni $Domain"
    $profileConfig = 'profile shortlived'
}
if ($HttpsOnly) {
    $httpsConfig = 'auto_https disable_redirects'
    $challengeConfig = 'disable_http_challenge'
}
if ($isIp -or $HttpsOnly) {
    # Override Caddy's default local issuer for IP addresses. Public IP certificates
    # require Let's Encrypt's shortlived profile and renew automatically.
    $tlsConfig = @"
    tls {
        issuer acme {
            dir https://acme-v02.api.letsencrypt.org/directory
            $profileConfig
            $challengeConfig
        }
    }
"@
}
$caddyfile = @"
{
    admin off
    $sniConfig
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

$envPath = Join-Path $root '.env'
if (!(Test-Path -LiteralPath $envPath)) {
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'setup-windows.ps1') -Origin "https://$Domain"
    if ($LASTEXITCODE -ne 0) { throw 'Gateway setup failed.' }
} else {
    $text = [IO.File]::ReadAllText($envPath)
    # Avoid ambiguous duplicate settings and preserve the upstream/admin keys.
    foreach ($name in @('BIND','PUBLIC_URL','COOKIE_SECURE')) {
        if ([regex]::Matches($text, "(?m)^$name=").Count -ne 1) { throw "Expected exactly one $name entry in .env." }
    }
    $task = Get-ScheduledTask -TaskName GoRouter -ErrorAction SilentlyContinue
    if ($task) { Stop-ScheduledTask -TaskName GoRouter }
    Copy-Item -LiteralPath $envPath -Destination (Join-Path $root 'data/env-before-public-backup.txt')
    $text = [regex]::Replace($text, '(?m)^BIND=[^\r\n]*', 'BIND=127.0.0.1:8080')
    $text = [regex]::Replace($text, '(?m)^PUBLIC_URL=[^\r\n]*', "PUBLIC_URL=https://$Domain")
    $text = [regex]::Replace($text, '(?m)^COOKIE_SECURE=[^\r\n]*', 'COOKIE_SECURE=true')
    [IO.File]::WriteAllText($envPath, $text, $utf8)
    $loginPath = Join-Path $root 'data/admin-login.txt'
    if (Test-Path -LiteralPath $loginPath) {
        $login = [IO.File]::ReadAllText($loginPath)
        $login = [regex]::Replace($login, '(?m)^URL:[^\r\n]*', "URL: https://$Domain/admin")
        $login = [regex]::Replace($login, '(?m)^API base URL:[^\r\n]*', "API base URL: https://$Domain/api/v1")
        [IO.File]::WriteAllText($loginPath, $login, $utf8)
    }
}
if (Get-ScheduledTask -TaskName GoRouter -ErrorAction SilentlyContinue) {
    Start-ScheduledTask -TaskName GoRouter
} else {
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'install-windows-task.ps1')
    if ($LASTEXITCODE -ne 0) { throw 'Gateway startup-task installation failed.' }
}
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
Write-Host "HTTPS proxy started. Public readiness still needs verification from outside the server."
if ($isIp) {
    Write-Host "Public IP certificate: Let's Encrypt shortlived profile; renewals are automatic. No DNS record is required."
} else {
    Write-Host "DNS A record: $Domain -> your server's public IP."
}
Write-Host "The provider firewall/NAT must allow incoming TCP $($ports -join ', ') to reach this Windows server."
if ($HttpsOnly) { Write-Host 'HTTPS-only mode: port 80 is unchanged. Use https:// explicitly. Certificate validation uses TLS-ALPN on port 443.' }
Write-Host "Admin: https://$Domain/admin ; API base URL: https://$Domain/api/v1"
Write-Host "Certificates renew automatically. If issuance fails, check data\caddy\process.log."
