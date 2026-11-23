# Builds an unsigned diagnostic driver. It does not sign, install or load it.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$driverRoot = Join-Path $root 'crates\radio\drivers\windows\mt7922'
$tools = Join-Path $root 'crates\radio\drivers\windows\.pixi\envs\default\Library\bin'
$wdk = Join-Path $root '.local\wdk-26100\c'
$msvc = Join-Path $root '.local\msvc-headers-14.51\Contents\VC\Tools\MSVC\14.51.36231\include'
$out = Join-Path $root '.local\driver-build\mt7922-diagnostic'
$env:TEMP = Join-Path $root '.local\tmp'
$env:TMP = $env:TEMP
$env:PATH = "$tools;$env:PATH"
$compiler = Join-Path $tools 'clang-cl.exe'
$linker = Join-Path $tools 'lld-link.exe'
if (-not (Test-Path $compiler) -or -not (Test-Path $linker)) { throw 'Run scripts/setup-driver-tools.ps1 first.' }
if (-not (Test-Path (Join-Path $wdk 'Include\wdf\kmdf\1.33\wdf.h'))) { throw 'Run scripts/fetch-wdk.py first.' }
if (-not (Test-Path (Join-Path $msvc 'excpt.h'))) { throw 'Run scripts/fetch-msvc-headers.py first.' }
New-Item -ItemType Directory -Force $out, $env:TEMP | Out-Null
$includes = @(
    $msvc,
    (Join-Path $wdk 'Include\10.0.26100.0\km'),
    (Join-Path $wdk 'Include\10.0.26100.0\shared'),
    (Join-Path $wdk 'Include\10.0.26100.0\ucrt'),
    (Join-Path $wdk 'Include\wdf\kmdf\1.33')
)
$includeArgs = @($includes | ForEach-Object { '/imsvc' + $_ })
$obj = Join-Path $out 'diagnostic.obj'
$sys = Join-Path $out 'awdrop_mt7922_diagnostic.sys'
& $compiler /nologo /c /W4 /WX /kernel /Zl /GS /guard:cf /D_AMD64_ /D_WIN64 /D_WIN32_WINNT=0x0A00 /DNTDDI_VERSION=0x0A00000C /DKMDF_VERSION_MAJOR=1 /DKMDF_VERSION_MINOR=33 /clang:-Wno-msvc-not-found @includeArgs "/Fo$obj" (Join-Path $driverRoot 'diagnostic.c')
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
& $linker /driver:wdm /release /subsystem:native /entry:FxDriverEntry /nodefaultlib /machine:x64 /integritycheck /guard:cf /dynamicbase /nxcompat "/out:$sys" "/libpath:$(Join-Path $wdk 'Lib\10.0.26100.0\km\x64')" "/libpath:$(Join-Path $wdk 'Lib\wdf\kmdf\x64\1.33')" $obj ntoskrnl.lib hal.lib wmilib.lib WdfDriverEntry.lib WdfLdr.lib BufferOverflowFastFailK.lib
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Get-FileHash -Algorithm SHA256 -LiteralPath $sys | Select-Object Algorithm,Hash,Path
Write-Output 'Unsigned diagnostic build only; not installed or loaded.'
