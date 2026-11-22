# Micro Input 开发状态与验证索引

面向 Work Louder Creator Micro 2 的独立 macOS AI Coding 工作台与原生硬件配置器，主页介绍见 [README](../README.md)。项目持续交付范围与接续入口见 [持续交付入口](autonomous-delivery.md) 与 [#44](https://github.com/semantic-craft/micro-input/issues/44)。

## 当前版本与架构基线

- **最新发行标签**：[v0.12.1](releases/v0.12.1.md)（`CFBundleShortVersionString = 0.12.1`；历次发行说明归档于 [releases/](releases/v0.12.1.md)）。
- **主线演进（v0.12.1 基线）**：合入 Antigravity 后台日志存活感知、自动重试/60 秒慢等/`429` 报错与守护进程断连异常呼吸灯效及双语悬停原因提示（PR #173），延续 v0.12.0 响应驱动双源状态灯、存量预设迁移与零普通注释门禁基线（PR #169、#170、#171）。
- **技术架构**：七个纯 Rust 内核 crate（`mi-proto`、`mi-transport`、`mi-model`、`mi-session`、`mi-bridge`、`mi-macos`、`mi-core`）+ `crates/mi-ffi` 9 函数 `extern "C"` 静态库 + 13 个原生 SwiftUI/AppKit 源文件（`app/MicroInput`），经 `CoreStore` `@Observable` + 12 个 `Equatable` 域切片驱动界面；静态与安全边界由 `scripts/check-boundaries.py` 强制执行 `R-C01` 至 `R-C15`。

## 已实现与已验证能力

### 1. 硬件协议、六层板载控制与固件更新

- **USB 独占与跨固件支持**：支持 Creator Micro 2（USB `303A:8298`）固件 `0.6.3` 与官方预发布 `0.7.0-rc.16`。所有写操作均先保存原字节与完整恢复链再写入、逐块读回核对；不静默改写设备 Flash。
- **六层物理路由与按层混合摇杆**：
  - 第 1 层（`Codex · Micro Input`）：接管全部 20 个控制位，顶部 `AG00–AG05` 独立承载 6 槽 Codex 会话状态灯与直达跳转，在 `0.7.0-rc.16+` 上摇杆默认采用硬件 `MOUSE` 模式；
  - 第 2 层（`Antigravity`）：顶部 `AG00–AG05` 为 6 槽会话状态灯与直达跳转，摇杆默认采用四向 `RADIAL`（上/下滚动对话、左划打开右边栏、右划关闭右边栏）；
  - 第 3 层（`DeepSeek Harness`）：顶部两键为 `Subagent` + `左键点击`（`AG02–AG05` 留空），摇杆默认采用硬件 `MOUSE` 模式；
  - 第 4–6 层：保留未分配。
- **响应驱动双源状态灯与 `0.7.0-rc.16` 能力**：`SharedVendorBridge` 通过 `v.oai.thstatus`（`AG00–AG17`）真实总线 ACK 独立同步第 1 层（`AG00–AG05` Codex）与第 2 层（`AG06–AG11` Antigravity）可行动会话槽位（第 3 层非会话键位保持熄灯，900 请求预轮转），默认保持运行中会话熄灯、未读完成常亮海洋蓝（`solid`），仅在会话或子代理待审批输入、Antigravity 后台日志检出自动重试/60 秒无进展/API 报错（如 `429`）或守护进程断连时呼吸琥珀金/珊瑚红并同步单路底光（`sa: true`），同时在工作台槽位与键帽悬浮提示展示具体原因；`0.7.0-rc.16` 支持 `mouse.config` 四档预设与滑块调校、`joystick.deadzone`（默认恢复出厂 `0.15`）、`kb.pl.set` 无写 Flash 实时切层、`pm.config`/`pm.state` 休眠状态读取及 `device.name` / `kb.sa.rgb` 动态灯效，详见 [0.7.0 进阶硬件能力](firmware-v070-capabilities.md)。
- **官方串口固件升级**：直接对接 `cm-v2-fw-releases` 校验 SHA-256、保存 16 MB Flash 备份并检查 `0x8000` 分区表，详见 [固件更新契约](firmware-update.md)。

### 2. 桌面应用控制、听写与工作台

- **4+1 桌面方案**：内置 `Antigravity`、`Codex · Micro Input`、`DeepSeek Coder (DSH)`、`Herdr` 与通配通用方案「其他软件」（默认映射苹果 `Fn` 键）；采用「六层板载路由 + 菜单栏手动切换 + 锁定当前方案」，前台应用自动跟随已依据 `R-C11` 退役。
- **旋钮模型选择器与轮盘菜单**：桌面 Agent 方案下左右旋钮直接原位展开并切换模型/推理力度，短按与长按统一触发会话搜索；软件模式下的「轮盘菜单」以回中离中机制驱动八向选择。
- **可控听写与无障碍聚焦**：区分按住说话（`Antigravity`、`DSH`，120 秒上限与原窗口/控件锁）与点击听写（`Codex`，停录读回后提交）；通过批读属性与定向 `AXChildren` 校验将 Antigravity 聚焦延迟降低 39.7%（见 [延迟测量记录](designs/host-input-latency.md)）。
- **本地文档、配置编辑与日常体验**：支持 `v2` 原子持久化、结构与动作差异审阅、整次撤销/重做、`1U+1U`/`2U` 底行合并（`WideKeyMerger`）、菜单栏 `⇧⌘M` 弹层、中英双语即时切换、`SMAppService` 开机静默驻留、Sparkle Ed25519 自动更新与 `mi draft` CLI 提案审阅。

## 调研入口与设计契约

- **协议与架构研究**：[业务架构与证据定位](input-architecture.md)、[只读协议说明](protocol-readonly.md)、[写入与宿主通知候选协议](protocol-candidates.md)、[InputKit 基础层与单设备写入预算](input-kit-foundation.md)、[模块契约](module-contracts.md)。
- **产品与桌面适配**：[桌面应用控制](designs/four-app-control.md)、[桌面应用接口](four-app-interfaces.md)、[原生工作台设计](designs/creator-micro-workbench.md)、[单色视觉规范](designs/DESIGN.md)、[运行时一致性](designs/runtime-consistency.md)、[Antigravity 会话与导航](../integrations/antigravity/README.md)、[DSH 零补丁适配](../integrations/dsh/README.md)。
- **规格与台账**：[SPEC](spec/SPEC.md)、[BUILD_PROMPT](spec/BUILD_PROMPT.md)、[EVIDENCE](spec/EVIDENCE.md)、[UNKNOWNS](spec/UNKNOWNS.md)。

## 验证入口与已知未决边界

```sh
scripts/check            # Python 单测 + cargo fmt + Rust/Swift 全量测试 + 构建 + R-C01..R-C15 门禁
MI_AUTO_HOST_READY=0 scripts/verify-offline  # 全量检查 + 6 个隔离合成 QA 场景与清理校验
```

- **已知代码与硬件边界**（合并与离线通过不替代以下验收）：
  - **灯光模态框预览入口**：工作台到会话层 `Preview::begin_native` 的实时预览接线（含 `LightingPreviewState`、`VendorBridgePreviewLease` 冲突抑制及关闭/切换/断连/睡眠排空）已在源码与离线测试收口（见 [模块契约](module-contracts.md#rgb-编辑与合成预览)）；实体物理 LED 视觉验收与结束预览后的运行态回读/自动恢复边界（[#24](https://github.com/semantic-craft/micro-input/issues/24) / [#44](https://github.com/semantic-craft/micro-input/issues/44)）仍属实机验收范围，不以协议 ACK 替代。
  - **BLE 传输**：Vendor `0xFF00` 配置读写与写 Flash 当前仅对 USB（`TransportHint::Usb`）放行；蓝牙模式仅作正常 HID 键盘/鼠标工作，写键位或更新参数需插线走 USB。
  - **实机持续验收**：[#44](https://github.com/semantic-craft/micro-input/issues/44) 的完整端到端实体按键、多屏、灯光视觉与各宿主应用升级后的回归继续按 [开发与验证入口](development.md)、[实机验收准备](designs/hardware-acceptance.md)、[桌面控制验收](verification/desktop-agent-controls.md) 与 [听写验收](verification/push-to-talk.md) 登记。
