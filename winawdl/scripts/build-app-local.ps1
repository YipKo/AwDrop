$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$env:DOTNET_ROOT = Join-Path $repo '.local\dotnet'
$env:DOTNET_CLI_HOME = Join-Path $repo '.local\dotnet-home'
$env:NUGET_PACKAGES = Join-Path $repo '.local\nuget'
$env:APPDATA = Join-Path $repo '.local\appdata'
$env:LOCALAPPDATA = Join-Path $repo '.local\localappdata'
$env:DOTNET_CLI_TELEMETRY_OPTOUT = '1'
$env:DOTNET_GENERATE_ASPNET_CERTIFICATE = 'false'
$env:TEMP = Join-Path $repo '.local\tmp'
$env:TMP = $env:TEMP
New-Item -ItemType Directory -Force $env:TEMP | Out-Null
$dotnet = Join-Path $env:DOTNET_ROOT 'dotnet.exe'
if (-not (Test-Path $dotnet)) { throw 'Put the .NET 10 SDK in .local\dotnet first.' }
& $dotnet build (Join-Path $repo 'app\winawdl.csproj') -c Release -nologo -v minimal
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
