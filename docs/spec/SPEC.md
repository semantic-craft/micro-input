# SPEC — Micro Input 兼容／替代边界（clean-room）

- 版本：0.1（2026-11-02，Asia/Shanghai）。参照对象：Work Louder Input 1.1.0（Electron）+ Creator Micro 2 固件 0.6.3。
- 证据：每条规则引用 [EVIDENCE.md](EVIDENCE.md) 的 `E-xxx`。级别 **SEEN／INFERRED／UNKNOWN**；裁决 **必须兼容／可替代／丢弃／待确认**。
- clean-room：本文只用自己的话描述行为，引用符号名、通道名、文件名和版本号作证据锚点；不含原厂代码、资源或文案。未提取的报文细节一律 UNKNOWN，不得在实现中猜。

## 1. 目的与非目标

**目的**：给自研 macOS 应用（Rust 设备/核心 + SwiftUI 配置器 + 常驻宿主）圈定要替代的能力面：①设备发现与通信；②设备配置的读取（之后才是写入）；③设备触发、必须由宿主执行的动作（Smart Action、径向菜单、Cheat Sheet、前台应用上报）。

**非目标**：
- 不做输入法（IME）、内核扩展或 HID 驱动替代；原产品本身也不是 IME（E-003）。
- 不刷固件、不替换固件；不复刻 Nomad/Knob/XYZ 的屏幕、壁纸、媒体卡片功能（E-025）。
- 不兼容 Windows/Linux；不使用 .NET 桥（E-040）。
- 不复刻厂商分析统计、反馈、远程 API 客户端（E-021、E-029）。
- MVP 阶段不写设备、不激活 Profile（E-043）。

## 2. 原系统架构摘要

| 层 | 原系统（证据） | 说明 |
|---|---|---|
| 进程 | Electron 薄壳 → main（单包）+ 3 个 renderer 窗口 + 1 个 worker_thread（E-003、E-005） | 主窗、radial menu、cheat sheet 三个窗口各有独立 preload |
| main 服务 | application、storage(Loki)、update、window、tray、devices_comm_manager、device_file、device_keymap、device_notify、focus_app、native(AppleScript)、diagnostic、image(LVGL)、media_*、mock_*（E-021） | 以单例服务容器初始化后再建 IPC 通道（E-022） |
| IPC | `ipcMain.handle/on` + preload 暴露的通道对象：common、devicesManager、connectedDevice、rpc、localStorage、fs、image、update、radialMenu、cheatSheet（E-022、E-023） | renderer 可经 rpc／connectedDevice 通道直接发任意 JSON-RPC（`sendJsonRpcCall`） |
| 设备 kit | `@worklouder/wl-device-kit@0.2.13`：Discovery、DeviceComm、RPCClient/RPCApi、Programmer、Permissions、Release、LVGL（E-007、E-009） | 依赖 node-hid、serialport、esptool-js |
| 原生件 | kit `device_discovery.node`（IOKit HID 通知）、`permissions.node`（输入监控预检）（E-008）；node-hid；electron-edge-js（未被引用，E-040） | 无 CoreBluetooth 用法（E-008） |
| 宿主脚本 | `osascript` 跑 3 个 AppleScript：粘贴、前台应用、媒体（E-027） | |
| 持久化 | `userData/input_storage.json`（LokiJS：app_settings、devices、presets、migrations）+ `user-id.json`（E-028）；设备侧 `keymap.json`、`smart_actions.json`（E-030） | |

## 3. 能力清单（规则表）

### 3.1 架构（A）

