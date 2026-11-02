# Started by the GoRouter scheduled task. Runs the gateway and appends its output
# to data\server.log, rotating that file at startup once it exceeds 20 MiB.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $root
[Environment]::CurrentDirectory = $root
$data = Join-Path $root 'data'
New-Item -ItemType Directory -Path $data -Force | Out-Null
$log = Join-Path $data 'server.log'
if ((Test-Path -LiteralPath $log) -and (Get-Item -LiteralPath $log).Length -gt 20MB) {
    Move-Item -LiteralPath $log -Destination (Join-Path $data 'server.log.1') -Force
}
$ErrorActionPreference = 'Continue'
& (Join-Path $root 'go-router.exe') >> $log 2>&1
exit $LASTEXITCODE
