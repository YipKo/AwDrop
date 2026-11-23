$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$env:PIXI_HOME = Join-Path $root '.local\pixi'
$env:RATTLER_CACHE_DIR = Join-Path $root '.local\cache'
$env:CARGO_HOME = Join-Path $root '.local\cargo'
$env:TEMP = Join-Path $root '.local\tmp'
$env:TMP = $env:TEMP
New-Item -ItemType Directory -Force $env:TEMP | Out-Null
Push-Location $root
try {
    & pixi install --no-config
    if ($LASTEXITCODE -ne 0) { throw 'Pixi environment setup failed.' }
    & (Join-Path $PSScriptRoot 'fetch-rust-gnu.ps1')
} finally { Pop-Location }