| ID | 规则 | 证据 | 级别 | 裁决 |
|---|---|---|---|---|
| R-A01 | 原系统是 Electron 桌面配置器 + 常驻宿主，非 IME，业务逻辑在 JS 与 kit。新系统不需要与 Electron 结构兼容，只需在能力上替代。 | E-003, E-004, E-005 | SEEN | 可替代 |
| R-A02 | main 承担全部设备会话、存储、宿主动作；UI 只经 IPC 访问。新系统同样让 UI 不直接碰设备，统一经 Rust 核心。 | E-020, E-021, E-047 | SEEN | 必须兼容（边界语义） |
| R-A03 | 三个窗口（主窗／radial／cheat sheet）各自只拿到所需通道。新系统的浮层窗口只订阅与自己相关的事件。 | E-005, E-022, E-023 | SEEN | 可替代 |
| R-A04 | 设备层是独立库，分为发现、通信、RPC API、编程器、权限、发布六块。新系统的 Rust crate 按同样关注点拆分。 | E-007, E-009 | SEEN | 可替代 |
| R-A05 | renderer 能发任意 JSON-RPC（通用 sendJsonRpcCall）。新系统**不**开放通用透传：只暴露白名单方法。 | E-022 | SEEN | 丢弃（有意收紧） |
| R-A06 | electron-edge-js 随包但没有任何 JS 引用，mac 路径不依赖 .NET。 | E-040 | SEEN（未引用）／UNKNOWN（运行时动态加载） | 丢弃（待确认见 §9） |

### 3.2 宿主边界（H）

| ID | 规则 | 证据 | 级别 | 裁决 |
|---|---|---|---|---|
| R-H01 | 原 App 为硬化运行时、非沙盒，最低 macOS 12。新宿主同样非沙盒（需执行命令、开应用、IOKit HID）。 | E-001, E-002 | SEEN | 必须兼容（非沙盒）；最低版本可替代 |
| R-H02 | 启动时在 macOS 上检查**输入监控**（Listen Event）权限，不满足则进入引导页，可打开对应系统设置面板并重新检查；HID 访问被认为依赖此权限。 | E-008, E-018, E-037 | SEEN | 必须兼容 |
| R-H03 | 蓝牙用途说明存在于 Info.plist，但 app 代码未用 CoreBluetooth；说明文字很可能是 Electron 框架链 CoreBluetooth 的副产物。新系统不申请蓝牙权限，除非将来真用 CoreBluetooth。 | E-001, E-048, E-008 | INFERRED | 丢弃（待确认） |
| R-H04 | 前台应用上报：有带 focusedApp 特性的设备（Micro 2 属于）连接时，约每秒取一次前台应用，变化时用 `host.focused_app` 发给设备；设备断开即停。AppSense"链接应用"UI 采集前台停留 5 秒。新系统用 NSWorkspace 激活通知做事件驱动替代轮询，语义（变化才发）保持。 | E-025, E-026, E-027, E-046 | SEEN | 必须兼容（行为）；实现可替代 |
| R-H05 | 文本插入（`kb.sa.inserttext`）：宿主先保存剪贴板、写入文本、模拟 ⌘V、再恢复剪贴板。新系统可用 CGEvent 发键或同样的剪贴板方案，但必须保留"执行后恢复剪贴板"。 | E-024, E-027 | SEEN | 必须兼容（结果）；实现可替代 |
| R-H06 | 打开应用（`kb.sa.openapp`，参数为路径）与打开 URL（`kb.sa.openurl`）。参数缺失或类型错误时静默丢弃并记日志。 | E-024 | SEEN | 必须兼容 |
| R-H07 | 粘贴依赖 System Events 发键，理论上需"自动化"与"辅助功能"权限；原 App 未声明 AppleEvents 用途说明、未检查这两项。新系统应显式检查并引导辅助功能（若用 CGEvent 发键）。 | E-001, E-002, E-027, E-037 | INFERRED | 待确认 |
| R-H08 | 径向菜单：设备发 `kb.radial` 通知，宿主在当前显示器铺一个透明、不抢焦点、穿透鼠标、全屏空间可见的浮层，并接收摇杆移动事件更新选择。仅 Micro 2／Codex Micro 注册。 | E-024, E-025, E-032 | SEEN | 必须兼容 |
| R-H09 | 命令执行（`kb.sa.exec`）只有在设置 `smartActionCmdEnabled`（默认关）为真时才执行，否则丢弃。新系统保持默认关、显式授权。 | E-024, E-029, E-045 | SEEN | 必须兼容 |
| R-H10 | Cheat Sheet：设备发 `kb.cs.show/hide/toggle`，宿主显示一个透明不抢焦点、可拖动的浮层，由内容决定尺寸后再定位。 | E-024, E-032 | SEEN | 必须兼容 |
| R-H11 | 托盘（显示主窗／退出）；开机自启以登录项 + `--autostart` 参数实现，自启时隐藏 Dock。新系统用 SMAppService 登录项 + 菜单栏常驻替代。 | E-033 | SEEN | 可替代 |
| R-H12 | 宿主动作必须由宿主常驻执行：设备只发通知，不自己执行（Smart Action 存在设备文件里，触发后由宿主做）。宿主不在时的设备行为未知。 | E-024, E-031 | SEEN（通知方向）／UNKNOWN（宿主缺席行为） | 必须兼容 |

