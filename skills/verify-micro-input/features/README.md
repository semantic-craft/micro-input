# Micro Input 验收功能地图

## 1. 基线前置条件

- **主机与工具链**：macOS 14+，Rust stable，Swift 工具链，Python 3。
- **隔离运行目录**：由 `scripts/run --verify` 在 `<repo>/.scratch/qa-runs/<run-id>/` 创建（权限 `0700`，非 `/tmp`、非 `/private/tmp`）。
- **隔离数据与审计路径**：`MI_DOCUMENT_DIR=<run-dir>/documents`，`MI_HOST_AUDIT_LOG=<run-dir>/evidence/audit.jsonl`。
- **Doctor 检查**：`<run-dir>/evidence/launcher.json` 存在，`bundle_id == "com.semanticcraft.microinput.qa"`，只驱动本次运行启动的实例。

## 2. 驱动约定

- 每个功能配方均从干净的隔离 `MI_DOCUMENT_DIR` 开始。
- 优先使用合成场景驱动（`MI_SYNTHETIC_SCENARIO=host-execution`）与真实 UniFFI 桥接测试验证确定性行为，避免向宿主机当前前台编辑器乱发真实按键。
- 每次运行结束后核对 `launcher.json` 确认 `.app` 副本已清理且 `evidence/` 保留。

## 3. 证据与跳过报告

- **证据产物**：
  - 宿主执行与预设切换：`<run-dir>/evidence/audit.jsonl`（JSONL 格式的 `HostActionReceipt`）与 `<run-dir>/evidence/launcher.json`。
  - 汇总判定：`<run-dir>/evidence/result.json`（状态词仅限 `PASS` / `FAIL` / `BLOCKED` / `NOT_RUN` / `N/A`）。
- **证据层级声明**：
  - `synthetic-unit`：Rust `cargo test` / Swift `RecordingHostActionExecutor` 单元与桥接测试；
  - `isolated-app-synthetic-hid`：完整 `Micro Input QA.app` 经真实 UniFFI 接收合成 64B HID 帧并输出 `audit.jsonl`；
  - `live-hardware-readonly`：`scripts/observe-input --observe-once` 45 秒实机只读物理采样（需连接 USB `303A:8298` 设备并由操作者按压按键，缺设备时标 `BLOCKED` 并注明原因）。

## 4. 功能列表

| 功能 | 用户路径与驱动入口 | 可观察的通过证据 | 限制 |
| :--- | :--- | :--- | :--- |
| [T3 待处理会话 (`t3-attention`)](t3-attention.md) | 显式开启只读连接 → 下一个需要我；或绑定同名宿主动作 | 固定类别回执 `delivered`，精确目标另由原生 UI 核对 | 不发送、审批或创建会话；灯光未接通 |
| [宿主动作路由与执行 (`host-execution`)](host-execution.md) | 菜单栏开启「启用宿主动作执行」（或点「一键就绪」）→ 按键触发 `Shortcut` / `PasteText` / `OpenApp` / `OpenUrl` / `AgentCommand` / `FocusSession` | `audit.jsonl` 产生 `host_ready` 或 16 条合成动作回执，`no_newline=true`，工作台浏览位置不变 | 合成模式下默认记录审计回执，不抢占用户桌面焦点 |
| [三大 Coding Agent 预设与切换 (`agent-presets`)](agent-presets.md) | 安装 Coding Agent 预设 → 菜单栏切换 `Antigravity` / `Herdr` / `DeepSeek Coder (DSH)` → 触发按键、旋钮与摇杆 | 3 套 Profile 覆盖 20 槽位中的 19 个有效动作槽（含 `logical-15` `ENC_CLK` 按压与 `logical-16..19` 摇杆扇区），切换后同一按键路由至对应 Agent | `Herdr` / `DSH` 真实 CLI 调用视本机是否安装对应二进制而定 |
| [2U 宽键合并防抖与透传层健康检查 (`wide-key-and-passthrough`)](wide-key-and-passthrough.md) | 底部宽键双开关 (`AG10`+`AG11` / `ACT10`+`ACT11`) 先后按下/释放；检查 `KV_OAI_AG*` / `KV_OAI_ACT*` 透传层状态 | 宽键重叠按压仅触发 1 次动作且 `logical-10`↔`logical-11` 对称回退；非透传层返回 `LayerDriftWarning` | 电容触摸切层的实机通知形态需连接实机核验 |
