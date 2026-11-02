# EVIDENCE — Micro Input 统一证据索引 v1.0

> E-001～E-048 保留早期调查的自撰摘要及定位，属于历史证据，不是全部结论的最新状态。本轮只核对项目文档与 GitHub 状态，没有重读厂商代码、资源或用户数据。后到证据与冲突裁决见下文；待确认项统一在 [UNKNOWNS](UNKNOWNS.md)。

- 日期：2026-11-02（Asia/Shanghai）。对象：Helios `/Applications/input.app`（Input 1.1.0，bundle `it.focusense.input-app`）及 `~/Projects/micro-input` 调研原件。
- 方法：只读的便宜工具（`plutil`、`codesign`、`otool -L`、`nm`、`file`、内存中解析 asar 头并对 main/preload/worker/renderer 做符号与字符串计数、读取 kit 的 JSDoc 注释与枚举名）。未解包到磁盘、未反编译、未动态跟踪、未抓包、未读 TCC 数据库、未连接或操作设备。
- 级别：**SEEN**＝本轮或既有调研直接观察；**INFERRED**＝由 SEEN 推断；**UNKNOWN**＝证据不足。
- clean-room 说明：下表只记路径、模块／符号／枚举名、通道名、版本号和用自己话写的行为摘要；不含厂商代码片段、资源或大段文案。
- 简写：`APP`＝`/Applications/input.app/Contents`；`ASAR`＝`APP/Resources/app.asar`（内存解析）；`KIT`＝`APP/Resources/app.asar.unpacked/node_modules/@worklouder/wl-device-kit`；`MAIN`＝`ASAR:/dist-electron/main/index.js`；`REPO`＝`~/Projects/micro-input`。

## 证据表