### 3.3 配置面（C）

| ID | 规则 | 证据 | 级别 | 裁决 |
|---|---|---|---|---|
| R-C01 | 设备配置的权威存储在设备文件系统：`keymap.json` 与 `smart_actions.json`。连接后先列目录，没有 `keymap.json` 视为失败；有则读取两者。 | E-030, E-031 | SEEN | 必须兼容 |
| R-C02 | 写配置：先在副本上生成两份 JSON 并记录校验和，按需发 smart_actions，再发 keymap，之后列目录核对校验和，成功才更新本地缓存。浏览 Profile 名称在原 App 会触发设备更新，新系统必须把"选中"与"写入"分开。 | E-031, E-043 | SEEN | 必须兼容（写后校验）；MVP 不做写入 |
| R-C03 | 键目录按分类组织（字母数字、符号、修饰、F 键、小键盘、媒体、额外、层键、Profile 键、蓝牙通道键、Internal 等），token 前缀 KC_／KI_／KV_／KM_。精确 token 全表未取。 | E-038, E-045 | SEEN（分类）／UNKNOWN（全表） | 必须兼容（未知 token 原样保留） |
| R-C04 | 导出：Profile 打包为 `<名称>-profile.json`（设备类型、语言、Profile 及其引用的 Action／Multi／Smart 与分组）；Layer 同理为 `<名称>-layer.json`；导入为 JSON 文件。 | E-039 | SEEN | 可替代（格式兼容待确认） |
| R-C05 | 数据模型要素：Profile（id 与数组位置分开）→ Layer（名称、颜色、13 键布局、旋钮 3 槽、摇杆类型与扇区、两区灯光）；Macro（事件 kc/delay/act，act 为 release/press/click）；Multi Action 四分支 + tapping term；分组；linkedApps；灯效 6 种。数值范围全部 UNKNOWN。 | E-016, E-044, E-045, E-046 | SEEN（结构）／UNKNOWN（范围） | 必须兼容（结构）；范围待确认 |
| R-C06 | 本地缓存：LokiJS 单文件，集合 app_settings、devices（每设备配置副本）、presets、migrations。新系统用自己的存储（例如 SQLite 或 JSON 文件），**不读写**原 App 的数据库。 | E-028, E-047 | SEEN | 可替代 |
| R-C07 | 应用设置：分析同意、命令执行许可、设备通知许可、开机启动。新系统保留后三者，分析可丢弃。 | E-029, E-045 | SEEN | 部分必须兼容（命令许可、通知、自启）；分析丢弃 |
| R-C08 | Presets：内置目录、可安装为层。安装时的资源 ID 重映射与冲突规则未知。 | E-022, E-045 | SEEN（存在）／UNKNOWN（规则） | 待确认（非 MVP） |

### 3.4 设备通信面（D）

