# Offline WDK checks; creates no catalog and does not install anything.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$out = Join-Path $root '.local\driver-package\mt7922-diagnostic-unsigned'
$inf = Join-Path $out 'awdrop_mt7922_diagnostic.inf'
$sys = Join-Path $out 'awdrop_mt7922_diagnostic.sys'
$sourceInf = Join-Path $root 'crates\radio\drivers\windows\mt7922\awdrop_mt7922_diagnostic.inf'
$sourceSys = Join-Path $root '.local\driver-build\mt7922-diagnostic\awdrop_mt7922_diagnostic.sys'
$verifier = Join-Path $root '.local\wdk-validation\tools\c\tools\10.0.26100.0\x64\infverif.exe'
$inf2cat = Join-Path $root '.local\wdk-validation\tools\c\bin\10.0.26100.0\x86\Inf2Cat.exe'
foreach ($file in @($inf, $sys, $sourceInf, $sourceSys, $verifier, $inf2cat)) {
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "Required local file missing: $file" }
}
foreach ($pair in @(@($inf, $sourceInf), @($sys, $sourceSys))) {
    if ((Get-FileHash -LiteralPath $pair[0] -Algorithm SHA256).Hash -ne (Get-FileHash -LiteralPath $pair[1] -Algorithm SHA256).Hash) {
        throw 'The package differs from current sources/build; run scripts/package-mt7922-diagnostic.ps1.'
    }
}
$env:TEMP = Join-Path $root '.local\tmp'
$env:TMP = $env:TEMP
New-Item -ItemType Directory -Path $env:TEMP -Force | Out-Null
& $verifier /w $inf
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
& $inf2cat "/driver:$out" /os:10_GE_X64 /nocat
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Write-Output 'INF and signability checks passed without creating a catalog or installing a driver.'
