# MT7922 Windows diagnostic driver (not a radio driver)

This KMDF prototype reads the PCI identity and PnP resource descriptors for a
matching MT7922 device. It reports whether BAR0 could be matched without
exposing its address. When a unique BAR0 is at least one MiB, its first MiB
is mapped during device preparation and unmapped during hardware release;
an explicit ownership diagnostic reads and writes the fixed LPCTL register,
reads four DMA state bits while holding driver ownership, and attempts to
return ownership to firmware before completing. It is disabled
unless explicitly invoked and requires an administrator write handle. A failed
recovery blocks further ownership tests and reports device failure to Windows
without automatically restarting it.
The operation has only been tested against a simulated device, and the driver
must not be installed or run as a production Wi-Fi replacement. Another IOCTL can
allocate and immediately free six WDF common buffers holding three simulated
firmware-stage descriptor rings, seven event buffers and two transmit buffers
(28,800 bytes in total). It initializes the memory but never gives the device its address.
This memory layout is a diagnostic experiment and is not ready for hardware.
There is no arbitrary register interface, device DMA, firmware loading,
channel control, RX or TX. `radio_ready` is always zero.

`wfsys_reset.h`, `dma_stop.h`, `dma_program.h` and `dma_enable.h` contain
experimental MT7922 initialization steps tested against simulated registers.
They are **not called by this driver**, and have not been validated against
hardware. Firmware queues cannot be activated until the device-specific
runtime, ownership, timeout and powered-off teardown paths are connected.

The driver also queries Windows' reset interface and reports its declared
capabilities without invoking a reset. A successful query is not proof that a
reset works, stops DMA, or permits safe buffer release. A real radio driver
must handle hung devices and removal before it enables DMA. PCI configuration
headers are controlled by Windows and must not be modified as in a Linux port.

The source is experimental and has not been installed or tested on hardware.
The sample INF matches only the current `14c3:0616` / `105b:e0cd` revision 00
board. InfVerif `/w` and Inf2Cat's `/nocat` check pass locally; it has no
signed catalog and has not been installed. Binding any function driver to the
Wi-Fi card would temporarily remove the vendor network driver and interrupt
Wi-Fi; this prototype alone cannot provide a working wireless replacement.

From the repository root, prepare project-local development dependencies:

```powershell
.\scripts\setup-driver-tools.ps1
.\crates\radio\drivers\windows\.pixi\envs\default\python.exe .\scripts\fetch-wdk.py
.\crates\radio\drivers\windows\.pixi\envs\default\python.exe .\scripts\fetch-msvc-headers.py
.\scripts\build-diagnostic-driver.ps1
.\scripts\package-mt7922-diagnostic.ps1
.\crates\radio\drivers\windows\.pixi\envs\default\python.exe .\scripts\fetch-wdk-validation.py download
.\crates\radio\drivers\windows\.pixi\envs\default\python.exe .\scripts\fetch-wdk-validation.py extract
.\scripts\validate-mt7922-diagnostic.ps1
```

The result is an unsigned `.sys` under `.local/driver-build/` and an INF/SYS
pair under `.local/driver-package/`. No catalog is produced. These scripts do
not sign, stage, install or load the driver or change boot settings. The toolchain is
an experimental Clang/LLD build, not a Microsoft-validated WDK toolchain.

`diagnostic.c` follows the PCI-query/PnP structure of Airdrop-for-Windows'
MT7925 driver; it is covered by BSD-3-Clause-Clear. Preserve the notice in
`crates/radio/LICENSE-mt7925.txt`. The downloaded toolchain and SDK keep their
own licenses and remain outside the source repository under ignored folders.