| ID | 规则 | 证据 | 级别 | 裁决 |
|---|---|---|---|---|
| R-D01 | 设备识别：VID 0x303A；Creator Micro 2 PID 0x8297、0x8298（布局 universal）；Codex Micro 0x8360。只对注册表内 PID 认设备。 | E-007, E-010 | SEEN | 必须兼容（以本机实测为准） |
| R-D02 | 发现：IOKit HID 匹配／移除通知只作变化信号，合并约 175 ms 后重新枚举；只接收厂商 usage page 0xFF00 的接口；先按厂商字符串＋VID 匹配。 | E-008, E-011 | SEEN | 必须兼容（语义）；实现可替代 |
| R-D03 | 串口发现仅用于旧设备和 bootloader 模式，Micro 2 日常不走串口。 | E-011, E-017 | SEEN（代码分支）／INFERRED（Micro 2 不走） | 丢弃（MVP） |
| R-D04 | 传输帧：64 字节 HID report；单片载荷至多 61 字节，长消息多片；report 内带 channel 与长度，debug 与 RPC 分 channel；文本行以换行结束。report ID 数值、字段偏移、channel 编号均 **UNKNOWN**。 | E-012, E-044 | SEEN（帧大小与分片原则）／UNKNOWN（字节布局） | 必须兼容；布局待确认 |
| R-D05 | 消息：JSON-RPC 风格对象（method、params、id），非 ASCII 转义；id 为 [0,999) 的整数；无 id 的入站对象按 `method`（或简写 `m`）作为通知派发；另有旧式 `#rpc#…#` 文本协议（主要走串口）。错误对象结构 **UNKNOWN**。 | E-012, E-013, E-014 | SEEN | 必须兼容（JSON-RPC 面）；旧式协议丢弃 |
| R-D06 | 蓝牙：没有 CoreBluetooth／noble 用法；kit 通过 HID release 号区分 USB 与蓝牙，诊断报告区分两者，键目录有蓝牙通道键。推断蓝牙路径是系统层 BLE HID（HID over GATT）暴露为普通 IOHID 设备，与 USB 走同一 HID 通道。 | E-006, E-008, E-011, E-036, E-038, E-048 | SEEN（无 CoreBluetooth）／INFERRED（BLE HID 路径） | 待确认（MVP 只做 USB） |
| R-D07 | 会话：每设备一个串行发送队列，任务之间留约 50 ms；断连时立即拒绝全部挂起请求、清队列与重组缓冲；macOS 上非独占打开 HID；锁屏时断开所有设备，恢复后重新搜索。错误分类见 WLDeviceErrorCode。超时值 **UNKNOWN**。 | E-012, E-016, E-035 | SEEN | 必须兼容（行为） |
| R-D08 | 文件 API：fs.list／read／readbin／write／writebin／delete／rmdir 与事务 fs.txbegin／fs.txcommit；有分块读写与多文件写。参数、返回 schema、校验和算法、事务语义 **UNKNOWN**。 | E-014, E-015, E-030, E-031 | SEEN（方法名）／UNKNOWN（语义） | 只读子集必须兼容；写方法待确认 |
| R-D09 | 状态与杂项 RPC：sys.version、device.status（MVP 白名单候选）；lights.preview、ui.*、appmgr.*、host.focused_app；sys.bootloader／sys.selftest 属危险方法。 | E-014, E-015 | SEEN（方法名）／UNKNOWN（schema） | sys.version、device.status 必须兼容；host.focused_app 必须兼容（后续）；bootloader／selftest 丢弃 |
| R-D10 | 固件：通过 GitHub 发布仓获取（Micro 2 → `cm-v2-fw-releases`），用 esptool 在 bootloader 串口模式刷写，可手动选 `.bin`。 | E-017, E-042 | SEEN | 丢弃（新系统只显示版本，可链接到官方 App） |
| R-D11 | LVGL wasm 只用于带屏设备的壁纸／图像转换；Micro 2 无 wallpaper 特性。 | E-019, E-025 | SEEN | 丢弃 |

### 3.5 更新与诊断（U）、新系统约束（N）

