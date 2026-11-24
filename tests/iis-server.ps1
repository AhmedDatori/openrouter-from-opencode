# Disposable GitHub Actions Windows runner only. Reproduces README.txt settings.
$ErrorActionPreference = 'Stop'
Import-Module WebAdministration
$root = $env:IIS_PACKAGE_ROOT
$pool = 'IIS:\AppPools\GoRouter'
New-WebAppPool -Name GoRouter | Out-Null
Set-ItemProperty $pool -Name managedRuntimeVersion -Value ''
Set-ItemProperty $pool -Name enable32BitAppOnWin64 -Value $false
Set-ItemProperty $pool -Name startMode -Value AlwaysRunning
Set-ItemProperty $pool -Name processModel.identityType -Value ApplicationPoolIdentity
Set-ItemProperty $pool -Name processModel.idleTimeout -Value ([TimeSpan]::Zero)
Set-ItemProperty $pool -Name recycling.periodicRestart.time -Value ([TimeSpan]::Zero)

# Use a path containing spaces and remove inherited user access. Only the pool
# reads the app, and only data is writable, as described to the installer.
& icacls.exe $root /inheritance:r /grant:r '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' 'IIS AppPool\GoRouter:(OI)(CI)RX'
if ($LASTEXITCODE) { throw 'Could not apply program ACLs' }
& icacls.exe (Join-Path $root data) /grant 'IIS AppPool\GoRouter:(OI)(CI)M'
if ($LASTEXITCODE) { throw 'Could not apply data ACLs' }

$cert = New-SelfSignedCertificate -DnsName localhost -CertStoreLocation Cert:\LocalMachine\My
New-Website -Name GoRouter -PhysicalPath (Join-Path $root iis) -ApplicationPool GoRouter -Port 8443 -Ssl | Out-Null
(Get-WebBinding -Name GoRouter -Protocol https).AddSslCertificate($cert.Thumbprint, 'My')
Set-WebConfigurationProperty -PSPath MACHINE/WEBROOT/APPHOST -Location GoRouter -Filter system.webServer/security/authentication/anonymousAuthentication -Name enabled -Value $true
Set-WebConfigurationProperty -PSPath MACHINE/WEBROOT/APPHOST -Location GoRouter -Filter system.webServer/security/authentication/anonymousAuthentication -Name userName -Value ''
Start-Website GoRouter
