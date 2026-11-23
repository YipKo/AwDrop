# Read-only inventory of known PCI identities. Registry entries can include
# devices that are not currently present; this is not a readiness check.
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$known = @{
    'VEN_14C3&DEV_0616' = @('MediaTek MT7922 / RZ616', 'not implemented')
    'VEN_14C3&DEV_7922' = @('MediaTek MT7922', 'not implemented')
    'VEN_14C3&DEV_7925' = @('MediaTek MT7925', 'reference only; not integrated')
    'VEN_10EC&DEV_C852' = @('Realtek RTL8852CE', 'upstream backend; requires its driver')
}
$pci = 'HKLM:\SYSTEM\CurrentControlSet\Enum\PCI'
foreach ($device in Get-ChildItem -LiteralPath $pci) {
    if ($device.PSChildName -notmatch '^(VEN_[0-9A-F]{4}&DEV_[0-9A-F]{4})(?:&|$)') { continue }
    $id = $Matches[1].ToUpperInvariant()
    if (-not $known.ContainsKey($id)) { continue }
    foreach ($instance in Get-ChildItem -LiteralPath $device.PSPath) {
        $values = Get-ItemProperty -LiteralPath $instance.PSPath
        $description = [string]$values.DeviceDesc
        if ($description.Contains(';')) { $description = $description.Substring($description.LastIndexOf(';') + 1) }
        [PSCustomObject]@{
            Identity = "PCI\$id"
            Model = $known[$id][0]
            Description = $description
            InstanceId = "PCI\$($device.PSChildName)\$($instance.PSChildName)"
            DriverService = $values.Service
            Backend = $known[$id][1]
            Presence = 'not checked (registry inventory)'
        }
    }
}