| ID | 规则 | 证据 | 级别 | 裁决 |
|---|---|---|---|---|
| R-U01 | App 自更新走 electron-updater + GitHub 发布仓。新系统用自己的更新方案（如 Sparkle）或先不做。 | E-004, E-017, E-034 | SEEN | 可替代 |
| R-U02 | 诊断：跑一组只读诊断 RPC 并生成 Markdown 报告，可导出日志。新系统做自己的诊断导出，只包含白名单查询结果。 | E-036 | SEEN | 可替代 |
| R-N01 | MVP 只读：不写配置、不切/激活 Profile、不刷写、不复位；浏览选择不得产生设备写请求。 | E-043, E-031 | SEEN（项目约束） | 必须遵守 |
| R-N02 | 单设备单会话；遇设备被占用不抢占、不自动退出原 Input。 | E-012, E-043 | SEEN | 必须遵守 |
| R-N03 | 未知字段与未知 token 原样保留，界面标"未知"，不显示成空绑定。 | E-044, E-045 | SEEN（项目约束） | 必须遵守 |
| R-N04 | 提供模拟传输（内存文件表 + 可编程通知），保证没有设备也能跑通全链路。原系统也内置模拟设备。 | E-041 | SEEN | 必须（自研） |

## 4. 宿主边界汇总

| 能力 | 触发方 | 宿主所需 | 权限（TCC） | MVP |
|---|---|---|---|---|
| HID 访问 | 宿主 | IOKit HID，非独占 | 输入监控（R-H02，SEEN）| 是 |
| 前台应用上报 | 宿主→设备 | NSWorkspace 激活通知 | 无（NSWorkspace 不需 TCC，INFERRED） | 否 |
| 文本插入 | 设备通知 | 剪贴板 + 发 ⌘V | 辅助功能（CGEvent）或 自动化 System Events（AppleScript）— 待确认（R-H07） | 否 |
| 开应用／开 URL | 设备通知 | NSWorkspace | 无（INFERRED） | 否 |
| 执行命令 | 设备通知 | `/bin/sh -c`，受设置开关 | 无 TCC；应用内显式许可（R-H09） | 否 |
| 径向菜单 | 设备通知 | NSPanel 浮层，非激活、穿透鼠标、所有空间 | 无 | 否 |
| Cheat Sheet | 设备通知 | NSPanel 浮层，可拖 | 无 | 否 |
| 菜单栏／自启 | 宿主 | MenuBarExtra、SMAppService | 登录项（用户可见） | 否 |
| 蓝牙 | — | 不直接使用 | 不申请（R-H03） | 否 |

## 5. 配置面数据模型（新系统）

- `DeviceConfig`：来源（设备快照／演示／导入）、读取时间、固件版本、原始 JSON 保留（未知字段不丢）。
- `Profile { id, index, name, layers[] }`，`id` 与 `index` 分开（R-C05）。活动 Profile 语义未证实前显示未知。
- `Layer { id, name, color, keys[13], encoders[3], joystick{ kind, sectors[] }, lights{ backlight, underglow } }`；键位行长 2/4/4/3（E-044）。
- `Binding = Basic(token) | Macro(ref) | Multi(ref) | Smart(ref) | Unknown(raw)`。
- `Macro { id, name, color, events[{ token, delay_ms, act: Release|Press|Click }] }`；`MultiAction { tap, double_tap, hold, tap_hold, tapping_term_ms }`。
- `SmartAction = Text | Command | Url | App`（宿主执行）。
- 本地持久化：新系统自有存储（R-C06）；导入导出兼容 `-profile.json`／`-layer.json` 外形待用真实导出文件核对（R-C04）。

## 6. 设备通信面（新系统）

1. **发现**：IOKit HID 通知 → 去抖约 175 ms → 枚举 → 过滤 VID/PID/usage page（R-D01、R-D02）→ 标注 USB/BLE（判定规则待实测，R-D06）。
2. **连接**：非独占打开；同一时刻一个会话；断连使会话和挂起请求全部失效（R-D07）。
3. **帧层**：`trait Framer`（编码请求为若干 report、从 report 重组行）。具体布局 UNKNOWN，必须以实测填（R-D04）。
4. **RPC 层**：JSON-RPC 风格，id ∈ [0,999)，响应按 id 匹配，通知按 method 派发，串行队列 + 间隔（R-D05、R-D07）。
5. **白名单**：MVP 只允许 `sys.version`、`device.status`、`fs.list`、`fs.readbin`／`fs.read`（后两者参数以实测为准）。写、删、事务、预览、bootloader、selftest 编译期禁止（R-N01、R-D08、R-D09）。
6. **固件／LVGL**：不做（R-D10、R-D11）。

