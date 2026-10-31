$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $root
New-Item -ItemType Directory -Path (Join-Path $root 'data') -Force | Out-Null
$ErrorActionPreference = 'Continue'
& (Join-Path $root 'go-router.exe') >> (Join-Path $root 'data/server.log') 2>&1
exit $LASTEXITCODE
