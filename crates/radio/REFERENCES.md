# AWDL implementations and MT7922

Reviewed 2026-11-24. The links are technical references, not a claim that
AwDrop already contains or has validated these backends.

| Project | Role | Hardware boundary |
| --- | --- | --- |
| [winawdl](https://github.com/raph559/winawdl#how-it-fits-together) | Windows radio service → OWL-derived AWDL process → Wintun `awdl0` → AirDrop engine | The verified radio is RTL8852CE. Its UMDF register proxy takes the card from the vendor driver; devourer drives that particular chip in user space. |
| [Airdrop-for-Windows](https://github.com/Dy1an-Zou/Airdrop-for-Windows/blob/main/README.en.md) | KMDF driver, radio process, AWDL process, Wintun and file transfer | MT7925 PCIe, exact tested device. It does not claim MT7922 support. |
| [OWL](https://github.com/seemoo-lab/owl#requirements) | Implements AWDL, creates a virtual interface, uses libpcap and Linux netlink | Needs monitor reception and frame injection; efficient delivery also needs ACK. It is not a hardware driver. |
| [Omdrop](https://github.com/netmojo-systems/omdrop-plugin#requirements) | AirDrop frontend/receiver over a selected backend providing `awdl0` | Supports distinct Broadcom firmware, Broadcom software and Owly backends; backend is a separate requirement. |
| [mt76](https://github.com/openwrt/mt76/blob/master/mt7921/pci.c) | Linux MediaTek driver reference | The `mt7921` PCI table maps both `14c3:0616` and `14c3:7922` to MT7922 firmware. Linux kernel code is not a Windows driver. |
| [airdrop-mt7921](https://github.com/jedbillyb/airdrop-mt7921#status) | Linux mt76 + modified OWL and OpenDrop | Reports iPhone send/receive on MT7921 and MT7922, with an MT7922 send tested on Arch. This is not a Windows result or proof for every firmware/kernel. |

## Relevant MT7922 observations

The Linux project reports using one normal monitor interface to tune the
channel, plus an active monitor interface for ACK. It also reports that power
management can stop monitor reception and that the requested channel should
be checked against radiotap, not assumed from `iw` output. Its default
exclusive mode temporarily disconnects ordinary Wi-Fi. Its shared mode has
channel and configuration restrictions; it is not evidence that simultaneous
Wi-Fi and AirDrop works unconditionally. See [its design and caveats](https://github.com/jedbillyb/airdrop-mt7921#how-it-works).

On the current Windows machine, the original MT7922 driver reported network
monitor mode and promiscuous mode as unsupported. Its card ID is `14c3:0616`.
The successful Apple BLE advertisement test is a separate layer. On 2026-11-24,
a short experiment on this card with the original MediaTek driver used a
private vendor interface and a temporary WPP trace while ordinary Wi-Fi was
disconnected. Offline reconstruction yielded 3,070 802.11 records at 5220 MHz;
five had AWDL-compatible action headers. This does not establish live RX in the
application, frame injection, ACK, an AWDL link or an iPhone transfer. A
reconnect request was followed by Windows reporting Wi-Fi connected. These
observations apply only to the tested driver version and hardware instance.

One Windows fallback is to port MT7922 initialization, DMA, firmware and radio
operations from the Connac2/mt7921 sources behind a suitable Windows driver
and validate them on hardware. An existing-driver extension remains an open
investigation and could avoid replacement if it proves all required abilities.
The MT7925 driver checks a
different PCI ID and its chipset-specific implementation must not be rebound
to this card. After reliable raw RX/TX, ACK and channel handling exist, adapt
the common radio-link bridge, AWDL/Wintun, and AirDrop engine; verify discovery,
then send and receive with the target iPhone. Keep each component's existing
license and notices when code is reused.

## Windows DMA and recovery constraints

Linux's `pci_set_master` and config-header writes must not be copied into the
Windows driver: Windows reserves control of the PCI configuration header to
the operating system. Use WDF DMA interfaces and supported bus interfaces;
see [Microsoft's PCI configuration-space documentation](https://learn.microsoft.com/en-us/windows-hardware/drivers/pci/accessing-pci-device-configuration-space).
The diagnostic prototype queries the
[device reset interface](https://learn.microsoft.com/en-us/windows-hardware/drivers/kernel/working-with-guid-device-reset-interface-standard)
without issuing a reset. Interface availability is not evidence that the
reset succeeds or that a hung device has stopped DMA. Before enabling device
DMA, the real implementation must prove its stop, reset, surprise-removal,
and buffer-lifetime handling; the diagnostic's simulated queue does not do so.

## Existing-driver/filter approach

Windows [NDIS filter drivers](https://learn.microsoft.com/en-us/windows-hardware/drivers/network/ndis-filter-drivers)
can coexist with an existing miniport, but do not themselves create radio
capabilities that the miniport does not expose. For example, [Npcap's raw
802.11 option](https://npcap.com/guide/npcap-users-guide.html) adds a filter
and relies on the adapter and driver supporting monitor mode. On this machine,
the installed MediaTek driver reported monitor mode unsupported. That is not
proof that every possible vendor-specific interface is impossible. A separate
private vendor path produced offline-reconstructed RX records in the bounded
experiment above; TX, channel switching, ACK, live streaming and reliable
recovery remain unverified. A shared backend must check each capability on the
actual device, rather than treating a filter or normal Ethernet capture as success.

The existing driver also reports Action Frame and Wi-Fi Direct support.
[WDI defines a request carrying an action-frame body](https://learn.microsoft.com/en-us/windows-hardware/drivers/network/oid-wdi-task-send-request-action-frame),
and [an indication for received action frames](https://learn.microsoft.com/en-us/windows-hardware/drivers/network/ndis-status-wdi-indication-action-frame-received).
These are driver-stack interfaces, not evidence of a usable user-mode API or
that this miniport accepts arbitrary AWDL vendor-specific frames. AWDL also
uses data frames, channel synchronization and acknowledgements; none of those
requirements has been demonstrated through this path. Investigate separately;
do not advertise shared-driver support until the complete path is verified.

### Vendor extension investigation (read-only)

The local original `mtkwl6ex.inf` maps this machine's `14c3:0616` /
`105b:e0cd` board to `AMD7922_MODE6.ndi`, whose installation section includes
`IHV.reg` and `IHV.CopyFiles`. The INF names `mtkihvx.dll` as its extension;
the copied signed DLL exports the standard `Dot11ExtIhvGetVersionInfo` and
`Dot11ExtIhvInitService` entry points. This identifies a vendor extension,
not its proprietary operations or an AWDL-capable API. No DLL or driver was
modified or loaded during the inspection.

Windows documents a [vendor-specific IHV communication channel](https://learn.microsoft.com/en-us/windows-hardware/drivers/network/802-11-wlan-adapter-communication-channel)
and [WlanIhvControl](https://learn.microsoft.com/en-us/windows/win32/api/wlanapi/nf-wlanapi-wlanihvcontrol).
The vendor determines the request format and capabilities. The existence of
either channel does not establish raw frame receive/transmit, channel scheduling,
or ACK support. Do not send speculative vendor commands to the active device.
Likewise, changing the vendor DLL's registration is a system change, not a
portable application-only plugin.

The project-local `examples/wlan_device_services.rs` uses only WLAN interface
enumeration and `WlanGetSupportedDeviceServices`; it sends no device service
commands. On this machine, an unsandboxed non-elevated run found one MT7922
interface, but the service query returned `ERROR_ACCESS_DENIED` (5). This is
an unknown result, not an empty list or proof of absence. Even a successful
list of service GUIDs would not by itself identify what their commands do.
The user subsequently ran the same read-only program in an administrator
terminal: the interface was enumerated, but its service query returned
`ERROR_NO_DATA` (232, "The pipe is being closed"). The cause is unconfirmed;
this is also not an empty list or a capability result. No device service
command was sent. Raw output is kept locally under `.local/`.

A separate [MT7921 filter experiment](https://github.com/Obsidian-Strike/mt7921-windows-wifi-monitor-mode-research)
reports that its miniport rejected a monitor OID and an attempted vendor
extension request. That is relevant prior art, not a result for this MT7922,
driver version, every possible request or every future implementation.
