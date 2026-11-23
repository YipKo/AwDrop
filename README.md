# AwDrop

<img src="winawdl/docs/logo.svg" width="112" align="right" alt="AwDrop logo, two orange cards with radio waves and a white transfer arrow">

AwDrop builds on [winawdl](https://github.com/raph559/winawdl) and uses WinUI 3 on Windows. In the desktop preview, use **Settings** to change the language and **Choose files** to queue a file. Device discovery and transfers are unavailable.

## Install

### Manual download

Choose an asset for your OS and architecture from [AwDrop releases](https://github.com/YipKo/AwDrop/releases). The expected Windows x64 filename is `AwDrop_<version>_x64-setup.exe`. Compare the published SHA-256 with `Get-FileHash <filename> -Algorithm SHA256` in PowerShell. No public AwDrop asset has been confirmed because the release page could not be verified.

The AwDrop preview can queue files, but its radio is not connected and discovery and transfers do not work. Linux Debian/Ubuntu packaging is planned.

[winawdl 0.4.0](https://github.com/raph559/winawdl/releases/tag/v0.4.0) is a separate upstream Windows 11 x64 pre-release for RTL8852CE (`10ec:c852`), with a setup EXE and ZIP. Read its [setup notes](winawdl/README.md#before-you-run-it); Windows Wi-Fi loses access to the card while winawdl Radio is on.

### Install with an agent

```text
Detect my OS and architecture. Check https://github.com/YipKo/AwDrop/releases for an official AwDrop asset that matches both; do not use winawdl or take over a Wi-Fi adapter. Download only the matching official release asset and its published SHA-256, verify the hash, and report the publisher signature status. Stop if there is no matching asset or published hash, or if verification fails. Install the verified asset, launch AwDrop, and report the installed version and any checks you could not perform.
```

## Wi-Fi cards

PHY rates describe the Wi-Fi hardware. File speeds come from a winawdl test with one Windows 11 PC and one iPhone.

| Chip | App or backend | AirDrop send | AirDrop receive | Peak Wi-Fi PHY (160 MHz) | Measured file speeds |
|---|---|---|---|---|---|
| RTL8852CE (`10ec:c852`) | winawdl | Tested | Tested | [2,402 Mbps (Wi-Fi 6E)](https://device.report/m/cd9902780ee0f69db5768f74ce598a8cd9bc309d77fbbdaaf4dd0075d58cbfbd) | [9–14 MB/s send, ~16 MB/s receive](winawdl/README.md#where-it-stands) |
| MT7922 | AwDrop, offline radio tests | Unavailable | Unavailable | [2.4 Gbps (RZ616, Wi-Fi 6E)](https://www.mediatek.com/press-room/amd-and-mediatek-develop-amd-rz600-series-wi-fi-6e-modules-to-enhance-laptop-and-desktop-pc-connectivity-experiences) | — |
| MT7925 | AwDrop, detection only | Unavailable | Unavailable | [2.8 Gbps (AW-EB600NF-NV, Wi-Fi 7)](https://www.azurewave.com/img/nvidia/AW-EB600NF-NV_DS_DF_B_Nvidia.pdf) | — |

The desktop preview can list known cards, including RTL8852CE. See [radio backend status](crates/radio/README.md) for hardware and frame-test details.

[Add or test an adapter](CONTRIBUTING.md).

## Build

```text
In this checkout, inspect the project instructions, Cargo.lock, pixi.toml, any Pixi lockfile, and submodules. Initialize the required submodules, including owl's radiotap submodule. On Windows x64, check for the .NET 10 SDK, Pixi, the project-local GNU Rust toolchain, and Inno Setup 6 or 7. Prepare missing prerequisites using the repository scripts and official sources. Run scripts/setup-windows.ps1 if the project toolchain needs setup, then scripts/build-windows-installer.ps1 (pass -Iscc if needed). Use locked Cargo dependencies, do not assume a Pixi lockfile exists, and report any dependencies that are not locked. Report the installer path, its SHA-256, signature status, and checks not run. Do not install drivers or publish anything.
```

See [LICENSE](LICENSE) and the component licenses before distributing source or binaries. AwDrop is independent of Apple.
