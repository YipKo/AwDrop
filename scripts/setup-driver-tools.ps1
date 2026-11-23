$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$env:PIXI_HOME = Join-Path $root '.local\pixi'
$env:RATTLER_CACHE_DIR = Join-Path $root '.local\cache'
$env:TEMP = Join-Path $root '.local\tmp'
$env:TMP = $env:TEMP
New-Item -ItemType Directory -Force $env:TEMP | Out-Null
& pixi install --no-config --manifest-path (Join-Path $root 'crates\radio\drivers\windows\pixi.toml')
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
