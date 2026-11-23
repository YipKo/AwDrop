# Launch the interactive WPF app from the source checkout. The MT7922 radio
# remains unavailable until its hardware backend is implemented.
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$app = Join-Path $repo 'build\app\winawdl\winawdl.exe'
if (-not (Test-Path $app)) { throw 'Build first: .\scripts\build-app-local.ps1' }
if (-not (Test-Path (Join-Path $repo '.pixi\envs\default\python.exe'))) {
    throw 'Prepare the project runtime first: .\scripts\adapter-pixi.ps1 python'
}
$env:DOTNET_ROOT = Join-Path $repo '.local\dotnet'
$env:TEMP = Join-Path $repo '.local\tmp'
$env:TMP = $env:TEMP
New-Item -ItemType Directory -Force $env:TEMP | Out-Null
& $app
