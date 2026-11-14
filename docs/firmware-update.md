# Creator Micro 2 固件更新契约

范围：从 Work Louder 官方发布源获取固件，通过 USB 串口 bootloader 更新 Creator Micro 2。以下分开记录 Input 1.1.0 静态观察、官方资料、独立实现约束与未验事项；不代表已刷写真实设备。普通 HID 配置 RPC 不因此开放任意写入。

## 发布与更新流程

官方来源是 [cm-v2-fw-releases](https://github.com/worklouder/cm-v2-fw-releases/releases)，API 为 `https://api.github.com/repos/worklouder/cm-v2-fw-releases/releases`。稳定通道默认排除预发布；预发布应由用户明确选择。页面存在较新 RC 不等于稳定版过期。

Input 的观察链为：查询 release → 下载 `.bin` 到临时目录 → 请求 `sys.bootloader` → 发现 USB 串口 → 使用 ESP loader 写入 → 重置 → 显示完成。下载、进入 bootloader、刷写分别有失败状态；失败后用户可重试，没有观察到自动恢复旧固件。实现应把写入完成与重连后版本确认分开。

独立实现使用官方发布资产身份、下载长度和 SHA-256（GitHub 返回 digest 时）核对下载；正式进入写入前确认当前选择的串口、固件版本和文件。更新检查不连接或写设备。检查失败不能显示“已是最新”。固件写入通过 Rust 后端完成，Swift 只负责呈现、选择和转发命令。

## Input 1.1.0 的静态证据

文件缩写与身份沿用 [Input 架构证据表](input-architecture.md#证据身份与可复核位置)。安装包 SHA-256 为 `a6c7185d5c3d8dda1262437a9c0740080cceaea5585dd289b8eef0668b8a4206`。

| 定位 | 观察 |
| --- | --- |
| K:13905–13954、13992–14006 | 获取 releases，按 semver prerelease 筛选稳定/预发布，取首条及首个 `.bin` 资产 |
| K:14152–14179 | 稳定 Input 先检查自身是否最新，再提示固件；这是厂商应用版本策略，不能套用 Micro Input 的版本号 |
| K:14251–14265 | CreatorMicroV2 与 CodexMicro 映射 `cm-v2-fw-releases` |
| M:3815–3816、4493–4505 | Electron 下载到系统 temp；这层未见独立 SHA-256 检查 |
| K:4608–4637 | HID 使用 `sys.bootloader`，旧串口使用 legacy `bootloader` 请求 |
| R UTF-8 字节169074–171100 | 进入 bootloader 后等待与重新扫描；异常分支可能重连，不可把计数变量理解为严格重试上限 |
| K:5780–5815 | macOS 串口候选以 VID `0x303A` 和 manufacturer `Espressif` 识别；不是 Creator Micro 2 型号证明 |
| U UTF-8 字节12283–14300 | 自动更新取 bootloader 列表首项，完成后删除下载文件并显示完成；本段没有重连版本读回 |
| R UTF-8 字节6980400–6985500 | 高级页支持本地文件、手动重试，并提示多个 bootloader |
| K:13608–13685 | 单个 merged 文件写入地址 `0`；baud `921600`；mode/freq/size=`keep`，eraseAll=`false`，compress=`true`；MD5 与进度回调 |
| K:13730–13771 | 写后寄存器 `0x6000812c` 置零，配合 DTR/RTS 重置，最后断开；连接/写入/reset 失败返回失败 |

REA 定向分析原始 Evidence：`ev_454c0751f8555478d99f149372ad4c2a2e0b26b680a81ed33488178cec5789f0`。完整记录在受忽略的 `.scratch/input-study/firmware-update/rea-evidence.json`。只分析主进程、SDK、preload；语义图仍有 partial/动态关系缺口。29 个 unpacked native 文件与包内 integrity 声明不一致，工具按 `record-and-continue` 保存为 observed-untrusted；这些原生文件没有执行。上述具体链路由逐段读取补证，不把工具图当作运行验证。

## 固件镜像与配置保留边界

2026-11-14 查询官方 [v0.6.3 release API](https://api.github.com/repos/worklouder/cm-v2-fw-releases/releases/tags/v0.6.3)，资产 `firmware_0.6.3_merged.bin` 为 2,214,944 字节，digest 为 `sha256:1d8bd545b9047182d7112dd86b14f6c636bad3834725ee0a6eca7cacee0ec6a7`。本机既有镜像与该长度和摘要一致，原件不提交。

镜像头 offset 12 的小端 chip ID 为 `9`，与 [Espressif 对 ESP32-S3 正式芯片的说明](https://docs.espressif.com/projects/esp-faq/en/latest/software-framework/system.html) 一致。这证明此资产面向 ESP32-S3，不证明任意已枚举 ESP32-S3 都是 Creator Micro 2。

在镜像 offset `0x8000` 解析到如下分区表：

| 名称 | type/subtype | 起始地址 | 长度 |
| --- | --- | --- | --- |
| phy_init | 1/1 | `0xF000` | `0x1000` |
| factory | 0/0 | `0x10000` | `0x800000` |
| nvs | 1/2 | `0x810000` | `0x20000` |
| fs | 1/130 | `0x830000` | `0x200000` |
| coredump | 1/3 | `0xA30000` | `0x10000` |

该 merged 文件写入区间为 `[0, 2214944)`，不覆盖表内 nvs/fs。由此只能推断按该范围、非整片擦除刷入 0.6.3 时不直接覆盖这两个区域；固件启动迁移、文件系统挂载恢复及键位实际保留仍未验证。新版本必须重新解析，不能把 0.6.3 的分区地址当所有固件固定布局。`eraseAll=false` 单独不足以承诺保留配置。

## Bootloader 操作与设备身份

[Work Louder 官方支持说明](https://feedback.worklouder.cc/p/creator-micro-v2-knob1-battery-charging-issue-megathread) 给出的路径是 USB 连接后在 Input 的 Setup / Advanced Flash 中点击 Bootloader，再选择固件和刷写，并要求更新期间保持连接。该页面针对诊断版本，不将其诊断固件作为本项目默认下载。

[Creator Micro 2 官方设置页](https://www.worklouder.cc/setup/creator-micro-2) 说明机内上方单圆点按钮为 Reset，下方双圆点为 Boot；其公开操作步骤只说明按 Reset 软复位，没有给出该产品完整的手动进入 bootloader 组合。Input 当前高级页提供软件 Activate Bootloading，也没有实体按钮组合说明。因此 UI 不应虚构“按住某外部键插线”的步骤，更不能套用旧 Creator Micro 的 QMK 操作。

[Espressif ESP32-S3 文档](https://docs.espressif.com/projects/esptool/en/latest/esp32s3/advanced-topics/boot-mode-selection.html) 说明 GPIO0 拉低时复位进入下载模式，开发板通常按住 Boot 再按 EN。该通用芯片说明不是 Creator Micro 2 电路和按键时序的实机核验。

原 SDK 还有 eFuse 身份读取函数（K:13706 起），但本轮未发现前端调用，也未取得产品码对照。独立实现不能仅凭 VID、Espressif manufacturer 或芯片类型静默选首项刷写；应显示串口候选并要求明确选择设备，保持其他设备不受影响。

## 验证边界

本轮研究没有启动 Input、打开串口、发送 bootloader 或刷写命令。离线验证可覆盖 release 选择、下载校验、镜像布局、状态转换和失败结果；不能替代以下实机事项：实际 bootloader 关联、刷写与重启、版本读回、原键位及宿主行为保留、失败后的恢复。新版固件支持的配置协议仍须单独核验，不因能刷写就放开现有配置读写门禁。
