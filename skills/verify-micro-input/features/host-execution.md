# 宿主动作路由与执行 (`host-execution`)

用户在菜单栏开启「启用宿主动作执行」（或点击「一键就绪（连接 USB + 预设 + 开启接管）」）并选定宿主激活 Profile 后，键盘发出的 `AG00`–`AG18` 及出厂 0.6.3 Codex 层事件（`ACT06`–`ACT12`、`ENC_CC`/`ENC_CW`/`ENC_CLK`、`v.oai.rad`）经 Rust 纯函数路由解析为 `Shortcut`、`PasteText`、`OpenApp`、`OpenUrl`、`AgentCommand` 或 `FocusSession`，由 Swift `HostActionExecuting` 执行并记录脱敏回执。

## Sub-features

- `HE-01`：菜单栏 `hostActiveProfileId` 与工作台草稿浏览 `profileIndex` 解耦（切换宿主激活 Profile 不改变工作台当前编辑的 Profile，也不写硬件 Flash；未显式设置时 Rust 与 Swift 一致回退到 `Antigravity`）。
- `HE-02`：`PasteText` 暂存并恢复原剪贴板，且绝不自动附加换行符或回车键（`no_newline=true`）。
- `HE-03`：关闭工作台主窗口后，会话级 `Store.hostInputTask` 继续监听输入并在 `InputStreamGap` 后自动重建订阅。

## How to get to it (user POV)

- 点击 macOS 菜单栏 Micro Input 图标（或按 `⇧⌘M`）打开菜单栏面板，打开「启用宿主动作执行」开关（或点击「一键就绪（连接 USB + 预设 + 开启接管）」 / 工作台顶栏「一键就绪（USB + 预设 + 开启接管）」），点击任意 Profile 名称将其设为宿主激活配置。

## Driving it with `verify-micro-input`

1. 构建合成后端应用包：`scripts/build --synthetic`。
2. 启动合成自检场景：`MI_SYNTHETIC_SCENARIO=host-execution scripts/run --verify --timeout=15`。
3. 待进程退出后，读取 `$QA_RUN_DIR/evidence/audit.jsonl` 与 `$QA_RUN_DIR/evidence/launcher.json`。
4. 确认 `audit.jsonl` 中包含 **9 条** 回执（涵盖 `open_app`、`paste_text`、`shortcut`、`focus_session`、`agent_command`），且 `launcher.json` 中 `app_bundle_removed == true`。
5. 若连接了唯一实机 USB `303A:8298` 设备，运行 `scripts/build && scripts/run --verify --timeout=10`，确认 `audit.jsonl` 自动记录 `action_type: "host_ready"`（`passthrough=ready presets=5`）。

## Gotchas

- 若未构建 `--synthetic` 即运行 `MI_SYNTHETIC_SCENARIO=host-execution`，应用不会注入合成 HID 帧，`audit.jsonl` 将不存在或为空，此时应判为 `FAIL` 并重新运行 `scripts/build --synthetic`。
