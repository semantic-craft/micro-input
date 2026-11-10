# 当前实施入口

当前范围见[桌面应用控制](designs/four-app-control.md)。适配入口见[桌面应用接口](four-app-interfaces.md)，接续见[接力队列](pstack-relay.md)。

接手未完成工单或产品改动时，先执行[研究成果接入规则](agents/research-context.md)。其中包含 Input 1.1.0 的证据索引、按功能阅读路径和可直接使用的接续提示词；实际工单状态以 GitHub 当前记录为准。

PR #12/#13 已交付离线原生工作台、稳定文档 ID、结构化动作与 Rust 本地持久化。设备发现、身份查询及 #5 有界配置读取已实现；当前0.6.3的核心读回和原生只读浏览见E-079。#6完整生命周期、其他固件和物理映射仍待验收，未知后端继续禁止发送。

本地v2文档、v1原字节备份迁移、Profile/Layer管理和真实快照显式创建独立草稿已实现，证据见E-083；三方比较已实现，身份仅有临时接口时保留明确的整份选择，自动合并保持禁用；设备应用当前范围见 E-108。

先读 [实施设计](implementation-plan.md) D01–D07 与 [BUILD_PROMPT](spec/BUILD_PROMPT.md) 的 S1，再按对应工单推进。完整产品与依赖入口为 [#44](https://github.com/semantic-craft/micro-input/issues/44)。当前继续开发本地功能与适用验证；设备操作遵守证据和恢复门槛，不因规划批准而解锁宿主执行或自动化。

# 开发起点

## 已确定的产品方向

为用户自己的 Creator Micro 2 做独立 macOS 应用，Rust 承担配置模型、协议与核心逻辑，SwiftUI 承担原生界面。目标是改善原 Input 的使用体验与性能；性能问题尚未量化，不能把技术栈本身当成性能验收。

## 兼容能力范围

需要逐步覆盖 Profile / Layer、基础按键与旋钮、普通宏、四分支 Multi Action、灯光两区、径向菜单、AppSense、Cheat Sheet，以及宿主执行的 Text / Command / URL / App Smart Action。设备保存的行为与需要宿主常驻的行为分开建模。

本机观察版本为 Input 1.1.0、设备固件 0.6.3；不代表其他版本。普通层灯光有 Off/Solid/Snake/Rainbow/Breath/Gradient 六类。AppSense 通过目标应用前台停留 5 秒采集。具体证据和来源见 README 的调研入口。

## 首轮开发范围

首版为 USB 只读配置查看器，范围与可勾选验收项统一维护在 [MVP 总工单 #1](https://github.com/semantic-craft/micro-input/issues/1) 及其子工单，不在本地重复维护工单状态。初始化基础已由 PR #12/#13 提供；下一步是 [#3](https://github.com/semantic-craft/micro-input/issues/3)，不重复初始化工程。

模拟数据用于建立可运行技术链路，不等于设备验收。身份和配置读回须在当前实机验证；原 Input 缓存不能充当设备快照。首版不含设备配置写入或宿主动作执行，后续写入仍需独立的备份、提交、读回及恢复设计。

## 未解决的问题

### Dispatch 证据进入开发的顺序

依据及逐项待验边界见[协议第 7 节](protocol-readonly.md#7-dispatch-补充证据与当前固件核验)。Dispatch 0.6.2 的社区实测与本机 0.6.3 的事实分开记录；工单验收仍以当前设备为准。

1. **#3 设备发现**：检查 device usage pairs 与 primary usage 的区别；保持 USB、VID/PID 和多候选选择约束。枚举不请求 Input Monitoring。
2. **#4 身份与 RPC**：核对 report ID 的后端适配、JSON 对象/行结束边界、长短字段、外来 ID 与迟到响应；连接失败区分权限与 Secure Input。本次独立探针已读回 0.6.3，但不能据此关闭应用身份票。
3. **#5 配置查看**：继续采用 fs.readbin、大小限制及前后校验；把 fs.read 留作社区候选。加入 [2,4,4,3] 行宽、宽键双开关、旋钮方向、未知绑定和非连续 ID 的合成夹具，再与真实配置及物理布局核对。
4. **#6 生命周期**：验证 manager/缓冲区寿命、断连取消、重连新会话、Input/Codex 共存和退出后重新打开。ID 范围不能替代跨进程冲突检查。
5. **MVP 之后**：再设计 BLE、宿主事件、宽键去重、Codex 灯光共存与配置写入/恢复；不因发现社区可调用方法而开放写入口。

2026-11-02 固件核验：独立 USB `sys.version` 返回 **0.6.3**，官方 latest 同为稳定版 **0.6.3**，无需升级；用户选择稳定通道，不安装 0.7 RC。本次没有配置读取与写入，映射行为验收仍归后续实机任务。

开源社区参考仓库提供候选设计线索，不自动扩大 MVP 或设备写入范围。

### 仍待验证

- 当前版本的读协议已整理到 [只读协议说明](protocol-readonly.md)：发现条件、HID 分帧、状态回退、分块读取和缓存边界已有静态证据；直接 IOKit/Rust 后端的报文适配和实机 RPC 仍待验证。
- 厂商没有在已检索资料中给出完整、版本化协议规范；社区项目的型号、固件、USB/BLE 和 HID 报文布局不同，不能直接混用。
- 灯光滑杆的 min/max/step、Magic 含义、径向扇区上限、Joystick 模式条件，以及宏容量等仍未证实。
- 未保存草稿中的数值显示不能当固件范围；Multi 初始 250ms、Delay 初始 200ms 只是观察值。
- 官方 7 工具与社区 8 扇区（含取消）的关系尚未通过边界实验验证。
- 性能根因、后台 CPU/内存、事件延迟、USB/BLE 重连、设备读回与异常恢复尚未测试。

## 来源与实现边界

已有材料包含官方文档、本机黑箱观察及带来源的社区公开研究，部分社区材料源自作者分析旧版应用；2026-11-02 按用户授权新增 Input 1.1.0 随包 JavaScript 静态分析，未执行原模块或反编译原生二进制。当前没有严格 clean-room 隔离证明。原始材料留在 .scratch，独立实现只接收整理后的协议说明和合成向量；不复制标注 UNLICENSED 的 device-kit 实现。新界面使用原创设计与素材，社区代码或模型按其许可和版本核查后再使用。

当前应用实施与验收状态以 MVP 工单为准；本页的独立版本查询不代表应用已经完成。原有映射仍需保留；之前的临时 Profile 切换授权不等于任意设备写入、刷固件或安装替代驱动的授权。

## 应用与菜单栏图标

应用使用已选定的薄荷绿短发角色原画 `app/MicroInput/Resources/AppIcon-artwork.png`。`scripts/build` 调用 `scripts/generate-app-icon.swift` 生成 16–1024 px 的图标表示并打包为 AppIcon.icns；圆角和 5% 透明外边距与 Koe 的 macOS 图标一致。原画不带截图外框。

菜单栏由 `StatusItemIcon.swift` 绘制 20 × 18 pt 的键盘与旋钮符号，设置 `isTemplate`，由系统处理深浅色及选中着色。尺寸是本项目选择，不是 Apple 强制尺寸。遵循 [Apple 菜单栏规范](https://developer.apple.com/design/human-interface-guidelines/the-menu-bar) 的黑色/透明符号建议及 [NSImage.isTemplate](https://developer.apple.com/documentation/appkit/nsimage/istemplate) 的模板语义。

## 有界只读输入观测

`scripts/observe-input`（或 `--dry-run`）默认只跑合成测试，原生测试保持 ignored。需要一次由操作者配合的物理观测时，显式运行 `scripts/observe-input --observe-once`。只复用生产 `MicroInput` / `DeviceSession` 的描述符、身份、完整性校验配置读取及唯一 reader 输入订阅；不激活 Profile/Layer、不写配置、不执行宿主动作、不申请权限或终止其他应用。

看到并刷新输出的 `READY_FOR_PHYSICAL_INPUT` 后，操作者才进行本轮约定的控件操作；被动窗口固定为 45 秒，事件上限 2048，gap/关闭/断连均停止订阅。准备阶段上限 100 秒，结束后状态复查上限 15 秒、shutdown join 上限 5 秒；外层启动器总预算 180 秒（包含构建），超时只停止并等待自己创建的进程组，不重试。可先运行默认入口完成构建。

每次生成独立 experiment ID 与开始时间；0700 的 `.scratch/input-study/input-observation-<ID>/` 下仅保存 0600 的脱敏事件和启动器状态。记录事件分类、语义 binding ID、时间、计数、session、gap；不保存通知、配置、宏、设备路径或 serial。报告的 `active_source_verified=false`、`active_ag_binding_count=null` 是当前证据边界：设备返回活动索引不证明它与源配置对象的对应语义。静默为 `silence_unconfirmed`，不能据此证明未按键、映射正确或硬件支持完成；有候选消息也只支持其通知形态，实体位置与其他消费者共存仍须另行核对。E-094 保留给实际观测后的证据登记，本入口的合成通过不占用实机证据。

## S3 离线验证入口

文档模型与候选：`cargo test -p mi-model --test lighting`；合成端到端：`cargo test -p mi-session --features synthetic-backend --test config_write`。后者仅用自编夹具、私有临时备份和编译的合成transport，不枚举/打开真实设备。合成写入器仅由显式 `synthetic_config_writer` 构造的高层工作流调用；普通合成实例不开放写入；原生接线已由 E-108 补齐。默认统一门禁仍为 `scripts/check`，原生探针继续ignored。

既有宿主回归使用 `scripts/build --synthetic` 后的 `MI_AUTO_HOST_READY=0 MI_SYNTHETIC_SCENARIO=host-execution scripts/run --verify --timeout=25`；该场景验证既有预设/宿主模拟未回退，不代表新增灯光UI或真实设备功能。生产离线启动必须设置 `MI_AUTO_HOST_READY=0`，本轮不启动生产应用。


持久记录回归：`cargo test -p mi-model --test operation_record`；合成恢复链路仍用 `cargo test -p mi-session --features synthetic-backend --test config_write`。覆盖首次发送前落盘、中断重开、损坏现状恢复、恢复链失败、新计划绑定、读回失败、核验结果未落盘、文档冲突和重复本地修复。`apply_observed` 是检查点观测接缝，不开放到 FFI；生产后端现经 E-108 的独立入口构造计划绑定 writer。完整检查使用 `MI_AUTO_HOST_READY=0 scripts/check`，真实硬件测试保持 ignored。


高层集成与恢复界面：`MI_AUTO_HOST_READY=0 swift test --package-path app/MicroInput -Xswiftc -DMI_SYNTHETIC_BACKEND --filter DeviceOperationsTests`（先构建合成绑定）。隔离应用场景 `MI_AUTO_HOST_READY=0 MI_SYNTHETIC_SCENARIO=config-write scripts/run --verify --timeout=55` 完成单键应用、完整读回、新计划恢复，并打开恢复记录面板。独立 `device-operations.jsonl` 只记录操作结果，实体键和视觉灯效固定 NOT_RUN。本地保存失败通过仅专用合成实例可用的故障开关注入，重开后用无设备会话的原生核心修复；不启动生产应用。


诊断导出：`MI_AUTO_HOST_READY=0 MI_SYNTHETIC_SCENARIO=diagnostic-export MI_DIAGNOSTIC_EVIDENCE_PATH=/absolute/report.json scripts/run --verify --timeout=45`。隔离应用打开报告，生成 0600 的 JSON 和 `evidence/diagnostics.json`；后者确认失败阶段存在，并把实体键和灯光视觉固定为 `NOT_RUN`。单元测试覆盖路径、serial 和 JSON 载荷脱敏，以及失败读取后的行动报告。

RGB 与全自动离线验收：`scripts/verify-offline` 是包含统一检查和八个 QA 场景的总入口；`--qa-only` 用于已完成单元门禁后的应用复验。每个应用场景 TTL 45 秒，启动器负责退出/清理，验证器另核对进程消失、回执数量及隔离目录移除。失败写入 FAIL 结果并非零退出；finally 恢复匹配的生产绑定/库/应用但不启动它。环境强制 `MI_AUTO_HOST_READY=0`，不运行任何物理探针。

性能基线准备：`scripts/perf-baseline` 构建合成应用后启动隔离 QA，记录从启动到 Workbench 首次 `onAppear` 的耗时，再采样默认 10 秒空闲 CPU time 增量与 RSS。`--seconds 120` 可延长观察；launcher 超时随观察预算增加，包含启动和停止余量。结果写入该轮 `evidence/result.json`，状态为 `MEASURED`；设备连接、实体键、灯光视觉、安装版和验收预算固定 `NOT_RUN` 或 `NOT_SET`。该命令只建立可复测基线，不签收 #42 的插拔、睡眠唤醒或轻量预算。

`cargo test -p mi-session --features synthetic-backend lighting_preview` 覆盖合成运行态与文件分离、最新值合并、切层/断连、失败未知状态、编号保留、30 秒上限及所有者丢弃恢复。`mi-core` 的 `inspector_and_editors` 回归覆盖两区六效果、参数可用性、非法值、单次全层撤销、重开、RGB 应用/恢复及预览不修改本地文档。恢复设备保留本地目标意图；测试分别检查当前设备快照和本地草稿，不把两者相等当成要求。

## 实机验收材料的离线准备

使用 `scripts/prepare-hardware-acceptance prepare` 生成新的私有准备包；`verify` 检查固定范围、权限及摘要，`compare` 比较调用方提供的本地原字节。工具没有设备I/O或执行入口，材料有效不等于设备备份或执行许可。最小实验、备份采集前提和恢复规则见 [实机验收准备](designs/hardware-acceptance.md)。回归 `test_hardware_acceptance_launcher.py` 随 `scripts/check` 运行，无需连接设备。

独立 H0/H1 执行器：`scripts/hardware-acceptance` 默认只跑假 USB；显式实机选项和一次性材料约束见上述设计。`cargo test --locked -p mi-session --test hardware` 覆盖完整备份、固定5块/删除、逐步保全及失败零重放；两个 native 用例保持 ignored。启动器系统边界回归随统一检查执行，不创建真实子进程或连接设备。

原生配置链（E-108）：`ConfigWriter::native` 与合成路径共用计划/记录/恢复；`native_plan_bound_writer_applies_and_restores_but_rejects_another_connection` 离线覆盖真实分支和连接nonce隔离，`multiframe_request_uses_one_transport_batch` 防止长请求回退到逐报告锁竞争。普通RPC出口继续只读。实机最小配置应用与原字节恢复的私有材料在 `.scratch/input-study/hardware-live/`；默认门禁不重跑设备变更。GUI/宿主回归仍使用 `MI_AUTO_HOST_READY=0 scripts/verify-offline`。

## 径向菜单

`RadialMenuState` 持有启用、等待回中、选择和确认后等待回中的状态。`RadialMenuRouting` 将冻结的菜单意图送入现有宿主队列及 lease，确认时重验同一会话和路由。原始角度/距离由 `mi-session` 验证并传递至 `mi-core`；连续位置资格独立于按键边沿。没有新硬件订阅或通用 RPC。

菜单默认关闭，本机偏好为 `radialMenuEnabled`。启用或恢复后的旧缓冲回中不能解除隔离。实现与合成原生验收见[径向菜单验证配方](../skills/verify-micro-input/features/radial-menu.md)。当前软件策略的距离阈值和八向手感仍待实体核验，`kb.radial` 没有因此解锁。
