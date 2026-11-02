# BUILD PROMPT（交给 Codex，一次性自包含）

> 复制下面整段作为任务提示。规格与证据在同目录 `SPEC.md`、`EVIDENCE.md`；遇到冲突以 SPEC 为准。

---

你在仓库 `micro-input`（macOS，Apple Silicon）里实现 **Micro Input** 的第一个可运行版本：Work Louder Creator Micro 2 的独立配置查看器。Rust 负责模型、协议和设备会话，SwiftUI 负责原生界面。先读 `README.md`、`AGENTS.md`、`lessons.md`、`docs/development.md`、`docs/spec/SPEC.md`。

## 目标

1. 可重复构建、启动的 macOS 应用；`scripts/check` 一条命令构建 Rust workspace 与 Swift 应用，并跑全部测试。
2. 无设备时：用**合成**夹具（自己编写的数据）跑通"解析 → 桥接 → SwiftUI 浏览 Profile / Layer / 13 键 / 旋钮 3 槽 / 摇杆扇区摘要"，界面明确标"演示数据"。
3. 有设备时：通过 USB 发现 Creator Micro 2，显示 OS 给出的身份；只读身份查询与配置读取的代码路径已就位，但被未知协议占位挡住（见"UNKNOWN 处理"），不得猜测实现。

## 硬约束（clean-room 与安全）

- 不得复制、引用或改写原厂 Input 应用、`@worklouder/wl-device-kit`、任何厂商 asar／解包物中的代码、图片、字体、wasm、文案。只依据 `SPEC.md` 的文字描述实现。界面、图标、文案全部原创。
- 不读取、不导入、不修改原 Input 的本地数据（`~/Library/Application Support/input/` 下任何文件）。夹具全部手写合成。
- 不向设备发送任何写入、删除、事务、灯光预览、bootloader、selftest 方法；方法白名单在编译期固定（见下）。
- 不独占打开 HID 设备；检测到设备被占用时报错，不抢占，不结束其他进程。
- 不 commit 用户数据、日志、真实设备回包原文。
- 不请求辅助功能或自动化权限；本阶段只涉及输入监控（HID 读写）的状态检查与引导。

## 栈

- Rust stable，Cargo workspace；`thiserror`、`serde`/`serde_json`（保留未知字段用 `serde_json::Value` 或 `#[serde(flatten)]`）、`tokio`（或等价的执行器）。
- Rust → Swift 桥：UniFFI（推荐）或 swift-bridge，二选一并写进文档；Rust 构建为静态库 / XCFramework。
- macOS HID：Rust 侧用 `IOKit` 绑定（`io-kit-sys` / `core-foundation`）或 `hidapi`（非独占模式），选型需在文档说明理由。
- SwiftUI（macOS 14+ 可接受；若选更低版本写明）。Xcode 工程或 SwiftPM，`xcodebuild` 可无界面构建。

## 模块与接口

```
crates/
  mi-model     配置类型 + 解析（保留原始值）+ 校验 + 未知标记
  mi-proto     trait Framer, trait RpcCodec, RpcRequest/RpcResponse/Notification, MethodAllowlist
  mi-transport trait HidTransport + MockTransport + macOS 实现（发现/打开/读写 report）
  mi-session   DeviceSession 状态机、串行队列、超时、断连失效、快照读取
  mi-ffi       给 Swift 的高层 API 与事件回调
app/MicroInput  SwiftUI：DeviceListView、ProfileBrowser、LayerView、BindingDetail、PermissionGuide、StatusBar
scripts/check
```

关键接口（名字可调，语义不可变）：

- `mi-model`
  - `DeviceConfig { source: Source(Demo|DeviceSnapshot|Imported), read_at, firmware: Option<String>, profiles: Vec<Profile>, raw: Value }`
  - `Profile { id: ProfileId, index: usize, name, layers }`——`id` 与 `index` 必须分开；"活动 Profile"字段语义未证实，显示为 Unknown。
  - `Layer { id, name, color, keys: [Binding; 13]（行长 2/4/4/3）, encoders: [Binding; 3]（逆时针/顺时针/按下）, joystick: Joystick{ kind: Raw/Known, sectors }, lights: Lights{ backlight, underglow } }`
  - `Binding = Basic(Token) | Macro(Ref) | Multi(Ref) | Smart(Ref) | Empty | Unknown(Value)`；`Token` 是字符串新类型，未知 token 原样保留。
  - `Macro { events: Vec<{ token, delay_ms, act: Release|Press|Click|Unknown(i64) }> }`，`MultiAction { tap, double_tap, hold, tap_hold, tapping_term_ms }`；数值不设上下限校验（范围 UNKNOWN），只拒绝类型错误。
  - `parse_config(bytes) -> Result<DeviceConfig, ParseError>`：畸形输入返回可读错误，不 panic。
- `mi-transport`
  - `trait HidTransport { fn enumerate(&self) -> Vec<HidCandidate>; fn subscribe(&self, tx); fn open(&self, id) -> Result<Box<dyn HidLink>>; }`
  - `HidCandidate { vid, pid, usage_page, usage, transport_hint: Usb|Bluetooth|Unknown, product, path }`
  - 识别：VID 0x303A，PID ∈ {0x8297, 0x8298}（Creator Micro 2），usage page 0xFF00；只把这些标为"候选"，其他忽略。USB/BLE 判定规则 UNKNOWN → `transport_hint` 先用 IOKit `Transport` 属性，取不到为 Unknown。
  - 变化通知去抖约 175 ms 后重新枚举。
  - `MockTransport`：可编程的连接/断开/多候选事件、可注入回包与通知、内存文件表。
