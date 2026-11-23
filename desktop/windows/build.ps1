param(
    [switch]$NoRestore,
    [string]$SharePublisher,
    [ValidateSet('Debug', 'Release')][string]$Configuration = 'Release'
)

$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$local = Join-Path $repo '.local/winui'
$project = Join-Path $PSScriptRoot 'AwDrop.Windows.csproj'
$bridge = Join-Path $repo 'desktop/bridge/Cargo.toml'
$dotnet = Join-Path $repo 'winawdl/.local/dotnet/dotnet.exe'
if (!(Test-Path $dotnet)) { $dotnet = (Get-Command dotnet -ErrorAction Stop).Source }

$prefix = Join-Path $repo '.pixi/envs/default'
$env:CARGO_HOME = Join-Path $repo '.local/cargo'
if (!(Test-Path (Join-Path $prefix 'Library/bin/cargo.exe'))) {
    $prefix = Join-Path $repo 'winawdl/.pixi/envs/default'
    $env:CARGO_HOME = Join-Path $repo 'winawdl/.local/cargo'
}
$cargo = Join-Path $prefix 'Library/bin/cargo.exe'
if (!(Test-Path $cargo)) { throw 'Prepare the project Rust toolchain with scripts/setup-windows.ps1.' }
$env:RUSTC = Join-Path $repo '.local/rust-gnu/bin/rustc.exe'
if (!(Test-Path $env:RUSTC)) { throw 'Prepare the project Rust compiler with scripts/fetch-rust-gnu.ps1.' }
$env:RUSTFLAGS = '-C dlltool=x86_64-w64-mingw32-dlltool'
$env:CARGO_TARGET_DIR = Join-Path $local 'rust-target'
$env:PATH = "$prefix/Library/bin;$prefix;$env:PATH"
$env:DOTNET_CLI_HOME = Join-Path $local 'dotnet-home'
$env:NUGET_PACKAGES = Join-Path $local 'packages'
$env:NUGET_HTTP_CACHE_PATH = Join-Path $local 'http-cache'
$env:DOTNET_CLI_TELEMETRY_OPTOUT = '1'
$env:DOTNET_GENERATE_ASPNET_CERTIFICATE = 'false'

Push-Location $repo
try {
    $cargoArgs = @('build', '--locked', '--manifest-path', $bridge, '--release', '--target', 'x86_64-pc-windows-gnu')
    if ($NoRestore) { $cargoArgs += '--offline' }
    & $cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    $dotnetArgs = @('build', $project, '-c', $Configuration)
    if ($NoRestore) { $dotnetArgs += '--no-restore' }
    $output = Join-Path $PSScriptRoot "bin/$Configuration/net10.0-windows10.0.19041.0/win-x64"
    if ($SharePublisher) {
        $output = Join-Path $local 'share-app'
        $shareSource = Join-Path $local 'share-source'
        $shareAssets = Join-Path $shareSource 'Assets'
        $null = New-Item -ItemType Directory -Path $shareAssets -Force
        [xml]$package = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'share/AppxManifest.xml') -Raw
        $package.Package.Identity.Publisher = $SharePublisher
        $package.Save((Join-Path $shareSource 'AppxManifest.xml'))

        [xml]$executable = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'app.manifest') -Raw
        $identity = $executable.CreateElement('msix', 'urn:schemas-microsoft-com:msix.v1')
        $identity.SetAttribute('publisher', $SharePublisher)
        $identity.SetAttribute('packageName', $package.Package.Identity.Name)
        $identity.SetAttribute('applicationId', $package.Package.Applications.Application.Id)
        $null = $executable.DocumentElement.AppendChild($identity)
        $identityPath = Join-Path $local 'share-app.manifest'
        $executable.Save($identityPath)
        $dotnetArgs += @('-o', $output, "-p:ApplicationManifest=$identityPath")
    }
    & $dotnet @dotnetArgs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    $builtBridge = Join-Path $env:CARGO_TARGET_DIR 'x86_64-pc-windows-gnu/release/awdrop-desktop-bridge.exe'
    Copy-Item -LiteralPath $builtBridge -Destination $output
    Get-Item -LiteralPath (Join-Path $output 'AwDrop.Windows.exe') | Select-Object -ExpandProperty FullName
    if ($SharePublisher) {
        Add-Type -AssemblyName System.Drawing
        $logo = [Drawing.Image]::FromFile((Join-Path $output 'AwDrop.png'))
        try {
            $sizes = @{ 'StoreLogo' = 50; 'Square44x44Logo' = 44; 'Square150x150Logo' = 150 }
            foreach ($name in $sizes.Keys) {
                $size = $sizes[$name]
                $image = $logo.GetThumbnailImage($size, $size, $null, [IntPtr]::Zero)
                try { $image.Save((Join-Path $shareAssets "$name.png"), [Drawing.Imaging.ImageFormat]::Png) }
                finally { $image.Dispose() }
            }
        } finally { $logo.Dispose() }
        $makeAppx = Join-Path $env:NUGET_PACKAGES 'microsoft.windows.sdk.buildtools/10.0.26100.4654/bin/10.0.26100.0/x64/makeappx.exe'
        $msix = Join-Path $local 'AwDrop.Share.msix'
        & $makeAppx pack /o /nv /d $shareSource /p $msix
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
        $msix
    }
} finally {
    Pop-Location
}
