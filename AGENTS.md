# Micro Input

- 五项持续交付范围与接续入口见 [持续交付](docs/autonomous-delivery.md)。完成一个 PR 后核对 #44 与下一可执行切片；代码合并不代表安装或实体效果验收。
- **机器自动验收与日常迭代策略**：功能实现默认以 `scripts/verify-offline`（含静态/单元门禁 + 六个合成隔离 QA 场景自动回执校验）全量通过作为交付准绳，通过后可直接更新本机日常版 `/Applications/Micro Input.app` 供实际使用，不因等待人工逐键验收阻塞交付；日常使用中发现的体验或边缘问题再按需迭代修复。
- **PStack 自动接力约定（A → B → C 跨对话推进）**：全景切片队列在 [docs/pstack-relay.md](docs/pstack-relay.md)，决策账本在 `.scratch/pstack-relay/decisions.tsv`。每次对话完成一个切片并通过验证后，必须运行 `scripts/pstack-relay advance <SLICE_ID> --evidence "<sha/receipt>" --copy`（若仅查询状态则运行 `scripts/pstack-relay status --copy`），并在回复末尾附上生成的 `## Next Chat Prompt (Copy & Paste)` 代码块，供用户直接粘贴到下一个新对话。

- 开工读 README.md、docs/development.md 和 lessons.md；当前首轮为离线工作台；阶段范围见 docs/spec/BUILD_PROMPT.md 的 S1a。
- 接手未完成工单、实现或评审产品改动前，必读 [研究成果接入规则](docs/agents/research-context.md)，按任务选读 Input 架构、候选协议和模块契约，并在交付中记录采用的证据与验证缺口。拆分任务或交接时传递该入口。
- 技术架构借鉴 [`metasequoiaime/msime`](https://github.com/metasequoiaime/msime) 的「纯 Rust 跨平台内核 + 薄 C-ABI 边界 + 原生宿主薄壳」设计：全部协议、模型、跨进程桥接、macOS 宿主执行与工作台状态机收敛在纯 Rust 内核（`crates/mi-proto`、`crates/mi-transport`、`crates/mi-model`、`crates/mi-session`、`crates/mi-bridge`、`crates/mi-macos`、`crates/mi-core`），经 7 函数极简 C-ABI 静态库（`crates/mi-ffi`）暴露给 100% 原生 macOS SwiftUI/AppKit 薄壳（`app/MicroInput`），彻底退役 UniFFI、`crates/mi-gpui` 及肥 Swift 业务层。
- 兼容功能或协议工作查 README 中对应调研原件，分别标明观察、官方说明、社区线索和未知；社区旧版参数不能充当当前固件规格。
- 真实设备操作保留已有映射。默认先做只读或脱敏夹具验证；涉及写入时依据当轮用户授权并验证恢复。Profile 选择在原 Input 中可能自动同步设备。
- 当前原始材料唯一位置是 .scratch/input-study，受 Git 忽略；正式实现结论进入源码或 docs，不提交用户宏载荷、应用数据库或原始日志。
- 后续在本项目维护事实；原 agent-tasks 入口只作跳转，不双写项目状态。

## Agent skills

### Issue tracker

工单与规格使用 GitHub Issues；读取或发布工单前见 [issue-tracker.md](docs/agents/issue-tracker.md)。

### Triage labels

采用五个默认分诊标签；分诊或修改标签前见 [triage-labels.md](docs/agents/triage-labels.md)。

### Domain docs

采用 single-context；探索领域模型或架构前见 [domain.md](docs/agents/domain.md)。

### Verification

应用交互验证与宿主执行闭环验收使用项目内置技能 [verify-micro-input](skills/verify-micro-input/SKILL.md)（驱动 `scripts/run --verify`、检查 `launcher.json` 与 `audit.jsonl` 并输出 `result.json`）。

## 规则与门禁对照表

| 规则 | 强制门禁 / 检查点 |
| --- | --- |
| 严禁硬编码用户家目录（`/Users/<name>`），只允许 `Path.home()` / 环境变量 / 配置发现 | `scripts/check-boundaries.py`（`R-C01`，随 `scripts/check` 执行） |
| 严禁向外部 Agent 维护侵入式补丁（`integrations/*/*.patch`），必须基于其开源代码与可观测行为在 `Micro Input` 侧直接适配 | `scripts/check-boundaries.py`（`R-C02`，随 `scripts/check` 执行） |
| 桌面 Agent 预设完整度唯一真源为 `WorkbenchStore::has_all_desktop_agent_presets`（派生自 `CANONICAL_DESKTOP_AGENT_PROFILES`），`prepare_desktop_profiles()` 幂等且不覆盖就绪状态 | `crates/mi-core/tests/workbench_store.rs`（`prepare_desktop_profiles_is_idempotent_and_preserves_ready_status`） |
| 生产出口只读门禁、物理分帧隔离与 `UNKNOWN-*` 清单对齐 | `scripts/check-boundaries.py` + `mi-proto` / `mi-session` 边界测试 |
| 构建 `target/MicroInput.app` 后注销临时包 LaunchServices 登记，且 `app/MicroInput/Sources/MicroInput/MicroInputApp.swift` 必须在 `setupStatusItem()` 前调用 `mi_core_acquire_instance_lock` 获取 `<documentDir>/instance.lock` 排他锁、`scripts/run` 必须使用严格 `argparse`（支持 `--list`/`--kill` 并剥离继承的 `__CFBundleIdentifier`），杜绝与 `/Applications/Micro Input.app` 双开产生双菜单栏图标 | `scripts/build` + `scripts/check-boundaries.py`（`R-C05`）+ `scripts/tests/test_run_verify_launcher.py` + `crates/mi-macos/tests/macos_scenarios.rs`（`document_instance_lock_rejects_duplicate_owner_until_released`） |
| `FourAppVoiceManager::start_dictation` 必须先检查 `dispatch_open`，并等待 `PushToTalkController::wait_for_start_outcome()` 确认启动成功后才可进入 `FourAppDictationState::Recording`，严禁未确认即报告成功 | `scripts/check-boundaries.py`（`R-C04`）+ `crates/mi-bridge/src/voice.rs`（`test_start_dictation_fails_closed_when_ownership_blocked_or_driver_rejects`） |
| `CoreStore` 必须采用 Swift `@Observable` + `Equatable` 域切片门禁（`header`、`sidebar`、`canvas`、`sessionStrip`、`inspector`、`statusMenu`、`overlay`、`footer`、`activeModal`），严禁使用全树广播的 `ObservableObject` / `@Published var model` 或向子视图传递巨石 `ProjectionModel`；Rust `mi_core_dispatch_json` 与 `poll_hardware_events` 必须以 `projection_generation` 门禁无变化短路 | `scripts/check-boundaries.py`（`R-C06`）+ `app/MicroInput/Tests/MicroInputTests/MicroInputTests.swift`（`testRenderInvalidationAndLatencyProfile`）+ `crates/mi-ffi/src/lib.rs`（`ffi_roundtrip_projects_and_dispatches_commands`） |
| `app/MicroInput/Sources/MicroInputUI/*.swift` 中所有 `.buttonStyle(.plain)` 与 `.onTapGesture` 必须声明 `.contentShape(Rectangle())`；`InlineInspectorView` 必须绑定 `.id(...)`、`ScrollViewReader` 滚动复位及搜索框 `.onSubmit`，且 `consume_physical_input_event` 在 `inspector_search_active` 时禁止抢占 `selected_surface`；`MicroInputApp.swift` 必须实现 `applicationDockMenu`、`win.deminiaturize`、`applicationShouldHandleReopen` 与 `AppIcon.icns` 挂载 | `scripts/check-boundaries.py`（`R-C07`）+ `app/MicroInput/Tests/MicroInputTests/MicroInputTests.swift`（`testSidebarProfileAndSurfaceSwitchDistinctnessAndSearchFocusIsolation`） |
