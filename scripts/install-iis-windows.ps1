#Requires -RunAsAdministrator
# Publish the gateway as an IIS website. Use this on servers where IIS already serves
# other websites on ports 80/443. Run from the cloned repository:
#   powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\install-iis-windows.ps1 -Domain api.example.com
#
# IIS starts go-router.exe through the ASP.NET Core Module, restarts it if it stops,
# and serves HTTPS with a certificate that is already installed on the server (for
# example a *.example.com wildcard). No Caddy service and no startup task are used.
param(
    [Parameter(Mandatory=$true)][string]$Domain,
    # Optional: the certificate to use. By default the newest valid certificate in the
    # computer's store that covers -Domain (exact name or wildcard) is chosen.
    [string]$CertificateThumbprint,
    [string]$SiteName = 'GoRouter'
)
$ErrorActionPreference = 'Stop'
$Domain = $Domain.Trim().ToLowerInvariant()
if ($Domain.Length -gt 253 -or $Domain -notmatch '^(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z](?:[a-z0-9-]{0,61}[a-z0-9])?$') {
    throw 'Supply a DNS hostname without https://, port, or path, for example api.example.com.'
}
if ($SiteName -notmatch '^[A-Za-z0-9_-]{1,64}$') { throw 'SiteName may contain only letters, digits, - and _.' }
$root = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $root
[Environment]::CurrentDirectory = $root
$iisRoot = Join-Path $root 'iis'

# 1. Preconditions: IIS with the ASP.NET Core Module, no other Go Router installation.
if (!(Get-Service -Name W3SVC -ErrorAction SilentlyContinue)) { throw 'IIS is not installed on this server.' }
Import-Module WebAdministration
if (!(Get-WebGlobalModule -Name AspNetCoreModuleV2 -ErrorAction SilentlyContinue)) {
    throw 'The ASP.NET Core Module for IIS is missing. Install the latest "ASP.NET Core Hosting Bundle" from https://dotnet.microsoft.com/download/dotnet , run "iisreset", then retry.'
}
if ((Get-Service -Name GoRouterCaddy -ErrorAction SilentlyContinue) -or (Get-ScheduledTask -TaskName GoRouter -ErrorAction SilentlyContinue)) {
    throw 'Go Router is already installed here with Caddy. Use either that installation or IIS, not both.'
}
if (Get-Website -Name $SiteName) { throw "IIS site $SiteName already exists. Use update-windows.ps1 to upgrade." }
if (Test-Path "IIS:\AppPools\$SiteName") { throw "IIS application pool $SiteName already exists. Remove it or choose another -SiteName." }
$taken = Get-WebBinding | Where-Object { $_.bindingInformation -match (':' + [regex]::Escape($Domain) + '$') }
if ($taken) { throw "Another IIS site already has a binding for $Domain. Remove that binding first." }

# 2. Lock the folder before anything is downloaded into it.
& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'secure-windows.ps1') -AclOnly
if ($LASTEXITCODE -ne 0) { throw 'Could not secure the installation folder.' }

$addresses = @(Resolve-DnsName -Name $Domain -Type A -DnsOnly -ErrorAction SilentlyContinue | Where-Object { $_.Type -eq 'A' } | ForEach-Object { $_.IPAddress })
if ($addresses.Count -eq 0) { throw "$Domain has no DNS A record yet. Point it at this server's public IPv4 address and retry." }
Write-Host "$Domain resolves to $($addresses -join ', '). It must be this server's public IPv4 address."

