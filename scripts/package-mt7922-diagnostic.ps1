# Assemble source INF and already-built unsigned SYS locally. Never install.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$source = Join-Path $root 'crates\radio\drivers\windows\mt7922\awdrop_mt7922_diagnostic.inf'
$binary = Join-Path $root '.local\driver-build\mt7922-diagnostic\awdrop_mt7922_diagnostic.sys'
$destination = Join-Path $root '.local\driver-package\mt7922-diagnostic-unsigned'
if (-not (Test-Path -LiteralPath $source) -or -not (Test-Path -LiteralPath $binary)) {
    throw 'Build the diagnostic driver first: .\scripts\build-diagnostic-driver.ps1'
}
New-Item -ItemType Directory -Path $destination -Force | Out-Null
if (Get-ChildItem -LiteralPath $destination -Filter '*.cat' -File) {
    throw 'The output directory contains a catalog; remove it explicitly before rebuilding this unsigned package.'
}
Copy-Item -LiteralPath $source -Destination $destination -Force
Copy-Item -LiteralPath $binary -Destination $destination -Force
foreach ($file in @($source, $binary)) {
    $copied = Join-Path $destination (Split-Path -Leaf $file)
    if ((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ne (Get-FileHash -LiteralPath $copied -Algorithm SHA256).Hash) {
        throw "Package copy does not match its source: $copied"
    }
}
Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $destination 'awdrop_mt7922_diagnostic.inf'), (Join-Path $destination 'awdrop_mt7922_diagnostic.sys')
Write-Output 'Unsigned package assembled; catalog missing. Nothing was signed, staged, installed, or loaded.'