| ID | 级别 | 来源（路径／命令） | 观察摘要（自己的话） | 支撑规则 |
|---|---|---|---|---|
| E-001 | SEEN | `plutil -p APP/Info.plist` | 版本 1.1.0，最低 macOS 12.0，类别 developer-tools，主类 AtomApplication（Electron）。有 Bluetooth、Camera、Microphone、Audio capture 的泛用说明文字；**没有** `NSAppleEventsUsageDescription`、没有输入监控／辅助功能相关键（这两项本来也不在 Info.plist 声明）。 | R-H01, R-H03, R-H07 |
| E-002 | SEEN | `codesign -d --entitlements :- APP/MacOS/input` | 仅有 JIT、unsigned-executable-memory、dyld 环境变量三项（典型 Electron 硬化运行时）；无 sandbox、无 apple-events、无 bluetooth 等 entitlement。 | R-H01, R-H07 |
| E-003 | SEEN | 盒上 `/workspace/input-app-lean-triage.md` §1 | Electron 40.10.6 / Chromium 144；主可执行为 arm64 薄壳；Developer ID Focusense Srl，已公证；非 IME（无 PlugIns/appex）。 | R-A01, R-A02 |
| E-004 | SEEN | `ASAR:/package.json` | 入口 `dist-electron/main/index.js`，ESM；运行依赖：`@worklouder/wl-device-kit ^0.2.13`、`electron-edge-js ^40.0.1`、`electron-updater`、`electron-dl`、`electron-log`、`lokijs`、`jimp`、`gifwrap`、`semver`。 | R-A01, R-U01 |
| E-005 | SEEN | asar 文件清单（内存解析头） | 业务文件仅：main 单包、3 个 preload（主窗、cheat sheet、radial menu）、1 个 worker（`search_devices_worker.js`）、3 个窗口 HTML、Vite 分块（Main、MainScaffold、ChooseDevice、PermissionsGuide、device_config_data、smart_action、cheat_sheet、radial_menu、EditWallpaper、native）。 | R-A01, R-A03 |
| E-006 | SEEN | asar `node_modules` 目录名列表 | 无 noble／bluetooth-hci／任何 BLE Node 库；有 serialport、esptool-js、lokijs、zod、jimp 系列。 | R-D06, R-A04 |
| E-007 | SEEN | `KIT/package.json`、`find KIT -type f` | kit 0.2.13，UNLICENSED，GitHub 私有 npm 源；依赖 node-hid ^3.4.0、serialport ^12、esptool-js ^0.6、semver；文件：`dist/index.js`（未压缩、带注释）、`dist/browser_safe.js`、`wasm-node/wl_lvgl_wasm*`、native：`darwin/{permissions,device_discovery}.node`、`win32/x64/device_discovery.node`、`linux/*/serial_control.node`。公开导出：ConnectionEventType、ConnectionType、DeviceLayoutType、DeviceType、JsonRPCMethods、LightingEffect、MediaErrorCode、RpcSignalType、WLDeviceCommImpl、WLDeviceDiscovery、WLDeviceError、WLDeviceErrorCode、WLDeviceProgrammer、WLPermissions、WLRPCApi、WLRPCClient、WLRelease、convertImageToLvgl、convertLvglToImage、noopLogger。 | R-A04, R-D01 |
| E-008 | SEEN | `otool -L`、`nm -u` on `KIT/dist/native/darwin/*.node` | `device_discovery.node`（universal）只链 IOKit+CoreFoundation，导入 IOHIDManager 匹配／移除回调、IOServiceAddMatchingNotification、CFRunLoop；`permissions.node` 只链 CoreGraphics，唯一业务导入为 `CGPreflightListenEventAccess`（输入监控预检）。两者都**不**链 CoreBluetooth／IOBluetooth。 | R-D02, R-H02, R-D06 |
| E-009 | SEEN | kit 源段标记（`// src/...`） | 模块：wl_rpc_client、wl_rpc_api（含 media_player、lighting_effect 类型）、wl_device_discovery（platforms linux/mac/windows、device_registry、wl_const）、wl_device_comm、wl_lvgl、wl_device_programmer（node_webserial_port、linux_serial_control）、wl_permissions、wl_release。 | R-A04 |
| E-010 | SEEN | kit `DeviceType` 枚举、`DEVICE_REGISTRY`、`WL_VID` | 设备类型：nomad_e、nomad_e_v2、knob、knob_f1、creator_micro_v2、xyz、codex_micro、bootloader。creator_micro_v2 对应 PID 33431、33432（0x8297、0x8298），布局 universal；codex_micro 为 33632（0x8360）。VID 12346（0x303A）；厂商字符串两种写法（空格／下划线）。 | R-D01, R-D02 |
| E-011 | SEEN | kit `WLDeviceDiscovery` JSDoc 与方法名 | 方法：init、isListeningForChanges、stopListeningForChanges、findWLDevices、filterWLDevices、findWLSerialDevices、findWLBootloaderDevices。HID 枚举只收注册表内 PID 且 usage page 为厂商页 0xFF00 的接口；先按厂商字符串+VID 匹配，无果再退回 VID。USB 与蓝牙用 HID 设备 release 号的低位区分；注释说 macOS/Windows 会从蓝牙设备名填厂商字符串。OS 变化通知只作"脏"信号，合并 175 ms 后由调用方重新枚举。串口扫描用于旧 Nomad 与 bootloader（DFU）模式设备。 | R-D02, R-D03, R-D06 |
| E-012 | SEEN | kit `WLDeviceCommImpl` JSDoc 与方法名 | 方法：connect、connectWithSerial、connectWithHID、disconnect、isConnected、sendLegacyRpcRequest、sendJsonRpcRequest、sendDataHID/sendDataSerial、parseHIDReport、parseRpcData、addNotifyHandler/removeNotifyHandler、handleNotifyMessage、enqueue、abortJsonRpcRequest。macOS 上以**非独占**方式打开 HID；串口 115200。发送：长于 61 字节的消息拆成多个 64 字节 HID report。接收：每个 report 解出 channel、长度与 UTF-8 载荷，按 channel 区分 debug 与 RPC，以换行结束一行；JSON 片段累积到完整对象后，带 id 的解析为响应，否则按 `method`（或简写 `m`）派发为设备通知。发送串行排队，任务间隔 50 ms；断连时拒绝全部挂起请求并清队列。**未提取**：report ID 数值、header 内字段偏移、channel 编号、超时值。 | R-D04, R-D05, R-D07, R-N02 |
| E-013 | SEEN | kit `WLRPCClient` JSDoc；`RpcSignalType` | 两类请求：旧式 `#rpc#args#` 文本，与 JSON-RPC 风格对象（method/params/id），序列化时非 ASCII 字符转义；id 为随机整数，注释称范围受固件约束为 [0, 999)。中止信号类型 abort=0。 | R-D04, R-D05 |
| E-014 | SEEN | kit `JsonRPCMethods` 枚举 | version／bootloader／selftest（旧式）；sys.version、sys.bootloader、sys.selftest、device.status；mp.write_info、mp.write_artwork、media.state.update、media.cover.chunk；fs.list、fs.read、fs.write、fs.writebin、fs.readbin、fs.delete、fs.txbegin、fs.txcommit、fs.rmdir；appmgr.list_active、appmgr.list_installed；ui.active_screen、ui.home_accent_color；lights.preview；host.focused_app。 | R-D05, R-D08, R-D09 |
| E-015 | SEEN | kit `WLRPCApi` 方法名 | getFirmwareVersion(+Legacy)、getDeviceStatus、sendIntoBootloader(+Legacy)、sendIntoSelfTest(+Legacy)、getFileList、readFile、readFileChunked、writeFile、writeFileChunked、writeFileChunkedFromStr、deleteFile、deleteFolder、beginMultifileWrite、commitMultifileWrite、sendMultipleFiles、sendLightingPreview、sendHomeAccentColor、getDeviceCurrentScreen、getFwActiveApps、getFwInstalledApps、sendFocusApp、媒体相关若干、splitIntoBase64Chunks、abortRpcCall。 | R-D05, R-D08 |
| E-016 | SEEN | kit 枚举 `WLDeviceErrorCode`、`ConnectionType`、`ConnectionEventType`、`DeviceLayoutType`、`LightingEffect` | 错误码：ALREADY_CONNECTED、TIMEOUT、WRITE_FAILED、HID_UNAVAILABLE、DEVICE_DISCONNECTED、RPC_ERROR、INVALID_RESPONSE、ABORTED、UNKNOWN。连接类型 serial／hid；事件 CONNECTED／DISCONNECTED／ERROR；布局 unknown/ansi/iso/universal；灯效 off/solid/snake/rainbow/breath/gradient。 | R-D07, R-C05 |
| E-017 | SEEN | kit `WLDeviceProgrammer`、`WLRelease` 方法名与 JSDoc | 编程器基于 esptool（ESP 芯片串口烧录、读 eFuse 身份，需设备处于 bootloader 模式）；方法 flashDeviceFirmware、flashFiles、readEfuses、resetDeviceAfterFlash。WLRelease 从 GitHub 拉发布与缓存，creator_micro_v2 与 codex_micro 映射到 `cm-v2-fw-releases`，支持 beta 轨道、dev 版本跳过、特定 dirty 版本特例；getInputReleaseNotes 读 App 发布说明。 | R-D10, R-U01 |
| E-018 | SEEN | kit `WLPermissions` JSDoc | check(devicePaths)：darwin 调 permissions 原生模块询问"输入监控"权限；linux 检查 hidraw 节点读写；其他平台忽略路径。 | R-H02 |
| E-019 | SEEN | `KIT/wasm-node/*`、kit `wl_lvgl`、MAIN `image-channel-*` | LVGL 图像格式与普通图片互转（wasm 实现），主进程经 image 通道给壁纸／GIF 编辑用。 | R-D11 |
| E-020 | SEEN | MAIN 的 import 语句计数 | 从 electron 取 ipcMain、app、BrowserWindow、dialog、shell、Menu、screen、nativeTheme、Tray、Notification、clipboard、powerMonitor；从 kit 取 ConnectionEventType、DeviceType、WLPermissions、WLRelease、convertImageToLvgl/convertLvglToImage、WLDeviceErrorCode、WLDeviceCommImpl、WLRPCApi、DeviceLayoutType、WLDeviceError、WLDeviceProgrammer、WLDeviceDiscovery；另有 child_process、worker_threads、lokijs、electron-updater、electron-dl、jimp、gifwrap。**无** electron-edge-js、无 systemPreferences、无 globalShortcut。 | R-A02, R-A05, R-H04 |
| E-021 | SEEN | MAIN 日志标签 `|xxx|` 去重 | 服务：application、analytics、storage、update、window、device_notify、device_file、device_keymap、device_screen、devices_comm_manager、diagnostic、focus_app、fs、image、mac_native/native、media_player_collector/devices、media_v2、mock_comm、mock_device、remote_api_client、local_storage_channel、connected_device_channel 等。 | R-A02 |
| E-022 | SEEN | MAIN 枚举（IPC 名） | 通道组：common（app-version、open-mac-settings、start-focus-detect、common-check-app-permissions、common-get/send-app-settings 等）、devicesManager（devsManGetDevs、devsManSearchDevs、devsManGetBootloaderDevs、devsManFlashFw 及进度／日志事件）、connectedDevice（connect、disconnect、sendJsonRpcCall、sendLegacyRpcCall、connDeviceChangeDeviceConfig、connDeviceRunDiagnostic、connDeviceRefreshDeviceStatus、onHidUnavailable 等）、rpc（rpcGetFirmwareVersion、rpcGetDeviceStatus、rpcGetFileList、rpcReadFile、rpcWriteFile(Chunked)、rpcDeleteFile、rpcSendLightingPreview、rpcSentIntoBootloader 等）、localStorage（localStorageGet/SaveDeviceConfig、Get/SavePreset、迁移三项）、fs（downloadFile、readFile、fsChannelSelectApplication、fsChannelGetApplications、fsChannelOpenApplication、fsChannelSaveDiagnosticReport）、image（image-channel-*）、update（check-updates、install-update、get-app-release-notes、on-update-downloaded）、radialMenu（OnInitData、OnMouseMove、OnJoystickMove、OnHide）、cheatSheet（OnInitData、OnModeChange、OnHide、ReportWindowSize、Start/StopDrag）、undo/redo。 | R-A03, R-A05, R-C08 |
| E-023 | SEEN | 3 个 preload 与 worker 的字符串集合 | 主窗 preload 暴露全部通道组；cheat sheet 与 radial menu preload 只带各自事件名（加公共键表）；worker 只处理 searchDevices／resume／shutdown 并回传结果或错误。 | R-A03 |
| E-024 | SEEN | MAIN 枚举 `T` 与 device_notify 路由 | 设备主动发给宿主的通知：kb.radial、kb.sa.inserttext、kb.sa.exec、kb.sa.openapp、kb.sa.openurl、kb.cs.show/hide/toggle、mp.fetch_data、media.state.get、media.cover.get、alert.generic、diag.report。路由：inserttext 取文本交原生服务；exec 仅在设置 smartActionCmdEnabled 为真时执行 shell 命令，否则记日志丢弃；openapp 按路径打开应用；openurl 打开 URL；radial 交窗口服务。参数缺失或类型不对则丢弃。 | R-H05, R-H06, R-H08, R-H09, R-H12 |
| E-025 | SEEN | MAIN 设备 feature 表 | creator_micro_v2 与 codex_micro 只开 radialMenu、focusedApp 两个特性；Knob 类开 alert、wallpaper；XYZ 只开 focusedApp。radial/cheat sheet 通知监听只为前两类型注册。 | R-H05, R-H08, R-D11 |
| E-026 | SEEN | MAIN focus_app_service | 有带 focusedApp 特性的设备连接时启动；约每 1 秒调用一次前台窗口脚本；前台应用路径变化时对每台设备调用 sendFocusApp（对应 host.focused_app）；设备都断开时停止。 | R-H04 |
| E-027 | SEEN | `ls APP/Resources/scripts`；MAIN native_service | 三个 AppleScript（源码与编译版各一）：粘贴（经 System Events 发 ⌘V）、前台应用（NSWorkspace 取名称／bundle id）、媒体信息（Spotify/Music）。插入文本流程：先保存剪贴板，写入文本，`osascript` 触发粘贴，再恢复剪贴板。 | R-H05, R-H04, R-H07 |
| E-028 | SEEN | MAIN storage_service | LokiJS 数据库文件 `userData/input_storage.json`，集合 app_settings、devices、presets、migrations，缺失时创建；另有 `user-id.json` 保存分析用 client id。 | R-C06 |
| E-029 | SEEN | MAIN AppSettings DTO 字段 | showedAnalyticsPopUp、analyticsConsented、smartActionCmdEnabled（默认 false）、deviceNotificationsEnabled（默认 true）、autoStartupEnabled（默认 true）。 | R-C07, R-H09 |
| E-030 | SEEN | MAIN device_file_service | 设备连接后先 fs 列目录，要求存在 `keymap.json`，否则标记读取失败；再读取 `keymap.json` 与 `smart_actions.json`，有文件拉取状态（未拉取／拉取中／完成／错误）事件。壁纸相关路径仅用于带屏设备。 | R-C01, R-D08 |
| E-031 | SEEN | MAIN device_keymap_service（sendConfigData 及 JSDoc） | 写设备配置流程：在副本上准备 JSON 并先记录校验和；Smart Actions 仅在强制、无本地存档或与存档不同才发；用分块写入发送 smart_actions 再发 keymap；随后 fs.list 校验文件校验和，通过才写入本地 Loki。默认 Multi Action 模板 tapping term 250。 | R-C02, R-D08, R-H12 |
| E-032 | SEEN | MAIN window_service 的 BrowserWindow 选项 | 主窗 1500×903、最小 900×600。radial menu 窗：无边框、透明、不可聚焦、不可移动，铺满所在显示器，在所有桌面含全屏可见，macOS 下为 panel 类型并穿透鼠标。cheat sheet 窗：无边框透明不可聚焦 panel，先放屏外，渲染端回报尺寸后定位，可拖动。 | R-H08, R-H10 |
| E-033 | SEEN | MAIN tray_service、启动参数 | 托盘菜单只有"显示主窗"与"退出"；`--autostart` 启动时隐藏 Dock 只留托盘；开机启动通过登录项设置（带 `--autostart`）。 | R-H11 |
| E-034 | SEEN | `APP/Resources/app-update.yml`；MAIN update_service | electron-updater，GitHub provider `worklouder/input-releases`；检查并通知、下载后退出安装；发布说明经 WLRelease 读取。 | R-U01 |
| E-035 | SEEN | MAIN powerMonitor 监听 | 锁屏：停媒体服务、清各设备发送队列、断开所有设备；resume：恢复设备搜索；解锁：有对应处理（细节未展开）。 | R-D07 |
| E-036 | SEEN | MAIN diagnostic_service | 诊断跑一组 RPC（名单含 ble.params、ble.ring、sentry.get/crash/coredump 等），生成 Markdown 报告（含设备类型、布局、PID、连接 USB/Bluetooth、固件、RPC 成功数），可另存 `.md`。原生 Help 菜单有导出控制台日志。 | R-D06, R-U02 |
| E-037 | SEEN | MAIN application_service；`ASAR:/dist/assets/PermissionsGuide-*.js`；device_config_data 启动路由 | 启动发现流程中，若 darwin 且 checkAppPermissions 为假则进入权限引导页；引导页要求在"隐私与安全性 → 输入监控"给 Input 授权，提供"打开设置"（打开 `Privacy_ListenEvent` 面板）与"重新检查"。未见辅助功能／自动化的检查或引导。 | R-H02, R-H03, R-H07 |
| E-038 | SEEN | `ASAR:/dist/assets/device_config_data-*.js` 字符串计数；EditWallpaper 分块 | 键目录分类：Alphanumerics、Glyphs、Modifiers、Function keys、Number pad、Media、Extra、Layer Keys、Profile Keys、**Bluetooth keys**（KI_CBT1–3、KI_CBTP1–3、KI_CUSB 共 7）、Smart Actions、Actions、Internal 等；token 前缀 KC_／KI_／KV_／KM_。壁纸页对蓝牙连接下上传 GIF 提示较慢（带屏设备）。 | R-C03, R-D06 |
| E-039 | SEEN | device_config_data 分块中的导出／导入逻辑 | Profile 导出为 JSON 文件 `<名称>-profile.json`，包含设备类型、语言、profile 本体及其引用到的 actions、multiactions、smartActions 与三类分组；Layer 导出为 `<名称>-layer.json`，结构同理但以单层为主体；导入用 `.json` 文件选择器；Preset 页也接受 JSON 导入。 | R-C04 |
| E-040 | SEEN | `ls/cat` unpacked `electron-edge-js`；全部 app JS 关键词计数 | electron-edge-js 40.0.1（.NET CoreCLR 进程内桥）随包，含 darwin arm64/x64（按 Electron 35–40 分目录）与 win32 原生件；但 main、3 个 preload、worker 与全部 renderer 分块中**没有任何** edge-js 引用，kit 也不 require 它。 | R-A06 |
| E-041 | SEEN | MAIN mock_comm_service／mock_device | 内置模拟设备与模拟通讯：内存文件表实现 fs.* 方法、开发用摇杆窗口与按钮（环境变量控制隐藏）。 | R-N04 |
| E-042 | SEEN | MAIN devicesManager 通道与文件选择器 | 固件流程：列出 bootloader 设备、获取最新固件、刷写并回报进度和日志；支持手动选择 `.bin` 固件。 | R-D10 |
| E-043 | SEEN | `REPO/README.md`、`docs/development.md`、`lessons.md`、`AGENTS.md` | 自研方向 Rust 核心+SwiftUI；首版为 USB 只读查看器；原 Input 中点击 Profile 名称会触发"Updating device"倒计时；设备保存行为与宿主常驻行为分开建模；禁止复位、刷写、写配置、自动激活 Profile。 | R-N01, R-C02, R-N02 |
| E-044 | SEEN（含社区线索 C） | `REPO/.scratch/input-study/out/research.md` | 本机缓存 `keymap.json` 结构（profiles→layers→layout{keymap 行长 [2,4,4,3]、encoders 3 槽、joystick sectors}、lights{backlight,underglow}；macros、multiActions、groups、linkedApps）；`activeProfileId` 可能是下标而非 id；社区对报文的描述（report ID、channel 编号、换行规则）互相有出入且版本不同。 | R-C01, R-C05, R-D04, R-N03 |
| E-045 | SEEN | `REPO/.scratch/input-study/out/connected-ui-inventory.md` | Settings 四开关；Profile 菜单项；Key Mapping 171 项；Action 基础／高级模式；Multi Action 四分支、默认 250 ms；Smart Action 四类（Text/Cmd/Web/App）与命令开关。 | R-C03, R-C05, R-H09, R-C08, R-N03 |
| E-046 | SEEN | `REPO/.scratch/input-study/out/layer-lighting-joystick-followup.md` | 两区灯光六种效果及 Light/Speed/Magic/Hue/Saturation/Hex 字段；App linking 前台停留 5 秒；合并键影响所有层；摇杆页 Macro／Joystick 两模式；数值范围未证实。 | R-C05, R-H04 |
| E-047 | SEEN | 盒上 `/workspace/input-app-lean-triage.md` §2–§4 | 进程／模块边界汇总；本机状态目录 `~/Library/Application Support/input/{input_storage.json, devices/<id>/{keymap.json,smart_actions.json}}`（本轮未重读，属授权外路径）。 | R-A02, R-C06 |
| E-048 | SEEN | `otool -L` Electron Framework | Electron Framework 本身链接 CoreBluetooth 与 IOBluetooth（Chromium 的 Web Bluetooth 支持）；app JS 中无 `navigator.bluetooth`／`navigator.hid` 引用。 | R-H03, R-D06 |

