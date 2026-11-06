---
name: verify-micro-input
description: End-to-end acceptance and self-verification skill for Micro Input (macOS SwiftUI + Rust/UniFFI). Use before delivering changes to host action routing, menu-bar profile switching, input decoding/WideKeyMerger, Coding Agent presets/adapters, or workbench UI.
disable-model-invocation: true
---

# Verify Micro Input (`verify-micro-input`)

本技能为 Micro Input 提供冷启动即可执行的端到端自验证闭环。任何修改了宿主路由（`mi-model::host_router`）、输入解码与宽键防抖（`mi-session::input`）、UniFFI 桥接（`mi-ffi`）、菜单栏生效 Profile 切换、macOS 动作执行器（`HostActionExecutor.swift`）或 Coding Agent 预设/适配器（`preset.rs` / `AgentAdapters.swift`）的改动，交付前均按以下六节执行验收。

项目统一静态与单元门禁为 `scripts/check`，本技能在其基础上驱动隔离运行的完整应用（`scripts/run --verify`）并回读独立证据。

## 1. Launch

1. **构建合成后端或生产后端应用包**：
   - 驱动合成 HID 输入闭环（无需人工按压实体键盘）：
     ```sh
     scripts/build --synthetic
     ```
   - 验证生产二进制启动与离线工作台：
     ```sh
     scripts/build
     ```
2. **启动受控隔离 QA 实例**：
   - 使用 `scripts/run --verify` 启动。脚本默认在 `<repo>/.scratch/qa-runs/<timestamp>-<pid>/`（严禁使用 `/tmp` 或 `/private/tmp`）下创建隔离的 `Micro Input QA.app`（固定 `CFBundleIdentifier` 为 `com.semanticcraft.microinput.qa` 并持有 `.scratch/qa-runs/qa.lock` 排他锁）。
   - 启动合成宿主执行自检场景：
     ```sh
     MI_SYNTHETIC_SCENARIO=host-execution scripts/run --verify --timeout=25
     ```
   - 记录标准输出打印的三行关键定位信息：
     - `QA_APP_PATH=<run_dir>/Micro Input QA.app`
     - `QA_RUN_DIR=<run_dir>`
     - `QA_AUDIT_LOG=<run_dir>/evidence/audit.jsonl`

## 2. Doctor

驱动或断言任何行为前，核对以下健康指标：

1. **启动器回执检查**：读取 `$QA_RUN_DIR/evidence/launcher.json`，确认：
   - `bundle_id` 为 `"com.semanticcraft.microinput.qa"`（或显式传入 `--unique-bundle-id` 时的前缀）；
   - `run_dir` 的真实路径（`pwd -P`）不以 `/tmp` 或 `/private/tmp` 开头；
   - `pid` 为正整数。
2. **隔离环境检查**：确认 `$QA_RUN_DIR/documents` 与日常 `~/Library/Application Support/MicroInput` 完全分离。

## 3. Drive

支持两种驱动方式（详细配方见 [`features/README.md`](features/README.md)）：

1. **合成 HID 全链路自动驱动（`MI_SYNTHETIC_SCENARIO=host-execution`）**：
   - 应用启动后自动连接合成 `0.6.3` 设备、安装七套桌面 Agent 预设、开启 `hostExecutionEnabled`；
   - 依次切换非受保护的五个桌面 Profile（千问办公、ZCode、Antigravity、DSH、Herdr）；每个注入 AG00，Antigravity 和 DSH 另注入 AG01/AG02/AG03，Antigravity 另注入旋钮 AG13/AG14/ENC_CLK 与摇杆/提交/宏，Herdr 另注入 AG10+AG11 宽键组合与 AG15，并验证跨目标导航后退/前进。共 21 条 simulated 回执。该场景不验证真实应用 UI，不发送文本。
   - 生产模式下（无 `MI_SYNTHETIC_SCENARIO` 且 `MI_AUTO_HOST_READY != "0"`），若检测到唯一已验证 USB `303A:8298` 设备，自动完成连接、身份与配置读取、预设安装并向 `$QA_AUDIT_LOG` 写入 `action_type: "host_ready"` 就绪回执；
   - 宿主执行器将每次派发的脱敏回执实时追加写入 `$QA_AUDIT_LOG`。
