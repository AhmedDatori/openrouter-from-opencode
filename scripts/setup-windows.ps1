param(
    [string]$Origin = 'http://127.0.0.1:8080',
    [string]$Bind = '127.0.0.1:8080'
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $root
if (Test-Path -LiteralPath '.env') { throw '.env already exists. Setup never overwrites credentials.' }
if ($Origin -notmatch '^https?://[^/]+$') { throw 'Origin must be an HTTP(S) origin without a path or trailing slash.' }
if ($Bind -notmatch '^127\.0\.0\.1:\d+$') { throw 'This setup helper binds to loopback. Use an SSH tunnel or HTTPS reverse proxy.' }

# Protect the installation before writing credentials. The scheduled task runs as SYSTEM.
$sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
& icacls.exe $root /inheritance:r /grant:r "*$($sid):(OI)(CI)F" '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Could not protect the installation folder. Extract into a dedicated local folder and retry.' }
$secure = Read-Host 'OpenCode Go API key (hidden)' -AsSecureString
$ptr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
try { $key = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($ptr).Trim() }
finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($ptr) }
if (!$key -or $key.Contains("`r") -or $key.Contains("`n")) { throw 'A single-line Go API key is required.' }
$bytes = New-Object byte[] 32
$rng = [Security.Cryptography.RandomNumberGenerator]::Create()
try { $rng.GetBytes($bytes) } finally { $rng.Dispose() }
$password = [Convert]::ToBase64String($bytes).TrimEnd('=').Replace('+','-').Replace('/','_')
$text = [IO.File]::ReadAllText((Join-Path $root '.env.example'))
$text = $text.Replace('OPENCODE_GO_API_KEY=replace-with-your-opencode-go-key', 'OPENCODE_GO_API_KEY=' + ($key | ConvertTo-Json -Compress))
$text = $text.Replace('ADMIN_PASSWORD=replace-with-a-long-random-password', 'ADMIN_PASSWORD=' + $password)
$text = $text.Replace('BIND=127.0.0.1:8080', 'BIND=' + $Bind)
$text = $text.Replace('PUBLIC_URL=http://localhost:8080', 'PUBLIC_URL=' + $Origin)
$text = $text.Replace('COOKIE_SECURE=false', 'COOKIE_SECURE=' + $Origin.StartsWith('https://').ToString().ToLowerInvariant())
$utf8 = New-Object Text.UTF8Encoding($false)
[IO.File]::WriteAllText((Join-Path $root '.env'), $text, $utf8)
$key = $null
New-Item -ItemType Directory -Path (Join-Path $root 'data') -Force | Out-Null
$login = "Go Router admin`r`nURL: $Origin/admin`r`nPassword: $password`r`n`r`nAPI base URL: $Origin/api/v1`r`nCreate application keys in the dashboard. Do not use the Go upstream key in applications.`r`n"
[IO.File]::WriteAllText((Join-Path $root 'data/admin-login.txt'), $login, $utf8)
Write-Host 'Created private .env and data\admin-login.txt. No credentials printed.'
& (Join-Path $root 'go-router.exe') --check
if ($LASTEXITCODE -ne 0) { throw 'Gateway configuration check failed.' }
