param([ValidateSet('test', 'info', 'awdl', 'mock', 'python')][string]$Action = 'test')
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
# Keep Pixi's package cache, Cargo's registry and build output in this checkout.
$env:PIXI_HOME = Join-Path $repo '.local\pixi'
$env:RATTLER_CACHE_DIR = Join-Path $repo '.local\cache'
$env:CARGO_HOME = Join-Path $repo '.local\cargo'
$env:CARGO_TARGET_DIR = Join-Path $repo 'build\adapter'
$env:TEMP = Join-Path $repo '.local\tmp'
$env:TMP = $env:TEMP
New-Item -ItemType Directory -Force $env:TEMP | Out-Null
Push-Location $repo
try {
    if ($Action -eq 'info') { pixi info --no-config }
    elseif ($Action -eq 'awdl') { pixi run --no-config awdl-build }
    elseif ($Action -eq 'mock') { pixi run --no-config adapter-mock }
    elseif ($Action -eq 'python') { pixi run --no-config python-check }
    else { pixi run --no-config adapter-test }
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
} finally { Pop-Location }