2. **UniFFI + Swift 注入式驱动**：
   - 运行 `swift test --package-path app/MicroInput -Xswiftc -DMI_SYNTHETIC_BACKEND --filter HostExecutionTests`，通过 `RecordingHostActionExecutor` 与真实 UniFFI 会话验证全套动作派发与草稿隔离。

## 4. Evidence

每次运行必须在 `$QA_RUN_DIR/evidence/` 下留存并回读以下产物：

1. **`launcher.json`**：记录 `app_path`、`bundle_id`、`pid`、`started_ms`、`finished_ms`、`returncode`、`app_bundle_removed`、`documents_removed`、`preferences_removed`。
2. **`audit.jsonl`（宿主动作执行审计回执）**：每行一个 `HostActionReceipt` JSON 对象：
   ```json
   {
     "action_type": "host_ready | open_app | shortcut | paste_text | open_url | agent_command | focus_session",
     "binding_id": "AG01",
     "detail": "...",
     "outcome": "executed | simulated | blocked",
     "profile_id": "<uuid>",
     "target": "com.google.antigravity",
     "text_byte_len": null,
     "timestamp_ms": 1730600000000
   }
   ```
   核对点：
   - `AG10` 与 `AG11`（或出厂 Codex 层 `ACT10` 与 `ACT11`）同时按下时，`audit.jsonl` 中只产生 **1 条** 宽键动作记录（防抖生效）；
   - `paste_text` 动作记录的 `detail` 明确包含 `no_newline=true`，且不落盘剪贴板明文；
   - 切换 `hostActiveProfileId` 后，相同 `binding_id` 派发到新 Profile 对应的目标（`Antigravity` $\rightarrow$ `Herdr` $\rightarrow$ `DSH`）。
3. **`result.json`**：核验通过后在 `$QA_RUN_DIR/evidence/result.json` 写入总结：
   ```json
   {
     "cleanup_confirmed": true,
     "git_sha": "<head-sha>",
     "receipt_count": 16,
     "run_id": "<run-id>",
     "scenario": "host-execution",
     "status": "PASS",
     "timestamp_ms": 1730600000000,
     "verified_features": [
       "host-execution",
       "agent-presets",
       "wide-key-and-passthrough"
     ]
   }
   ```

## 5. Cleanup

1. `scripts/run --verify` 在退出或收到 `SIGTERM` / `SIGINT` 时自动执行清理：
   - 终止并等待自己启动的 QA 进程 `pid`；
   - 删除 `$QA_RUN_DIR/Micro Input QA.app` 与 `$QA_RUN_DIR/documents`；
   - 删除 `~/Library/Preferences/com.semanticcraft.microinput.qa*.plist`；
   - 释放 `.scratch/qa-runs/qa.lock`。
2. **清理确认（必做）**：
   - 用 `kill -0 <pid>` 确认进程已不存在（严禁在进程退出后再次用 AX / UI 查询该应用，以免重新拉起实例）；
   - 确认 `$QA_RUN_DIR/Micro Input QA.app` 已删除，且 `$QA_RUN_DIR/evidence/launcher.json` 中 `app_bundle_removed == true`；
   - 确认 `$QA_RUN_DIR/evidence/audit.jsonl` 与 `result.json` 完整保留。

## 6. Helpers

- **一键全量门禁**：`scripts/check`（依次运行 Python 启动器测试、`cargo fmt`、Rust 全工作区测试、合成 UniFFI 构建与 Swift 测试、生产 UniFFI 构建与 Swift 测试、协议只读与 `UNKNOWN` 边界检查）。
- **启动器回归测试**：`python3 -B -m unittest discover -s scripts/tests -p 'test_*launcher.py'`。
- **功能地图索引**：见 [`features/README.md`](features/README.md)。