- `mi-proto`
  - `trait Framer { fn encode(&self, payload: &[u8]) -> Vec<Report>; fn feed(&mut self, report: &[u8]) -> Vec<Line>; }`——**唯一实现是 `UnimplementedFramer`，所有调用返回 `ProtoError::Unverified`**，外加只在测试里用的 `TestFramer`（自定义、明确标注不是设备格式）。
  - `trait RpcCodec`：请求为 JSON 对象 `{ method, params, id }`，`id` 为 [0, 999) 内整数；响应按 id 匹配；无 id 且有 `method`（或简写 `m`）的入站对象为通知。错误对象结构 UNKNOWN → 保留原始 JSON。
  - `MethodAllowlist`：仅 `sys.version`、`device.status`、`fs.list`、`fs.read`、`fs.readbin`。其他方法名在类型层面无法构造（用 enum，不接受任意字符串）。
- `mi-session`
  - 每设备一个串行队列，请求间留约 50 ms；可配置超时（默认值标 TODO）；断连时挂起请求全部以 `Disconnected` 结束、队列与重组缓冲清空、会话作废；新连接生成新会话 id，旧会话的迟到响应丢弃。
  - `read_identity()`、`read_config_snapshot()`：后者流程为 list → 确认存在 `keymap.json` → 读 `keymap.json` 与 `smart_actions.json` → 完整解析成功才替换视图；失败保留旧快照并标"已过期"。
- `mi-ffi`：`list_devices()`、`subscribe_device_events(cb)`、`connect(id)`、`read_identity()`、`read_config()`、`load_demo()`、`input_monitoring_status()`；全部异步，不阻塞主线程。
- SwiftUI：设备列表（无设备 / 已连接 / 已断开 / 失败 / 占用）；Profile→Layer→键位图浏览；绑定详情显示 token 或引用名，Unknown 明确标注；权限引导页（输入监控未授权时，打开系统设置 Privacy_ListenEvent 面板、重新检查）；状态栏显示数据来源与读取时间。选中 Profile 只改 UI 状态，绝不触发设备请求。

## UNKNOWN 处理（必须照做）

以下每项都用可替换的 trait / enum 占位，加 `// TODO(UNKNOWN-xx): ...` 注释，并在 `docs/spec/UNKNOWNS.md`（新建）列出对应 TODO 与所需证据。**不得**按社区资料或猜测填写字节布局。

- UNKNOWN-01 HID report 布局：report ID 数值、header 字段偏移、channel 编号、长度字段含义、行结束规则（`Framer`）。
- UNKNOWN-02 超时与重试默认值（`SessionConfig`）。
- UNKNOWN-03 RPC 错误对象结构、`sys.version`／`device.status` 响应 schema（解析为 `Value` + 已知字段可选）。
- UNKNOWN-04 `fs.list`／`fs.read`／`fs.readbin` 的参数名、分块方式、校验和算法。
- UNKNOWN-05 USB 与蓝牙的判定，以及同一设备双通道同时在线的去重。
- UNKNOWN-06 设备文件 `keymap.json` 与本地缓存 schema 是否一致；活动 Profile 语义。
- UNKNOWN-07 宿主动作（文本、命令、URL、应用、径向菜单、Cheat Sheet、前台应用上报）的通知参数——本阶段只定义 `enum HostNotification` 与 `Unknown(Value)`，不实现执行。

`UnimplementedFramer` 导致真实设备的身份/配置读取返回"协议未验证"状态，UI 必须如实显示，不能退回演示数据冒充设备结果。

## 验收标准

1. `scripts/check` 在干净 clone 上通过：Rust 编译 + 测试、Swift 构建；写明实际验证的 macOS 版本、架构与工具链。
2. 应用能启动；演示模式可切换 Profile/Layer，13 键与 3 个旋钮槽的摘要由 Rust 解析结果驱动（不是 Swift 硬编码）。
3. 测试覆盖：非连续 Profile id、id 与 index 不一致、未知字段/未知 token 保留并标注、缺失引用、畸形 JSON 不崩溃。
4. `mi-session` 用 `MockTransport` + `TestFramer` 测：串行队列与间隔、id 匹配、通知穿插、错 id、畸形响应、超时、读取中断连、重连后旧响应被丢弃、重复点击不产生并发请求。
5. 白名单测试：除五个允许方法外无法构造请求（编译期或类型层面），并有测试证明页面导航/选中 Profile 不发任何请求。
6. 真实设备（如在场）：插拔时设备列表实时更新，仅凭 VID/PID/usage page 识别；不发送任何 RPC（因 Framer 未实现）；输入监控未授权时进入引导页。无设备时在报告里写明"硬件验收未完成"。
7. 仓库内无厂商代码、资源或用户数据；新增 `docs/spec/UNKNOWNS.md` 与代码中的 TODO 一一对应。
8. 交付说明：构建/运行命令、桥接选型理由、HID 库选型理由、未完成项清单。
