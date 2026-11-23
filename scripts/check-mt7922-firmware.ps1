# Inspect explicitly selected local firmware. Does not load it or touch a device.
param([Parameter(Mandatory = $true)][string]$FirmwareDirectory)
$ErrorActionPreference = 'Stop'
$directory = (Resolve-Path -LiteralPath $FirmwareDirectory -ErrorAction Stop).Path
foreach ($name in @('WIFI_MT7922_patch_mcu_1_1_hdr.bin', 'WIFI_RAM_CODE_MT7922_1.bin')) {
    $path = Join-Path $directory $name
    $file = Get-Item -LiteralPath $path -ErrorAction Stop
    if ($file.PSIsContainer -or $file.Length -gt 16MB -or $file.Length -eq 0) {
        throw "Invalid firmware file size or type: $name"
    }
    $before = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
    $signature = Get-AuthenticodeSignature -LiteralPath $path
    $after = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
    if ($before -ne $after) { throw "Firmware changed while being checked: $name" }
    if ($signature.Status -ne 'Valid' -or $signature.SignatureType -ne 'Catalog') {
        throw "No valid Windows catalog signature for $name ($($signature.Status), $($signature.SignatureType))."
    }
    [pscustomobject]@{ File = $name; Bytes = $file.Length; SHA256 = $after; Signature = 'Valid catalog' }
}
Write-Output 'Catalog verification does not prove chip compatibility; no firmware was loaded.'
