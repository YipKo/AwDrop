# Build an unsigned x64 WinUI desktop preview installer.
param([string]$Iscc, [switch]$NoRestore)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if (-not $Iscc) {
    $candidates = @((Join-Path $root '.local\inno\ISCC.exe'), "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:ProgramFiles\Inno Setup 6\ISCC.exe", "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe", "$env:ProgramFiles\Inno Setup 7\ISCC.exe", "$env:LOCALAPPDATA\Programs\Inno Setup 7\ISCC.exe")
    $Iscc = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $Iscc) { $cmd = Get-Command ISCC.exe -ErrorAction SilentlyContinue; if ($cmd) { $Iscc = $cmd.Source } }
}
if (-not $Iscc -or -not (Test-Path $Iscc)) { throw 'Inno Setup 6 or 7 ISCC.exe is required; pass its path with -Iscc.' }
[xml]$project = Get-Content -LiteralPath (Join-Path $root 'desktop\windows\AwDrop.Windows.csproj') -Raw
$version = $project.Project.PropertyGroup.Version
if ($version -notmatch '^\d+\.\d+\.\d+$') { throw 'Could not read the Windows desktop version.' }
$output = Join-Path $root 'build\installer'
New-Item -ItemType Directory -Force $output | Out-Null
Push-Location $root
try {
    $binary = & (Join-Path $root 'desktop\windows\build.ps1') -NoRestore:$NoRestore |
        Tee-Object -Variable buildOutput | Select-Object -Last 1
    if ($buildOutput) { $buildOutput | Write-Host }
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $binary -PathType Leaf)) { throw 'WinUI build failed.' }
    $appDir = Split-Path -Parent $binary
    & $Iscc /Qp "/DVersion=$version" "/DAppDir=$appDir" "/DOutput=$output" (Join-Path $root 'packaging\windows\awdrop-preview.iss')
    if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed ($LASTEXITCODE)." }
    $setup = Join-Path $output "AwDrop_${version}_x64-setup.exe"
    $hash = (Get-FileHash -Algorithm SHA256 $setup).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText("$setup.sha256", "$hash  $(Split-Path $setup -Leaf)`n", [Text.Encoding]::ASCII)
    Get-Item $setup, "$setup.sha256" | Select-Object FullName, Length
    "SHA256 $hash"
} finally { Pop-Location }
