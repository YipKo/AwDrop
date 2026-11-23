# Fetch official GNU-hosted Rust components into this project. No installer,
# registry changes or persistent PATH changes. SHA-256 files come from Rust.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$work = Join-Path $root '.local\rust-download'
$prefix = Join-Path $root '.local\rust-gnu'
New-Item -ItemType Directory -Force $work, $prefix | Out-Null
$packages = @(
    @{ Archive = 'rustc-1.99.1-x86_64-pc-windows-gnu'; Component = 'rustc' },
    @{ Archive = 'rust-std-1.99.1-x86_64-pc-windows-gnu'; Component = 'rust-std-x86_64-pc-windows-gnu' }
)
foreach ($package in $packages) {
    $url = "https://static.rust-lang.org/dist/$($package.Archive).tar.xz"
    $archive = Join-Path $work "$($package.Archive).tar.xz"
    $checksumFile = Join-Path $work "$($package.Archive).sha256"
    Invoke-WebRequest "$url.sha256" -OutFile $checksumFile
    $checksum = (Get-Content -LiteralPath $checksumFile -Raw).Trim().Split(' ')[0]
    Invoke-WebRequest $url -OutFile $archive
    if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $checksum) {
        throw "Checksum mismatch: $($package.Archive)"
    }
    & tar.exe -xf $archive -C $work
    if ($LASTEXITCODE -ne 0) { throw "Extraction failed: $($package.Archive)" }
    $component = Join-Path (Join-Path $work $package.Archive) $package.Component
    if (-not (Test-Path -LiteralPath $component)) { throw "Missing component: $component" }
    Copy-Item -Path (Join-Path $component '*') -Destination $prefix -Recurse -Force
}
& (Join-Path $prefix 'bin\rustc.exe') -vV
if ($LASTEXITCODE -ne 0) { throw 'The project-local compiler could not start.' }
