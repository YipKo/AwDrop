# 网卡后端实验接口

这里的 Rust crate 是独立的无线后端契约与 winawdl radio-link v3 编解码器，
没有访问或安装任何驱动。原版 WPF 界面、Realtek 无线进程和 AWDL 实现保持原样。
**MT7922 的真实收发、启动进程、驱动和界面接入尚未实现。** 识别到设备不等于设备已受支持。

`RadioAdapter` 描述信道、接收、发送、BSSID 过滤和本机地址；发送与接收的帧
带 radiotap，发送帧不带 FCS。`read_command`、`apply`、`write_hello` 和
`write_rx` 提供对 `radio/radio_link.h` v3 的转换。`Bridge` 负责每个客户端的
消息分片、边界校验、命令分发和 RX 消息编码。`windows_pipe::serve` 提供一个
单客户端实验性本地管道服务；模拟后端已与 AWDL 进程联调。它使用轮询和非阻塞
管道，真实设备场景下还需要验证并发、吞吐、背压、关闭和恢复，以及信道/带宽、
FCS、ACK 和过滤语义。现有 C++ 后端仍独立运行，并未接入此 Rust 服务。

`mt7922_rx` 是基于 mt76 Connac2 接收描述符的离线解析实验：仅提取普通、未加密、
非 A-MSDU 的原始帧，其余布局明确拒绝；缓冲区必须由驱动限制为实际 DMA 完成
长度。它只经过合成数据测试，FCS 是否存在和真实设备输出尚未确认，不能视为
可运行的无线接收器。该文件采用 BSD-3-Clause-Clear，许可与源码出处见
`LICENSE-mt7922-rx.txt` 和文件头。DMA 模型还保留了参考项目的
`LICENSE-mt7925.txt`。`mt7922_firmware` 对公开的 Connac2 固件容器
做边界检查，不加载或执行固件，也不验证固件来源的真实性。
`mt7922_mcu` 构造 Connac2 MCU 命令字节；`mt7922_dma` 只模拟 32 位描述符和队列
索引。两者都只使用内存，不会访问 PCIe，也不能替代 WDF DMA 分配、内存屏障或
硬件完成通知。MT7922 专用代码目前只通过离线测试，未经过实机验证。

## 硬件与移植边界

Linux mt76 的 mt7921 PCI 表将 `14c3:0616`
和 `14c3:7922` 映射到 MT7922 固件。参考项目的 MT7925 后端针对另一芯片的
寄存器、固件、DMA 和驱动初始化，不能通过修改设备 ID 当成 MT7922 驱动。

可运行 `.\scripts\inspect-adapters.ps1` 只读查看注册表中识别到的已知 PCI
设备和目前的后端状态。注册表也可能保留已移除设备，输出不证明设备在线。
MT7922 的 Linux 固件文件名是 `WIFI_MT7922_patch_mcu_1_1_hdr.bin` 和
`WIFI_RAM_CODE_MT7922_1.bin`；固件不包含在本仓库中。

后续需要以 mt76 的 `mt7921` / `mt792x` 实现为依据，分别完成 PCIe 资源和
DMA 生命周期、固件加载、校准和监管限制、信道配置、原始帧 RX/TX、ACK，
以及崩溃后的设备释放和恢复；再用真实 iPhone 验证发现和双向传输。涉及内核
驱动签名、系统驱动绑定及网卡占用，不能由当前的用户态接口代替。

参考：

- [winawdl](https://github.com/raph559/winawdl)；本仓库已有 WPF 和 AWDL 协议。
- [Airdrop-for-Windows](https://github.com/Dy1an-Zou/Airdrop-for-Windows)；MT7925 专用实现。
- [Linux mt7921 PCI 表](https://github.com/torvalds/linux/blob/master/drivers/net/wireless/mediatek/mt76/mt7921/pci.c)。
- [OWL](https://github.com/seemoo-lab/owl) 和 [omdrop](https://github.com/brentkearney/omdrop-plugin)；协议和 AirDrop 参考。

上游包含不同许可的组件，移植或分发时须保留相应的源码和许可；Rust crate 中
接口和桥接代码为 MIT，MT7922 描述符、MCU 和固件容器代码为 BSD-3-Clause-Clear。

## 本地构建

在已存在 Pixi 的 Windows 环境中，从仓库根目录执行：

```powershell
.\scripts\adapter-pixi.ps1 test
```

脚本将 Pixi 环境放在 `.pixi`，缓存、Cargo home 和临时目录放在 `.local`，
构建产物放在 `build/adapter`；不会全局安装 Rust，也不需要管理员权限。可用
`.\scripts\adapter-pixi.ps1 info` 查看实际缓存路径。项目的 Pixi 环境包含 GNU
链接器。该命令只测试此 crate，不能构建原版完整
应用；原版的 .NET、C/C++、WDK、Python 等要求仍见根目录 README。

`.\scripts\adapter-pixi.ps1 awdl` 使用项目内 CMake、Ninja 和 MinGW 构建上游
AWDL 进程；仍需 Wintun 和可用的无线后端，才能完成真实通信。
`.\scripts\adapter-pixi.ps1 mock` 后运行 `.\scripts\test-mock-radio.ps1` 可做
不接触网卡的模拟管道联调。此测试使用固定的 winawdl 管道名，并会拒绝已经存在
的管道；不应与实际无线服务同时运行。