## 证据级别与适用范围

- **DECIDED**：用户已确认的产品决定，不是硬件事实。
- **SEEN-O**：项目记录的本机观察；注明本轮是否重放。**SEEN-S**：静态行为摘要；只证明该版本客户端行为。
- **COMMUNITY**：第三方特定版本的观察或实现；**OFFICIAL-SNAPSHOT**：某日期的官方发布状态。二者不自动证明当前设备兼容。
- **SYNTHETIC**：独立生成的合成输入/向量；不证明固件接受。**IMPLEMENTATION**：指定提交的实现或验收记录；不代表已合并。
- **INFERRED / UNKNOWN**：推断或证据不足。早期 SEEN 必须连同其来源读，不能一律升级为本机硬件验证。
- 完成状态与事实级别独立记录：有文档、已有代码、通过模拟、通过实机、已合并、已发布分别判断。

## 新增统一索引

以下来源都是本项目的自撰行为说明或本轮用户决定；未复制厂商实现。协议细节的唯一维护位置是 [只读协议](../protocol-readonly.md)，本页只作范围索引。参考仓库完整名单和许可边界见 [参考仓库](../reference-repositories.md)。

| ID | 级别 / 来源 | 结论及限制 | 规则 |
| --- | --- | --- | --- |
| E-049 | DECIDED，本轮用户确认 Q1–Q9 及完整产品基线 | Creator Micro 管理软件，coding 优先；原生、菜单栏+主视图；不是跨 agent 任务管理平台。UX/Agent Experience/DX；原有 IME、刷机等非目标保留。 | R-P01, R-P02, R-X01, R-X02 |
| E-050 | DECIDED，同上 | 草稿/撤销/变更预览/明确应用；浏览不写设备；离线编辑与重连比较；失败保留草稿和副本，恢复须验证。 | R-E01–R-E06 |
| E-051 | DECIDED，同上 | 通用 Coding + 工具专用配置，先手动切换；动作库搜索/分类/收藏/条件说明；agent 只准备草稿，人审阅应用。当前不接自动化、不写设备。 | R-K01–R-K04, R-X03, R-N01 |
| E-052 | SEEN-S，[只读协议 §1–2](../protocol-readonly.md#1-设备发现与身份)，E02–E04 | VID/PID、vendor usage、175 ms 合并；node-hid 的64字节API缓冲/61字节载荷、report/channel/length 已有明确静态摘要。不能机械套成 IOKit/hidapi 参数。 | R-D01, R-D02, R-D04 |
| E-053 | SEEN-S，[只读协议 §2–3](../protocol-readonly.md#2-hid-帧与-json-请求)，E03–E06 | 请求不加换行；长字段、非ASCII转义；接收id/i、method/m、params/p及result/error；状态字段可缺失。ID 0–998是客户端选择，固件极值未验证。 | R-D05, R-D09 |
| E-054 | SEEN-S，[只读协议 §4](../protocol-readonly.md#4-从设备读取配置文件)，E05–E07 | list参数、readbin offset/len/Base64/total_size、SHA-1比对已整理；keymap必需、smart文件可缺；原应用先smart后keymap并复用缓存。自研前后list校验是设计选择，不保证跨文件原子性。 | R-C01, R-D08 |
| E-055 | SEEN-S，[只读协议 §5](../protocol-readonly.md#5-只读配置模型的实现注意点)，E08 | 设备schema不等于UI投影：颜色数值、token转换、joystick k、可选encoders/buttons、SmartActions对象转数组、旧分组名。不能把自编演示格式当设备schema。 | R-C03, R-C05, R-N03 |
| E-056 | SEEN-S，[只读协议 §4](../protocol-readonly.md#4-从设备读取配置文件) | 原客户端单请求10秒、整文件20秒、50ms等待属客户端策略；不证明固件时限或固定100ms间隔。 | R-D07 |
| E-057 | SEEN-O，项目 [只读协议 §7](../protocol-readonly.md#7-dispatch-补充证据与当前固件核验) 记载；本轮未重放 | 2026-11-02独立Swift/IOKit探针非独占打开303A:8298 USB，以usage pair FF00/1匹配，单次sys.version匹配回应0.6.3。无配置读取/写入，不代表Rust后端、长帧或应用验收。 | R-D01, R-D04, R-D09 |
| E-058 | OFFICIAL-SNAPSHOT，同文记录的2026-11-02发布查询 | 当时stable 0.6.3，0.7.0-rc.16为预发布；用户选择稳定通道。不是持续有效的latest声明；本产品不刷固件。 | R-D10 |
| E-059 | COMMUNITY，[只读协议 §7](../protocol-readonly.md#7-dispatch-补充证据与当前固件核验) 的Dispatch固定提交摘要（6c6770a） | 0.6.2社区：完整usage pairs、manager寿命、Secure Input、完整JSON对象重组/紧凑字段、fs.read替代路径。当前0.6.3对应行为待验。 | R-D02, R-D04, R-D07, R-H02 |
| E-060 | COMMUNITY，同上 | 13开关[2,4,4,3]可能对应12实体键（宽键双开关）；AG旋钮/摇杆事件、KV_OAI_AGnn绑定条件、v.oai.hid与多宿主/灯光所有权。不是当前设备布局与事件已验。 | R-C05, R-K03 |
| E-061 | COMMUNITY，同上 | BLE三槽、USB切换、逐键灯与区域灯差异；仍不纳入USB只读验收。 | R-D06, R-C05 |
| E-062 | SYNTHETIC，[protocol-vectors.json](../protocol-vectors.json) | 61/62/122/123边界、Unicode、合成readbin/Base64/SHA-1；由行为说明生成，非设备抓包、非厂商模块输出。 | R-N04, R-D04, R-D08 |
| E-063 | IMPLEMENTATION，[PR #8](https://github.com/semantic-craft/micro-input/pull/8)，e5811a5 | 五Rust模块、UniFFI、SwiftUI合成浏览与测试的候选；真实Framer/schema仍阻断。43ac6eb一次open记录、767379d权限状态差异不能证明稳定授权。12 Rust单测+1 compile-fail、6 Swift测试是该候选的既有验收记录，本轮未重跑。 | R-A02, R-N04, R-H02 |
| E-064 | SEEN，本轮Git/GitHub核对 | 主分支基线9e78e8a合并PR #9；PR #7/#8 CLOSED且mergedAt=null。旧代码未进main；不重开旧PR，不因此关闭 #1–#6。 | R-X02 |
| E-065 | REFERENCE，[参考仓库](../reference-repositories.md) | Dispatch、cm2-probe、creator-micro-kit、micro-manager、OpenBoard及其他条目提供设计线索；每仓型号、版本、许可分别核查。未核实Herdr/DSH/ZCode/QwenWork动作API。156仓检索记录不是156个已验证适配器，索引finalize未完成。 | R-K01, R-K02 |
| E-066 | IMPLEMENTATION，[PR #7评审](https://github.com/semantic-craft/micro-input/pull/7) 主题；准确入口为PR审查页 | 评审指出探针前置条件循环：未知framing时不能先盲发查询来验证framing。先有可靠规格/行为摘要或获准被动观察，再做离线验证和获准实机查询。 | R-D04, R-N01 |

## 冲突裁决与已缩小的未知

| 旧表述 | 当前裁决 | 保留的未知 |
| --- | --- | --- |
| 所有HID字节布局均UNKNOWN | E-052提供node-hid静态布局，E-057只证明一个IOKit短查询；不再说完全没有布局来源 | 后端适配、长帧、接收边界、异常与其他固件 |
| 超时/error/fs参数全未知 | E-053/054/056补充客户端schema与策略 | 设备实际接受范围、错误极值、readbin全链路与一致性 |
| 设备schema等同合成/缓存schema | E-055明确不同，必须单独建立保留raw的设备投影 | 真实配置读回及完整token/容量/活动索引 |
| 13键固定等于13实体键 | 使用13开关矩阵表达存储；宽键双开关是社区候选，UI实体图待本机核对 | 物理位置、方向、宽键释放去重 |
| 五个方法含fs.read均已允许 | USB只读生产候选采用status/version/list/readbin四项，fs.read保留社区线索 | 新增方法需独立证据和验收 |
| 有输入监控才可枚举；无权限必失败 | 枚举无需请求权限；打开/接收按当前签名与后端结果诊断 | Secure Input与权限错误区分、跨构建授权 |
| 保留0–998就是固件约束/能防串台 | 仅保守客户端策略；必须处理跨进程同ID、迟到与重连 | 设备ID边界及多宿主所有权 |
| 应用已实现/PR仍开放 | E-064：旧PR关闭未合并，候选不是主分支交付 | 选择性接续后的新提交验收 |

历史 E-044～E-047 的原始记录定位仍保留用于溯源；本轮只采用其已整理摘要，不读取原厂数据。后续补证必须先写入本页的来源、版本、证明范围和限制，再修改对应规则与验收。完整未决清单见 [UNKNOWNS](UNKNOWNS.md)。

## 工作台实施决定

| ID | 级别 / 来源 | 结论及限制 | 规则 |
| --- | --- | --- | --- |
| E-067 | DECIDED，用户在本对话确认“就按这个工作台方案”，2026-11-02 | 采用工作台布局并同步UI交互验收，开始Rust+SwiftUI首轮实现；不接自动化、不做设备写入、未知协议继续占位。HTML原型证明设计选择和浏览器交互，不证明原生应用已交付。S1a为离线纵向切片，完整S1/S2门槛不变。 | R-P03, R-X03, R-N01 |

- E-068（IMPLEMENTATION，本轮S1a原生验收）：macOS/arm64开发构建，键盘、深浅色、标准/紧凑窗口、主窗关闭/菜单栏重开/共享草稿/退出均实测。键盘采用明确快捷键避免依赖系统全键盘访问设置；菜单栏面板可由状态按钮或⇧⌘M打开，实测使用键盘入口。证明范围限S1a，设备/性能预算/正式签名仍未验收。对应R-P03/R-H11/AC-W04/AC-W05。


## Cindy 与本地文档补充证据

- **E-069 / COMMUNITY**：只读检查 Cindy 提交 `5368875f4bc5d14f1a94387acf53aca3403e4e1b` 的独立宿主代码。macOS先尝试可选SDK包，再寻找ChatGPT/Codex安装内的device-kit-oai；底层发现/连接交给WLDeviceDiscovery/WLDeviceCommImpl/RPCApiOAI。USB候选优先，事件在宿主归一化。Creator Micro 2监听路径可能读取、备份并重写当前层keymap/encoders/joystick后接收Agent事件，因此不能作为无写入按键识别能力证明。权限熔断、占用分类、子进程重启上限、结构化动作和布局归一化可作设计参考；本项目进程内架构不具有同等子进程隔离。没有复制原厂/社区实现或资产，没有启动Cindy设备功能。来源：[sdkResolver](https://github.com/makecindy/cindy/blob/5368875f4bc5d14f1a94387acf53aca3403e4e1b/apps/desktop/src/main/worklouder-codex/sdkResolver.ts)、[宿主](https://github.com/makecindy/cindy/blob/5368875f4bc5d14f1a94387acf53aca3403e4e1b/apps/desktop/src/main/worklouder-codex/workLouderCodexHostProcess.ts)、[生命周期](https://github.com/makecindy/cindy/blob/5368875f4bc5d14f1a94387acf53aca3403e4e1b/apps/desktop/src/main/worklouder-codex/WorkLouderCodexHostClient.ts)、[设置](https://github.com/makecindy/cindy/blob/5368875f4bc5d14f1a94387acf53aca3403e4e1b/apps/desktop/src/main/worklouder-codex/settingsStore.ts)。对应R-N05/R-D12/R-E07/R-K05。
- **E-070 / DECIDED**：用户确认先整理设备识别与只读证据，再实现稳定ID、结构化动作和本地持久化；仍不接自动化、不写设备、不猜未知协议。对应R-E07/R-E08/R-K05。
- **E-071 / SEEN-O记录复核**：本轮重读本机`.scratch/firmware-check/result.txt`，与E-057相符：2026-11-02独立USB探针一次sys.version，匹配响应，固件0.6.3；无配置读取/写入。本轮未重放探针，未打开真实设备；不能升级为Rust应用验收。E-052–E-060仍为真实读取的证据边界。

当前Git基线已由PR #12合并至1c8e4f5；E-064是当时历史状态，不再代表当前main。S1a离线工作台已合并，真实读回仍未完成。S2a实现验收入口见BUILD_PROMPT，硬件UNKNOWN-01/03/04/06保持原状态。

- **E-072 / IMPLEMENTATION**：S2a本地文档实现及验收，见BUILD_PROMPT AC-L01–L05。新建Rust核心实例与原生应用退出重开均恢复保存值；不代表硬件或工具适配验收。持久格式首版为v1，没有旧持久格式迁移事实。

- **E-073 / DECIDED**：2026-11-02 用户在 EGM 实施准备审查后明确“全部接受，按提案更新”。批准唯一 reader/读取事务/测试入口、v2兼容与比较语义、条件依赖修订、工具最低动作集、预览恢复/映射来源/手动覆盖及本地CLI方向。对应[实施设计](../implementation-plan.md)、R-D13/R-E09–R-E11/R-K06–R-K08/R-X04和AC-R01–R11。仅为产品及技术设计决定，不是设备观察，不提升任何协议UNKNOWN的证据等级；本次不接自动化、不写设备、不执行宿主动作。

- **E-074 / DECIDED**：用户调用 implement-spec，要求在统一集成分支实现全部 35 张工单，困难处参考 GitHub 相似实现。允许按依赖推进实现与验证；继续遵守来源/许可、未知协议不猜测及设备映射保护。不会以写入试探代替协议证据；任务自动化仍不接入。本条是执行范围更新，不是任何硬件能力已完成的证据。

- **E-075 / OFFICIAL + DEPENDENCY-SOURCE**：2026-11-02 核对 Apple IOKitUser `323ead896d04424f87184d8f6ff0cce811aab106` 的 `IOHIDDevice.h` SetReport 注释，以及 Cargo.lock 锁定的 hidapi crate **2.6.7** 随包 `etc/hidapi/mac/hid.c`：`set_report` 仅 report ID=0 时去掉占位首字节，非零 ID 将原 buffer/长度及首字节 ID 传给 IOKit；`hid_report_callback` 原样复制 callback buffer，不再插入 ID。`src/lib.rs` 的 macos-shared-device feature 调用 `hid_darwin_set_open_exclusive(0)`；`hid_close` 注销回调、唤醒并 join read thread 后释放资源。此版本采用 run loop，不套用 dispatch cancel-handler 模式。本项目 adapter 为编号0x06的完整64字节，生产候选限303A:8298/USB/macOS；固件0.6.3长帧仍待应用验收。来源：[Apple API](https://github.com/apple-oss-distributions/IOKitUser/blob/323ead896d04424f87184d8f6ff0cce811aab106/hid.subproj/IOHIDDevice.h#L642-L663)、[hidapi crate 2.6.7](https://crates.io/crates/hidapi/2.6.7)。只提取 API 事实，未移植实现。
- **E-076 / COMMUNITY**：独立研究固定 cm2-probe `1cb03f970bc86061be903fdd49f534e3fdaf0862` 的 [rpc.rs](https://github.com/panghy/cm2-probe/blob/1cb03f970bc86061be903fdd49f534e3fdaf0862/src/rpc.rs) 与 README：旧0.6.0设备可发带id的host.focused_app请求。因此入站先区分method/m与result/error，带id请求不满足pending，MVP既不执行也不回复。Dispatch固定提交E-059支持完整JSON对象及连续对象/LF/CRLF边界。均属社区/版本边界；当前固件同ID跨进程冲突与共享channel交错继续UNKNOWN-17。对应R-D05/R-D13。

- **E-077 / SEEN-O（受控身份验收）**：2026-11-02 当前303A:8298/USB、锁定hidapi2.6.7应用核心只读查询，观察到回复包络键为 `id/method/result`，method为此次只读查询名，result为非null。未记录params/result内容或原始载荷。固件可echo method；它与仅含method+id的设备请求必须区分。接收端以result/error恰一项、合法ID及可选echo method匹配pending method判为响应；带method但无result/error仍仅作未执行的设备请求/通知。此条只解锁包络合成回归，不代表查询/版本或完整S1已通过。

- **E-078 / IMPLEMENTATION + SEEN-O**：2026-11-02 T02只读应用验收：原生开发构建（ad-hoc、macOS/arm64）经SwiftUI“查找→明确选择8298 USB→连接→读取身份”，Rust/UniFFI/hidapi2.6.7实际读回version **0.6.3**、profile_index **0**、layer_index **1**，来源device.status。连接阶段未隐式读取配置；下方仍明确为Demo。另同一高层核心的opt-in native_identity_query_only通过并输出同版本/backend/source；不是独立探针。结束点击断开并退出，未读取配置/激活/灯光/写入/宿主执行、未修改权限或终止Input/Codex。原有映射物理行为、任意长响应/UTF-8、错误/拔插/睡眠和共存仍未验收。对应AC-1.5；不签收完整S1/AC-1.10。

- **E-080 / SEEN-O，原生物理拔插**：2026-11-02，集成55f49f6的生产开发构建，隔离文档和限时验收实例。用户按提示拔出/插回Creator Micro USB；CUA观察界面在不点击刷新时自动从“发现1个USB候选”→“无USB候选设备”→“发现1个USB候选”，重新出现后仍要求明确选择。实例通过菜单退出，进程结束。本次未点击连接/身份/配置，无RPC或设备写入；仅证明AC-1.3发现插拔，不证明已连接会话恢复或原有映射行为。
