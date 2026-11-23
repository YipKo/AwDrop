param(
    [ValidateSet('build', 'test', 'radio-test', 'run')][string]$Action = 'build',
    [switch]$NoRestore
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

if ($Action -ne 'radio-test') {
    $configuration = if ($Action -eq 'test') { 'Debug' } else { 'Release' }
    $binary = & (Join-Path $root 'desktop\windows\build.ps1') -NoRestore:$NoRestore -Configuration $configuration |
        Tee-Object -Variable buildOutput | Select-Object -Last 1
    if ($buildOutput) { $buildOutput | Write-Host }
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $binary -PathType Leaf)) { throw 'WinUI build failed.' }
    if ($Action -eq 'run') {
        $env:AWDROP_DATA_DIR = Join-Path $root '.local\userdata'
        Start-Process -FilePath $binary
    } elseif ($Action -eq 'test') {
        $data = Join-Path $root ('.local\winui\desktop-check-' + [Guid]::NewGuid().ToString('N'))
        $null = New-Item -ItemType Directory -Path $data -Force
        $env:AWDROP_DATA_DIR = $data
        $env:AWDROP_DESKTOP_CHECK = Join-Path $data 'report.json'
        $env:AWDROP_CHECK_SETTINGS = $data
        $process = Start-Process -FilePath $binary -WindowStyle Hidden -PassThru
        try {
            $limit = [DateTime]::UtcNow.AddSeconds(25)
            while (-not $process.HasExited -and [DateTime]::UtcNow -lt $limit) {
                if (Test-Path -LiteralPath $env:AWDROP_DESKTOP_CHECK) {
                    $json = Get-Content -LiteralPath $env:AWDROP_DESKTOP_CHECK -Raw
                    try { $report = ConvertFrom-Json -InputObject $json -ErrorAction Stop; break }
                    catch { }
                }
                Start-Sleep -Milliseconds 200
            }
            if (-not $report) { throw 'WinUI did not produce a test report.' }
            $json
            if ($report | Where-Object Error) { throw 'WinUI desktop checks failed.' }
        } finally {
            if (-not $process.HasExited) { Stop-Process -Id $process.Id -ErrorAction Stop; $null = $process.WaitForExit(5000) }
            Remove-Item Env:AWDROP_DESKTOP_CHECK, Env:AWDROP_CHECK_SETTINGS
        }
    }
    return
}

$prefix = Join-Path $root '.pixi\envs\default'
$env:PIXI_HOME = Join-Path $root '.local\pixi'
$env:RATTLER_CACHE_DIR = Join-Path $root '.local\cache'
$env:CARGO_HOME = Join-Path $root '.local\cargo'
$env:CARGO_TARGET_DIR = Join-Path $root '.local\target'
$env:TEMP = Join-Path $root '.local\tmp'
$env:TMP = $env:TEMP
$env:CARGO_BUILD_JOBS = '4'
$env:RUSTC = Join-Path $root '.local\rust-gnu\bin\rustc.exe'
$env:RUSTFLAGS = '-C dlltool=x86_64-w64-mingw32-dlltool'
New-Item -ItemType Directory -Force $env:TEMP | Out-Null
if (-not (Test-Path (Join-Path $prefix 'Library\bin\cargo.exe'))) {
    # Reuse an existing local reference environment, if present, to avoid a second download.
    $legacyPrefix = Join-Path $root 'winawdl\.pixi\envs\default'
    if (Test-Path (Join-Path $legacyPrefix 'Library\bin\cargo.exe')) {
        $prefix = $legacyPrefix
        $env:CARGO_HOME = Join-Path $root 'winawdl\.local\cargo'
    } else { throw 'Prepare the project toolchain: .\scripts\setup-windows.ps1' }
}
if (-not (Test-Path $env:RUSTC)) { throw 'Prepare the project-local GNU compiler: .\scripts\fetch-rust-gnu.ps1' }
$env:PATH = "$prefix\Library\bin;$prefix;$env:PATH"
Push-Location $root
try {
    $cargoArgs = @('test', '--locked', '--package', 'awdrop-radio', '--target', 'x86_64-pc-windows-gnu')
    if ($NoRestore) { $cargoArgs += '--offline' }
    & cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
} finally { Pop-Location }