# 3. HTTPS certificate already installed on this server.
$parent = $Domain.Substring($Domain.IndexOf('.') + 1)
$now = Get-Date
$certificate = $null
$storeName = $null
foreach ($store in @('My', 'WebHosting')) {
    if (!(Test-Path "Cert:\LocalMachine\$store")) { continue }
    $found = Get-ChildItem "Cert:\LocalMachine\$store" | Where-Object {
        if ($CertificateThumbprint) { return $_.Thumbprint -eq $CertificateThumbprint.Replace(' ', '').ToUpperInvariant() }
        $names = @($_.DnsNameList | ForEach-Object { $_.Unicode.ToLowerInvariant() })
        $_.HasPrivateKey -and $_.NotBefore -lt $now -and $_.NotAfter -gt $now.AddDays(1) -and
            (($names -contains $Domain) -or ($names -contains "*.$parent")) -and $_.Verify()
    } | Sort-Object NotAfter -Descending | Select-Object -First 1
    if ($found -and (!$certificate -or $found.NotAfter -gt $certificate.NotAfter)) { $certificate = $found; $storeName = $store }
}
if (!$certificate) {
    throw "No valid certificate for $Domain (or *.$parent) with a private key was found in the computer's certificate store. Install one, or pass -CertificateThumbprint."
}
if (!$certificate.HasPrivateKey) { throw 'The chosen certificate has no private key.' }
Write-Host "Certificate: $($certificate.Subject), valid until $($certificate.NotAfter.ToString('yyyy-MM-dd')), store $storeName."
if ($certificate.NotAfter -lt $now.AddDays(30)) { Write-Warning 'This certificate expires within 30 days. Renew it in IIS before then.' }

# 4. Gateway executable from the latest GitHub release (checksum verified).
& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'update-windows.ps1')
if ($LASTEXITCODE -ne 0) { throw 'Downloading the gateway failed.' }

# 5. Private configuration and admin password (never overwrites an existing .env).
if (!(Test-Path -LiteralPath (Join-Path $root '.env'))) {
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'setup-windows.ps1') -Origin "https://$Domain"
    if ($LASTEXITCODE -ne 0) { throw 'Gateway setup failed.' }
} else {
    Write-Host ".env already exists and was left unchanged. Check that PUBLIC_URL is https://$Domain"
}

# 6. iis\web.config (shipped with the project) forwards every request to go-router.exe.
#    The site folder holds only this file, so IIS can never serve .env or data\.
if (!(Test-Path -LiteralPath (Join-Path $iisRoot 'web.config'))) { throw 'iis\web.config is missing. Download the project again.' }

# 7. Application pool: no .NET runtime, LocalService account, never idles or recycles
#    on a timer (a recycle would cut streams that are in progress).
$pool = "IIS:\AppPools\$SiteName"
New-WebAppPool -Name $SiteName | Out-Null
Set-ItemProperty $pool -Name managedRuntimeVersion -Value ''
Set-ItemProperty $pool -Name startMode -Value 'AlwaysRunning'
Set-ItemProperty $pool -Name processModel.identityType -Value 'LocalService'
Set-ItemProperty $pool -Name processModel.idleTimeout -Value ([TimeSpan]::Zero)
Set-ItemProperty $pool -Name recycling.periodicRestart.time -Value ([TimeSpan]::Zero)

# 8. Website: HTTPS on 443 for this hostname only (SNI), next to the existing sites.
New-Website -Name $SiteName -PhysicalPath $iisRoot -ApplicationPool $SiteName -HostHeader $Domain -Port 443 -Ssl -SslFlags 1 | Out-Null
(Get-WebBinding -Name $SiteName -Protocol https).AddSslCertificate($certificate.Thumbprint, $storeName)

# Re-apply folder permissions (new files, admin-login.txt for Administrators only).
& powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'secure-windows.ps1') -AclOnly
if ($LASTEXITCODE -ne 0) { throw 'Could not secure the installation folder.' }
Start-WebAppPool -Name $SiteName -ErrorAction SilentlyContinue
Start-Website -Name $SiteName

# 9. Wait for a healthy response through IIS.
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$ok = $false
for ($i = 0; $i -lt 20 -and !$ok; $i++) {
    Start-Sleep -Seconds 3
    try { $ok = (Invoke-WebRequest -UseBasicParsing -TimeoutSec 10 "https://$Domain/healthz").StatusCode -eq 200 } catch { }
}
if ($ok) {
    Write-Host "Installed in IIS. Admin: https://$Domain/admin ; API base URL: https://$Domain/api/v1"
    Write-Host 'Read the admin password with: notepad .\data\admin-login.txt  (delete the file after you have saved it)'
} else {
    Write-Warning "https://$Domain/healthz did not answer within 1 minute."
    Get-ChildItem (Join-Path $root 'data\logs') -Filter 'stdout*.log' -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1 | ForEach-Object { Get-Content $_.FullName -Tail 20 }
    Write-Host 'Also check Event Viewer > Windows Logs > Application for "IIS AspNetCore Module V2" errors.'
}
