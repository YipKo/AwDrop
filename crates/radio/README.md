# 网卡后端实验接口

这里的 Rust crate 是 AwDrop 的无线后端契约与 winawdl radio-link v3 兼容编解码器，
默认不会访问或安装任何驱动。
**MT7922 的应用内实时收发、启动进程和界面接入尚未实现。** 识别到设备不等于设备已受支持。
独立硬件实验已在不替换 Windows 原厂驱动的情况下抓到无线帧，详见 `REFERENCES.md`。

## 目录与接入

- `src/lib.rs`：各网卡共用的 `RadioAdapter` 会话接口和 radio-link 消息。
- `src/registry.rs`：按 PCI/USB 型号匹配后端，记录独立设备实例，并将不同后端
  的会话统一为 `ErasedAdapter`，交给相同的 `Bridge` 处理。
- `src/inventory.rs`：Windows 使用 SetupAPI 列出当前存在的 PCI 实例；Linux 使用
  sysfs。它只识别型号，不改变网卡，也不验证驱动或无线能力。Linux 还未实机验证。
- `src/backends/mt7922/`：MT7922 特有的离线固件、MCU、DMA 和接收解析代码。
- `src/backends/mt7922/stock_rx.rs`、`stock_etl.rs`：原厂驱动 WPP 接收片段的
  Rust 组装和 ETL 离线回放；实时会话消费代码尚未实机验证，二者均不启停驱动。
- `src/backends/mt7922/stock_control.rs`：单次硬件实验所用启停请求的离线编码，
  不发送命令；`mt7922-stock` 以现有驱动方式注册，但明确报告不可用，直到实时
  会话、发送和恢复等功能完成并验证。
- `drivers/windows/mt7922/`：实验性 KMDF 诊断驱动源码；读取 PCI 身份和资源，
  按设备生命周期映射 BAR0；提供显式触发的 DMA 队列内存分配检查，以及会
  写入固定寄存器的所有权往返诊断。后者仅经过模拟测试；驱动不支持无线收发，
  尚未安装或实机验证。
- `src/diagnostic.rs`：该诊断驱动的用户态接口；仅显式调用才会访问已安装接口，
  不安装或加载驱动。详见驱动目录中的 README。
- `src/bridge.rs`、`src/windows_pipe.rs`：协议桥接和 Windows 管道传输，
  不含具体网卡实现。

增加网卡时，在 `src/backends/` 新建一个目录，实现 `AdapterBackend` 和
`RadioAdapter`，在 `registry::builtins()` 中注册。`probe` 只匹配身份和报告状态，
不得接管设备；实际打开设备需显式调用 `open`。`Device` 的 locator 用于区分
同型号的不同实卡，打开时必须重新核验身份；多个后端同时匹配时交给调用方选择，
不能按注册顺序悄悄选中。`Candidate` 仅表示可以尝试，不能保证打开成功。
后端还可声明是使用现有驱动还是需要独占设备；该声明不代表网卡具备监听、
注入或 ACK 能力。未来如果实现过滤驱动方式，仍必须逐项检查这些能力，不能
仅凭存在过滤器就报告可用。`probe_device` 支持对具体设备实例执行只读检查，
避免把一张卡的驱动能力套用到另一张同型号的卡上。
会话结束或打开失败时，后端必须释放自己持有的资源并恢复设备；接口本身不安装
驱动。每个会话实现必须提供显式、可重复调用的 `shutdown`，并返回恢复失败；
如果设备仍可能 DMA，不能因清理失败就释放内存。`mt7922_*` 旧模块名目前仍
作为兼容导出，新代码使用 `backends::mt7922`。

仓库里的 `winawdl/radio` 是原项目保留的 Realtek C++ 参考实现，尚未改接本接口；
它的存在不表示 AwDrop 的 Rust 后端已经支持该网卡。

`RadioAdapter` 描述信道、接收、发送、BSSID 过滤和本机地址；发送与接收的帧
带 radiotap，发送帧不带 FCS。`read_command`、`apply`、`write_hello` 和
`write_rx` 提供对 `radio/radio_link.h` v3 的转换。`Bridge` 负责每个客户端的
消息分片、边界校验、命令分发和 RX 消息编码。`windows_pipe::serve` 提供一个
单客户端实验性本地管道服务；模拟后端已与 AWDL 进程联调。它使用轮询和非阻塞
管道，真实设备场景下还需要验证并发、吞吐、背压、关闭和恢复，以及信道/带宽、
FCS、ACK 和过滤语义。现有 C++ 后端仍独立运行，并未接入此 Rust 服务。
`windows_pipe::serve_managed` 在服务成功或失败后都会显式调用 `shutdown`，
并保留服务错误和恢复错误；旧的 `serve` 在错误时只依赖对象的 Drop 清理。

