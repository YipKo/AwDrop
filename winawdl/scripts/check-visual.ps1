# Offline WPF model, color and layout checks; no background daemons or radio.
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$app = Join-Path $repo 'build\app\winawdl\winawdl.exe'
if (-not (Test-Path $app)) { throw 'Build first: .\scripts\build-app-local.ps1' }
$env:DOTNET_ROOT = Join-Path $repo '.local\dotnet'
$env:TEMP = Join-Path $repo '.local\tmp'
$env:TMP = $env:TEMP
$folder = Join-Path $repo 'build\app\visual-check'
$run = Start-Process $app -ArgumentList '--check-visual', "`"$folder`"" -WindowStyle Hidden -Wait -PassThru
Get-Content (Join-Path $folder 'visual.log') | Select-String '^(FAIL|STOPPED|ALL .*PASSED|\d+ FAILED)'
if ($run.ExitCode -ne 0) { exit $run.ExitCode }