## 7. 新系统映射

| 模块 | 职责 | 对应原系统 |
|---|---|---|
| crate `mi-model` | 配置类型、解析（保留原始值）、校验、导入导出 | renderer 数据模型、device_keymap 的解析部分 |
| crate `mi-transport` | `trait HidTransport`；macOS IOKit 实现；`MockTransport` | node-hid + kit discovery/comm |
| crate `mi-proto` | `trait Framer`、`trait RpcCodec`、请求/通知类型、方法白名单 | WLRPCClient、WLRPCApi |
| crate `mi-session` | 设备会话状态机、串行队列、超时、断连失效、读取快照 | devices_comm_manager、device_file_service |
| crate `mi-host`（后续） | 通知路由到宿主动作的纯逻辑（权限开关判断、参数校验） | device_notify_service |
| crate `mi-ffi` | 对 Swift 的稳定接口（UniFFI 或 swift-bridge），异步事件回调 | preload + ipcMain 通道 |
| Swift `MicroInputApp` | SwiftUI 主窗：设备列表、Profile/Layer 浏览 | 主窗 renderer |
| Swift `HostKit`（后续） | NSWorkspace 前台监听、文本插入、开应用/URL、命令执行、权限检查 | native_service、focus_app_service、application_service |
| Swift `Overlays`（后续） | 径向菜单与 Cheat Sheet 的 NSPanel | radial_menu、cheat_sheet 窗口 |

**IPC 方式**：单进程内 FFI（Rust 静态库 + 生成的 Swift 绑定）。UI 只调用高层命令（列设备、连接、读身份、读配置），不提供原始 RPC 透传（R-A05）。若后续拆成菜单栏常驻代理 + 配置器两个进程，用 XPC，同样只暴露高层命令。

## 8. 能力裁决汇总

- **必须兼容**：USB HID 发现与识别、输入监控权限引导、JSON-RPC 会话语义（串行、id 匹配、通知派发、断连失效）、设备文件为权威配置、写后校验（后续）、设备触发的宿主动作（文本、命令（默认关）、URL、应用、径向菜单、Cheat Sheet、前台应用上报）。
- **可替代**：Electron 结构、LokiJS 存储、托盘与自启、自更新、诊断、导入导出实现。
- **丢弃**：edge-js、串口与旧式协议、固件刷写、LVGL/壁纸/媒体、分析统计、通用 RPC 透传、Windows/Linux。
- **待确认**：BLE 路径、辅助功能/自动化需求、报文字节布局、fs 方法语义与 schema、导出格式细节、Presets。

## 9. 未知与风险

| 项 | 状态 | 风险 | 处理 |
|---|---|---|---|
| 报文字节布局（report ID、header、channel） | UNKNOWN | 猜错会让设备收到无意义数据 | `Framer` trait 占位，实测后再实现；禁止盲发 |
| 超时、错误结构、响应 schema | UNKNOWN | 误判失败／成功 | 统一"原始 JSON + 未知"显示 |
| `fs.*` 参数与校验和 | UNKNOWN | 读错文件、误调写方法 | 编译期白名单；只读 |
| BLE 是否同路径 | INFERRED | 设备去重错误 | MVP 只认 USB |
| 辅助功能／自动化 | INFERRED | 文本插入失败 | 后续宿主功能前补证 |
| edge-js 运行时动态加载 | UNKNOWN（倾向未用） | 低 | 丢弃 |
| 原 Input 并存争用 | SEEN（非独占打开） | 双方同时发写导致设备异常（社区报告） | 检测到原 Input 运行时提示，只读，不抢占 |
| 社区协议资料版本不同 | SEEN | 误用旧参数 | 只作线索，以实测为准 |