`mt7922_rx` 是基于 mt76 Connac2 接收描述符的离线解析实验：仅提取普通、未加密、
非 A-MSDU 的原始帧，其余布局明确拒绝；缓冲区必须由驱动限制为实际 DMA 完成
长度。`radiotap_frame` 可将帧转换为上层接口所需格式，根据信号描述符和显式
指定的接收天线生成信号强度，并要求调用者明确提供 FCS 是否存在。它只经过合成
数据测试，FCS 是否存在和真实设备输出尚未确认，不能视为
可运行的无线接收器。该文件采用 BSD-3-Clause-Clear，许可与源码出处见
`LICENSE-mt7922-rx.txt` 和文件头。DMA 模型还保留了参考项目的
`LICENSE-mt7925.txt`。`mt7922_firmware` 对公开的 Connac2 固件容器
做边界检查，不加载或执行固件，也不验证固件来源的真实性。
`mt7922_mcu` 构造 Connac2 MCU 命令字节；`mt7922_dma` 只模拟 32 位描述符和队列
索引。两者都只使用内存，不会访问 PCIe，也不能替代 WDF DMA 分配、内存屏障或
硬件完成通知。这些固件、DMA 和直接设备传输代码仍只经过离线测试。
`backends::mt7922::tx` 按 Connac2 的 TXD/TXP 布局编码部分未加密的 802.11
帧元数据，并将 radio-link 发送帧中的受支持 radiotap 字段接入编码器；它尚未
接入硬件队列，也不能验证实际发送或 ACK。
`download_plan` 可按 mt76 的 PCIe 流程生成补丁和 RAM 固件的离线步骤，处理
分块、标志和启动地址。`download` 对抽象传输执行这些步骤，补丁下载失败时
会尝试释放信号量并保留失败信息，并在后续控制命令前等待固件队列完成。
`firmware_mcu` 封装 Connac2 下载命令和回复匹配；它依赖尚未实现的设备传输，
发生交换、回复或队列排空错误后禁止继续发送，以避免序号回卷后把迟到回复
误认为新命令的回复；真实传输仍需实现 DMA、事件队列、超时、所有权和恢复。
`pcie_transport` 提供串行命令/事件路由、限时等待和有界异步事件队列，
但设备队列接口目前只有模拟实现，尚未连接 Windows 驱动。
`monitor` 按 mt7921 协议构造监听模式启停与信道配置命令，并将部分失败标记
为需要恢复；只有模拟传输，成功收到回复不证明无线功能可用。真实启用前还需
实现设备初始化、信道监管与功率限制、接收过滤、BSS 和硬件恢复。
`boot` 将 MCU 重启、电源状态检查、补丁及 RAM 固件下载和就绪检查串联；
完整流程只在模拟队列上运行，不能证明固件已加载到网卡。
`registers` 只计算寄存器映射位置和范围，不读取或写入硬件。
本地曾用参考 mt76 仓库中的 MT7922 固件检查容器和步骤生成，固件未写入设备。
Windows 上可对用户显式选择的固件目录运行 `scripts/check-mt7922-firmware.ps1`，
检查 Windows 目录签名和文件哈希；这是独立的检查，尚未接入硬件加载流程，
签名有效也不保证固件适合当前硬件。项目不分发原厂固件。

## 硬件与移植边界

Linux mt76 的 mt7921 PCI 表将 `14c3:0616`
和 `14c3:7922` 映射到 MT7922 固件。参考项目的 MT7925 后端针对另一芯片的
寄存器、固件、DMA 和驱动初始化，不能通过修改设备 ID 当成 MT7922 驱动。

桌面预览只读查看注册表中识别到的已知 PCI 设备。注册表也可能保留已移除设备，
输出不证明设备在线。
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

在已准备好 AwDrop 项目内 Windows 开发环境后，从仓库根目录执行：

```powershell
.\scripts\dev-windows.ps1 radio-test
```

环境准备和缓存位置见 `desktop/README.md`。测试只验证接口和离线模型，
不能证明 MT7922 可以实际收发。

`examples/wlan_device_services.rs` 是 Windows 上只读枚举厂商设备服务的
诊断程序；它不发送厂商命令或修改网卡设置。权限不足时会明确报告查询失败，
不能把失败解释成设备没有服务。相关调查和边界见 `REFERENCES.md`。

项目内驱动工具链准备、未签名诊断驱动构建和限制见
[`drivers/windows/mt7922/README.md`](drivers/windows/mt7922/README.md)。
