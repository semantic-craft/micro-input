# Micro Input 开发状态与验证索引

面向 Work Louder Creator Micro 2 的独立 macOS AI Coding 工作台与原生硬件配置器，主页介绍见 [README](../README.md)。项目持续交付范围与接续入口见 [持续交付入口](autonomous-delivery.md) 与 [#44](https://github.com/semantic-craft/micro-input/issues/44)。

## 当前版本与架构基线

- **最新发行标签**：[v0.11.0](releases/v0.11.0.md)（`CFBundleShortVersionString = 0.11.0`；历次发行说明归档于 [releases/](releases/v0.11.0.md)）。
- **主线演进（v0.11.0 基线）**：已合入按层混合硬件摇杆模式、`Antigravity` 顶部 6 键会话状态与摇杆左/右划开关右边栏、`Codex` / `DeepSeek Harness` 顶部 `Subagent` + `左键点击`、固件 `0.7.0-rc.16` 鼠标曲线与出厂 `0.15` 防漂移死区默认值回调、真实 `workbench.json` 残留槽位自动迁移，以及三阶段架构收口（生产 `qa-runner`/`mi-bridge` 隔离、`store`/`six_layer_runtime` 模块化拆解、全量 Clippy 清零、`CoreStore` `Codable` DTO 与 Swift `-strict-concurrency=complete`）。
- **技术架构**：七个纯 Rust 内核 crate（`mi-proto`、`mi-transport`、`mi-model`、`mi-session`、`mi-bridge`、`mi-macos`、`mi-core`）+ `crates/mi-ffi` 9 函数 `extern "C"` 静态库 + 13 个原生 SwiftUI/AppKit 源文件（`app/MicroInput`），经 `CoreStore` `@Observable` + 12 个 `Equatable` 域切片驱动界面；静态与安全边界由 `scripts/check-boundaries.py` 强制执行 `R-C01` 至 `R-C14`。

## 已实现与已验证能力

### 1. 硬件协议、六层板载控制与固件更新

- **USB 独占与跨固件支持**：支持 Creator Micro 2（USB `303A:8298`）固件 `0.6.3` 与官方预发布 `0.7.0-rc.16`。所有写操作均先保存原字节与完整恢复链再写入、逐块读回核对；不静默改写设备 Flash。
- **六层物理路由与按层混合摇杆**：
  - 第 1 层（`Codex · Micro Input`）：接管全部 20 个控制位，在 `0.7.0-rc.16+` 上摇杆默认采用硬件 `MOUSE` 模式；
  - 第 2 层（`Antigravity`）：顶部 `AG00–AG05` 为 6 槽会话状态灯与直达跳转，摇杆默认采用四向 `RADIAL`（上/下滚动对话、左划打开右边栏、右划关闭右边栏）；
  - 第 3 层（`DeepSeek Harness`）：顶部两键为 `Subagent` + `左键点击`（`AG02–AG05` 留空），摇杆默认采用硬件 `MOUSE` 模式；
  - 第 4–6 层：保留未分配。
- **逐键状态灯与 `0.7.0-rc.16` 能力**：`SharedVendorBridge` 通过 `KV_OAI_AG06–AG17` 与 `v.oai.thstatus` 真实总线 ACK 同步呼吸绿光与海洋蓝未读灯效（900 请求预轮转）；`0.7.0-rc.16` 支持 `mouse.config` 四档预设与滑块调校、`joystick.deadzone`（默认恢复出厂 `0.15`）、`kb.pl.set` 无写 Flash 实时切层、`pm.config`/`pm.state` 休眠状态读取及 `device.name` / `kb.sa.rgb` 动态灯效，详见 [0.7.0 进阶硬件能力](firmware-v070-capabilities.md)。
- **官方串口固件升级**：直接对接 `cm-v2-fw-releases` 校验 SHA-256、保存 16 MB Flash 备份并检查 `0x8000` 分区表，详见 [固件更新契约](firmware-update.md)。

### 2. 桌面应用控制、听写与工作台

- **4+1 桌面方案**：内置 `Antigravity`、`Codex · Micro Input`、`DeepSeek Coder (DSH)`、`Herdr` 与通配通用方案「其他软件」（默认映射苹果 `Fn` 键）；采用「六层板载路由 + 菜单栏手动切换 + 锁定当前方案」，前台应用自动跟随已依据 `R-C11` 退役。
- **旋钮模型选择器与轮盘菜单**：桌面 Agent 方案下左右旋钮直接原位展开并切换模型/推理力度，短按与长按统一触发会话搜索；软件模式下的「轮盘菜单」以回中离中机制驱动八向选择。
- **可控听写与无障碍聚焦**：区分按住说话（`Antigravity`、`DSH`，120 秒上限与原窗口/控件锁）与点击听写（`Codex`，停录读回后提交）；通过批读属性与定向 `AXChildren` 校验将 Antigravity 聚焦延迟降低 39.7%（见 [延迟测量记录](designs/host-input-latency.md)）。
- **本地文档、配置编辑与日常体验**：支持 `v2` 原子持久化、结构与动作差异审阅、整次撤销/重做、`1U+1U`/`2U` 底行合并（`WideKeyMerger`）、菜单栏 `⇧⌘M` 弹层、中英双语即时切换、`SMAppService` 开机静默驻留、Sparkle Ed25519 自动更新与 `mi draft` CLI 提案审阅。

## 调研入口与设计契约

- **协议与架构研究**：[业务架构与证据定位](input-architecture.md)、[只读协议说明](protocol-readonly.md)、[写入与宿主通知候选协议](protocol-candidates.md)、[模块契约](module-contracts.md)。
- **产品与桌面适配**：[桌面应用控制](designs/four-app-control.md)、[桌面应用接口](four-app-interfaces.md)、[原生工作台设计](designs/creator-micro-workbench.md)、[单色视觉规范](designs/DESIGN.md)、[运行时一致性](designs/runtime-consistency.md)、[Antigravity 会话与导航](../integrations/antigravity/README.md)、[DSH 零补丁适配](../integrations/dsh/README.md)。
- **规格与台账**：[SPEC](spec/SPEC.md)、[BUILD_PROMPT](spec/BUILD_PROMPT.md)、[EVIDENCE](spec/EVIDENCE.md)、[UNKNOWNS](spec/UNKNOWNS.md)。

## 验证入口与已知未决边界

```sh
scripts/check            # Python 单测 + cargo fmt + Rust/Swift 全量测试 + 构建 + R-C01..R-C14 门禁
MI_AUTO_HOST_READY=0 scripts/verify-offline  # 全量检查 + 6 个隔离合成 QA 场景与清理校验
```

- **已知代码与硬件边界**（合并与离线通过不替代以下验收）：
  - **灯光模态框预览入口**：工作台 `LightingBeginPreview` 当前只更新界面预览计时状态，尚未接入物理设备 `lights.preview` 发送（会话层已有独立原语与合成验证，见 [模块契约](module-contracts.md#rgb-编辑与合成预览)）。
  - **BLE 传输**：Vendor `0xFF00` 配置读写与写 Flash 当前仅对 USB（`TransportHint::Usb`）放行；蓝牙模式仅作正常 HID 键盘/鼠标工作，写键位或更新参数需插线走 USB。
  - **实机持续验收**：[#44](https://github.com/semantic-craft/micro-input/issues/44) 的完整端到端实体按键、多屏、灯光视觉与各宿主应用升级后的回归继续按 [开发与验证入口](development.md)、[实机验收准备](designs/hardware-acceptance.md)、[桌面控制验收](verification/desktop-agent-controls.md) 与 [听写验收](verification/push-to-talk.md) 登记。
