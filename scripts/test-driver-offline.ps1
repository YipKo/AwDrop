$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$compiler = Join-Path $root '.pixi\envs\default\Library\bin\x86_64-w64-mingw32-gcc.exe'
if (-not (Test-Path $compiler)) {
    # Older local setups may still share the reference project's environment.
    $compiler = Join-Path $root 'winawdl\.pixi\envs\default\Library\bin\x86_64-w64-mingw32-gcc.exe'
}
if (-not (Test-Path $compiler)) { throw 'The project-local MinGW compiler is not available.' }
$outDir = Join-Path $root '.local\driver-build'
New-Item -ItemType Directory -Force $outDir | Out-Null
foreach ($name in @('bar0', 'ownership', 'dma_stop', 'wfsys_reset', 'dma_program', 'dma_memory', 'dma_enable', 'mcu_runtime', 'mcu_ownership')) {
    $output = Join-Path $outDir "$name-test.exe"
    $source = Join-Path $root "crates\radio\drivers\windows\mt7922\tests\${name}_test.c"
    & $compiler -std=c11 -Wall -Wextra -Werror $source -o $output
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    & $output
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
Write-Output 'BAR0, DMA queues/registers, ownership and WFSYS reset simulations passed; no device was accessed.'
