# Micro Input 宿主实时接管、多 Coding Agent 切换与自验证闭环推进计划

*   **Author:** semantic-craft
*   **Status:** Approved for Implementation
*   **Date:** 2026-11-03

--------------------------------------------------------------------------------

## 当前实现校正（2026-11-10）

下文 Phase 3 的三套 CLI/通用快捷键方案保留为历史设计。当前实现以[四套内置 Agent Profile 和一套通用预设](four-app-control.md)为准，DSH CLI 占位动作已阻止执行；Herdr Profile 指向独立的 Herdr App。默认动作、迁移规则、真实 UI 验收与未支持项见 [桌面控制验收](../verification/desktop-agent-controls.md)。合成场景为 20 条 simulated 回执，不等于真实应用接受操作。

## Overview

本设计定义 Micro Input 从「只读离线配置工作台（`S1` + `S2a`，见 [`../spec/BUILD_PROMPT.md`](../spec/BUILD_PROMPT.md)）」演进为「支持宿主实时动作接管、多 Coding Agent 配置切换与端到端自验证闭环（`T04` + `S4: T17–T23`，见 [`../implementation-plan.md`](../implementation-plan.md) 及 [`../spec/SPEC.md`](../spec/SPEC.md)）」的技术方案。

结合 `cursor/plugins` 的 `pstack`（`create-verification-skill` / `maintain-verification-skill`）以及本机 `project-verification` 规范，Agent 独立完成「修改代码 → 启动隔离真实应用 → 注入按键事件 → 回读目标副作用 → 清理环境」闭环采用以下架构：
- **方案 A（Rust 纯确定性路由 + Swift 协议化原生执行层）+ 合成 HID 注入回路 + `skills/verify-micro-input` 验收技能**。

整个推进计划分为四个递进阶段（Phases 0–3）：
1. **Phase 0（建立 `skills/verify-micro-input` 自验证闭环）**：基于现有 `synthetic-backend`（合成 64B HID 帧注入）与 Swift 可注入执行器协议，建立符合 `project-verification` 六节规范（Launch、Doctor、Drive、Evidence、Cleanup、Helpers）的 `skills/verify-micro-input/` 技能，使 Agent 无需人工按键即可驱动完整应用并回读执行结果。
2. **Phase 1（物理输入映射、宽键防抖与透传健康检查，`T04 / E-094`，见 [`../spec/EVIDENCE.md`](../spec/EVIDENCE.md)）**：基于 `0.6.3` 固件下的 `v.oai.hid`（`AG00`–`AG18`）与 `v.oai.rad`（参见 [`../protocol-candidates.md`](../protocol-candidates.md)），建立 `AG00`–`AG18` 到 `LocalDocumentV2` 逻辑槽位 `logical-0`–`logical-19` 的确定性映射、2U 宽键（`AG10` + `AG11`）合并防抖器 `WideKeyMerger` 以及硬件 `KV_OAI_AG*` 透传层健康检查 `PassThroughHealth`。
3. **Phase 2（菜单栏宿主 Profile 纯内存切换与 macOS 动作执行层，`T17 / T18 / T20`）**：在菜单栏实现纯内存切换（`< 1ms`，不写 Flash）的「当前宿主生效 Profile（`hostActiveProfileId`）」，接通快捷键合成（`CGEvent`）、暂存并恢复原剪贴板的文本粘贴（`NSPasteboard`）与应用聚焦（`NSWorkspace`）。
4. **Phase 3（Coding Agent 预设与适配器，`T21–T23`）**：交付 **Antigravity（通用预设 `#30`）**、**Herdr（`#31`，多路终端复用器）**、**DeepSeek Coder / DSH（`#32`，DeepSeek 终端交互会话工具）** 三套预设与本地适配器，并通过 `verify-micro-input` 对隔离目标窗格/应用进行端到端实测验收。

## The Problem

目前项目已完成第 1～3 层后端（`mi-transport`、`mi-proto`、`mi-session`、`mi-model`、`mi-ffi`）与 SwiftUI 离线工作台，但距离支持宿主实时接管与 Agent 自主迭代验证存在四个核心问题：

1. **缺乏 `verify-micro-input` 自验证技能与端到端回读锚点**：
   - 仓库目前尚无 `skills/verify-micro-input/SKILL.md`。
   - 现有 [`../../scripts/run`](../../scripts/run) `--verify` 使用 `tempfile.TemporaryDirectory` 在 `/tmp`（解析后为 `/private/tmp`）下创建 QA 应用包，且每次生成随机 UUID 的 `CFBundleIdentifier`。按照 `project-verification` 的 macOS 实测经验，`/tmp` 下的应用会被 LaunchServices 登记为 `Other (255)`，随机 Bundle ID 会导致每次重跑都要重新授予辅助功能（Accessibility）权限，且缺乏 `MI_HOST_AUDIT_LOG` 审计回执供自动化回读验证。
2. **原厂 `Input.app` 切换 Profile 强制刷写硬件 Flash**：官方 Electron 应用每次点选 Profile 都会触发倒计时覆写硬件 LittleFS（参见 [`../input-architecture.md` IA-13/14](../input-architecture.md)）。
3. **宿主执行层与菜单栏生效态尚未接通**：[`../../crates/mi-model/src/action_library.rs`](../../crates/mi-model/src/action_library.rs) 中 `execution_available` 全为 `false`；菜单栏点选 Profile 仅切换工作台草稿浏览视图，且输入监听循环绑定在工作台诊断子视图上，关闭工作台主窗口即停止监听。
4. **物理键位反序、2U 宽键双微动与硬件层漂移缺乏保护**：第 0 行绑定曾被误判为反序（2026-11-03 实机逐键核验：`AG00` 在左、`AG01` 在右）、底部宽键双开关（`AG10` + `AG11`）以及摇杆四扇区（`AG15`–`AG18` 对应 `logical-16`–`logical-19`）尚未接入可执行路由；若误触左下角电容触摸区切走硬件 Layer，宿主将停止收到 `v.oai.hid` 而缺乏状态提示（关联 [`../spec/UNKNOWNS.md` `UNKNOWN-07` / `UNKNOWN-10`](../spec/UNKNOWNS.md)）。

## Goals

*   **G1（`project-verification` 自验证闭环）**：在仓库内建立 `skills/verify-micro-input/`（含 Launch、Doctor、Drive、Evidence、Cleanup、Helpers 与 `features/` 地图），提供「合成 HID 报文注入 → 真实 UniFFI 会话 → 菜单栏宿主路由 → 动作回读验证」自检链路，产出结构化 `result.json` 证据。
*   **G2（物理输入映射与宽键/透传保护，`T04 / E-094`）**：建立 `AG00`–`AG18` 到 `logical-0`–`logical-19` 的标准映射表，实现 2U 宽键（`AG10` + `AG11`）合并防抖与硬件 `KV_OAI_AG*` 透传层健康度检测。
*   **G3（菜单栏纯内存宿主 Profile 切换，`T20`）**：分离「工作台编辑 Profile（`profileIndex`）」与「菜单栏宿主生效 Profile（`hostActiveProfileId`）」，纯内存切换（`< 1ms`，不写 Flash），持久化于 `HostPreferences`。
*   **G4（可自验证的 macOS 动作执行层，`T17 / T18`）**：在 Swift 层通过 `HostActionExecuting` 协议实现快捷键合成（`CGEvent`）、暂存并恢复原剪贴板的文本粘贴（`NSPasteboard`）及应用聚焦（`NSWorkspace`），内置前台焦点轮询确认与重入防覆盖剪贴板恢复保护。
*   **G5（三大 Coding Agent 预设与适配，`T21–T23`）**：复用现有 `ActionKind` 交付 **Antigravity**、**Herdr**、**DeepSeek Coder / DSH** 三套预设，实现 `HerdrAdapter` 与 `DshAdapter`，支持聚焦指定会话/窗格与注入文本不自动回车。

## Non-Goals

*   **NG1（不开放硬件 Flash 生产写入）**：本阶段不启用 `fs.writebin` 刷写设备 `keymap.json` / `smart_actions.json`（`S3: T13–T15` 及 `UNKNOWN-08` 保持只读门禁）。
*   **NG2（测试不劫持用户真实桌面焦点或覆盖用户剪贴板）**：自动化门禁（`scripts/check`）中的动作测试一律走 `RecordingHostActionExecutor` 与注入回调，不向用户桌面乱发按键或污染日常剪贴板。
*   **NG3（不自动提交未确认的 Agent Prompt）**：注入 Prompt 文本后严禁追加换行符 `\n` 或合成回车键。
*   **NG4（不引入后台常驻守护进程或网络端口）**：全部功能收敛在菜单栏应用进程与一次性 CLI 内。

## Open Questions

*   **Q1（旋钮按压与摇杆实机报文形态核验）**：在当前设备 `profile_index: 0`（固件 `0.6.3`）下，按下左上旋钮及拨动右上摇杆实际发出的报文格式将在 45 秒 `scripts/observe-input --observe-once` 只读采样（`E-094`）中直接核实。若摇杆发出连续 `v.oai.rad` 或旋钮按压无 `AG` 绑定，应对回退策略见 `## Risks & Mitigations`。

## Critical User Journeys (CUJs)

### CUJ 1: Agent 自验证与自我迭代闭环（`verify-micro-input`）

*   **Given**: Agent 修改了宿主路由或 Agent 适配器代码，准备在无需人类按压物理键盘的情况下验证端到端行为。
*   **When**: Agent 按 `skills/verify-micro-input/SKILL.md` 执行 `launch` → `doctor` → `drive`（通过合成 HID 通道注入 `AG00`–`AG18` 帧，并在菜单栏切换宿主生效 Profile 到 `Herdr` / `Antigravity` / `DSH`）→ `evidence` → `cleanup`。
*   **Then**:
    1. 真实应用通过真实 UniFFI 接收到合成 `v.oai.hid` 帧并完成宽键去重与宿主 Profile 查表；
    2. 宿主执行器完成动作派发并将脱敏回执写入 `MI_HOST_AUDIT_LOG`；
    3. Agent 校验目标沙箱状态与 `MI_HOST_AUDIT_LOG` 一致，生成 `result.json`（`PASS`），并回收 QA 进程与临时偏好。

### CUJ 2: 45 秒实机物理输入核验（Phase 1）

*   **Given**: Creator Micro 2 通过 USB 连接到 Mac，处于当前 `profile_index: 0`（固件 `0.6.3`），运行 `scripts/observe-input --observe-once`。
*   **When**: 操作者按顺序触发 12 个键帽（含宽键左、右、中）、左上旋钮（顺/逆时针与按压）和右上摇杆四向，并测试轻触左下角电容区切层再切回。
*   **Then**: 脚本记录完整脱敏映射证据（`E-094`），核对 `AG00`–`AG18` 与物理控件的对应关系。

### CUJ 3: 菜单栏纯内存切换 Coding Agent 并操控窗口/窗格（Phases 2 & 3）

*   **Given**: 菜单栏宿主生效 Profile 设为 `Herdr`、`DeepSeek (DSH)` 或 `Antigravity`，且工作台主窗口已关闭或正在浏览另一个草稿 Profile。
*   **When**: 用户在菜单栏切换宿主生效 Profile，随后按压按键、旋转旋钮、拨动摇杆或按下底部宽键。
*   **Then**:
    1. 菜单栏切换纯内存生效（`< 1ms`，不写 Flash），且不受工作台当前浏览草稿或主窗口开关状态影响；
    2. 对应 Agent 的目标窗口/窗格被准确聚焦，常用 Prompt 经暂存并恢复原剪贴板的方式粘贴（不自动回车），旋钮与摇杆完成窗格/Tab 切换。

--------------------------------------------------------------------------------

## Technical Plan (Architecture)

### 1. 端到端数据流与分层边界（方案 A + 自验证回路）

```text
+----------------------------------------------------------------------------------------+
| 输入源：真实 USB HID (303A:8298, 0.6.3)  或  Synthetic Backend (64B v.oai.hid 帧注入)   |
+----------------------------------------------------------------------------------------+
                                            │
                                            ▼
+----------------------------------------------------------------------------------------+
| 第 1~2 层：mi-transport + mi-proto + mi-session::input                                 |
| - 单 reader 接收 0x06/0x02 报文，重组 JSON 通知                                        |
| - VerifiedInputDecoder / CandidateInputDecoder：解析 v.oai.hid (AG00..AG18)            |
| - WideKeyMerger：合并底部 2U 宽键双微动 (AG10 + AG11)，输出消抖后的逻辑控件事件        |
+----------------------------------------------------------------------------------------+
                                            │
                                            ▼
+----------------------------------------------------------------------------------------+
| 第 3 层：mi-model::host_router + mi-ffi (纯内存确定性路由，cargo test 直接验证)         |
| - 状态解耦：profileIndex (工作台草稿浏览) != host_active_profile_id (宿主生效)         |
| - PassThroughHealth：校验设备当前激活层是否包含 KV_OAI_AG* 原始字符串绑定              |
| - resolve_host_action()：按 (host_active_profile_id, binding_id) 映射至 logical-{slot} |
|   并直接从 ActionKind 解析出 ResolvedHostAction（Shortcut / PasteText / OpenApp /       |
|   OpenUrl / AgentCommand / FocusSession）                                              |
+----------------------------------------------------------------------------------------+
                                            │ UniFFI
                                            ▼
+----------------------------------------------------------------------------------------+
| 第 4~5 层：Swift 宿主执行与 Agent 适配层 (app/MicroInput/Sources/MicroInput/)          |
| - Store.swift：持有会话级 hostInputTask（在 InputStreamGap 后自动重订阅）、            |
|   hostActiveProfileId、hostExecutionEnabled 与 passThroughHealth                       |
| - StatusPanel (MicroInputApp.swift)：菜单栏切换宿主生效 Profile、显示透传健康状态      |
| - HostActionExecuting 协议：                                                           |
|   ├─ LiveHostActionExecutor (生产/QA 实测)：                                           |
|   │   * Shortcut -> 前置聚焦轮询确认 + CGEvent 键盘事件合成（前检 AXIsProcessTrusted） |
|   │   * PasteText -> 深拷贝暂存剪贴板 (<=4 MiB) + 写入 + ⌘V + 防重入合并 150ms 恢复    |
|   │   * OpenApp / OpenUrl -> NSWorkspace 前置激活指定 Bundle ID / 打开 URL             |
|   │   * AgentCommand / FocusSession -> 调用 HerdrAdapter / DshAdapter                  |
|   │   * 若设置 MI_HOST_AUDIT_LOG，每次执行追加写入结构化 JSONL 审计回执供自验证回读    |
|   └─ RecordingHostActionExecutor (单元与桥接测试)：纯内存记录动作序列，零桌面副作用    |
+----------------------------------------------------------------------------------------+
```

### 2. Phase 0：自验证脚手架与 `skills/verify-micro-input` 设计

遵循 `project-verification` 规范，消除 Agent 自迭代中的路径陷阱与并发冲突：

1. **升级 [`../../scripts/run`](../../scripts/run) `--verify` 启动器**：
   - **固定 QA Bundle ID 与单实例文件锁**：将 `--verify` 模式下的 `CFBundleIdentifier` 设为固定的 `com.semanticcraft.microinput.qa`（同时支持 `--unique-bundle-id` 供纯并行无 TCC 场景使用）。在 `<repo>/.scratch/qa-runs/qa.lock` 获取非阻塞排他文件锁（`fcntl.flock(LOCK_EX | LOCK_NB)`），防止多 worktree 同时启动相同 Bundle ID 的 QA 实例互相抢占；若锁被占用且持有进程存活，立即返回明确错误退出码。
   - **规避 `/tmp` 与 `/private/tmp` 陷阱**：QA 应用包、隔离数据目录（`MI_DOCUMENT_DIR`）与宿主执行审计日志（`MI_HOST_AUDIT_LOG`）统一创建在 `<repo>/.scratch/qa-runs/<run-id>/`（若传入自定义目录，用 `Path.resolve()` 校验绝对路径不得以 `/tmp` 或 `/private/tmp` 开头）。
   - **进程生命周期与清理保证**：保持 `subprocess.Popen` 启动并记录子进程 PID，在 `try / finally` 与 `SIGTERM` / `SIGINT` 处理器中确保 QA 应用进程退出、删除 `.app` 副本与临时偏好域（`~/Library/Preferences/com.semanticcraft.microinput.qa.plist`），同时保留证据目录 `<repo>/.scratch/qa-runs/<run-id>/evidence/`（含 `audit.jsonl` 与 `launcher.json`）。
   - **支持合成输入驱动**：在 `--verify` 模式下支持 `MI_SYNTHETIC_SCENARIO=host-execution`，自动连接合成 `0.6.3` 设备、安装三套 Agent 预设、开启宿主动作接管，并按序注入 `AG00`–`AG18` 合成帧，走完「HID 报文 → Rust 解码与路由 → UniFFI → Swift 执行器 → `MI_HOST_AUDIT_LOG` 回执」全链路。
2. **新增 `skills/verify-micro-input/SKILL.md` 及功能地图**：
   - 包含 `Launch`、`Doctor`、`Drive`、`Evidence`、`Cleanup`、`Helpers` 六节与 `features/` 功能地图，通过 `.agents/skills/verify-micro-input` 软链接线（记入 `.git/info/exclude`）。

### 3. Phase 1：物理输入映射、2U 宽键防抖与透传健康度（`T04 / E-094`）

1. **标准 `AG00`–`AG18` $\leftrightarrow$ `logical-{slot}` 映射表**：
   [`../../crates/mi-model/src/document.rs`](../../crates/mi-model/src/document.rs) 中的 `LayerNode` 以 `logical_control: "logical-{slot}"` 索引控件（`0..12` 为 13 个按键槽位，`13..15` 为 3 个旋钮槽位，`16..19` 为 4 个摇杆逻辑扇区槽位）。[`../../crates/mi-session/src/input.rs`](../../crates/mi-session/src/input.rs) 与 [`../../crates/mi-model/src/host_router.rs`](../../crates/mi-model/src/host_router.rs) 使用以下唯一双射表：

   | `v.oai.hid` 绑定 (`binding_id`) | 逻辑槽位 (`logical_switch`) | 物理位置标识 (`physical_control`) | 键帽分组 (`keycap_group`) | 动作角色 (`action_role`) | 说明 |
   | :--- | :--- | :--- | :--- | :--- | :--- |
   | `AG00` | `logical-0` | `row0-col0-left` | `key-r0c0` | `momentary_key` | 第 0 行左键（0.6.3 实机逐键核验） |
   | `AG01` | `logical-1` | `row0-col1-right` | `key-r0c1` | `momentary_key` | 第 0 行右键（0.6.3 实机逐键核验） |
   | `AG02`–`AG05` | `logical-2`–`logical-5` | `row1-col0`..`row1-col3` | `key-r1c0`..`key-r1c3` | `momentary_key` | 第 1 行左起 4 键 |
   | `AG06`–`AG09` | `logical-6`–`logical-9` | `row2-col0`..`row2-col3` | `key-r2c0`..`key-r2c3` | `momentary_key` | 第 2 行左起 4 键 |
   | `AG10` + `AG11` | `logical-10`（回退 `logical-11`） | `row3-wide-2u` | `wide-bottom-2u` | `wide_momentary_key` | 第 3 行底部 2U 宽键双开关合并输出 `binding_id: "AG10"`, `logical_switch: "logical-10"` |
   | `AG12` | `logical-12` | `row3-col2-right` | `key-r3c2` | `momentary_key` | 第 3 行右下单键 |
   | `AG13` | `logical-13` | `encoder-top-left-ccw` | `encoder-top-left` | `encoder_step` | 左上旋钮逆时针步进 |
   | `AG14` | `logical-14` | `encoder-top-left-cw` | `encoder-top-left` | `encoder_step` | 左上旋钮顺时针步进（`logical-15` 留给旋钮第三槽） |
   | `AG15` | `logical-16` | `joystick-sector-north` | `joystick-top-right` | `joystick_sector` | 右上摇杆北扇区（跳过 `logical-15`，即 `ag_num + 1`） |
   | `AG16` | `logical-17` | `joystick-sector-west` | `joystick-top-right` | `joystick_sector` | 右上摇杆西扇区 |
   | `AG17` | `logical-18` | `joystick-sector-south` | `joystick-top-right` | `joystick_sector` | 右上摇杆南扇区 |
   | `AG18` | `logical-19` | `joystick-sector-east` | `joystick-top-right` | `joystick_sector` | 右上摇杆东扇区 |

2. **2U 宽键合并防抖状态机（`WideKeyMerger`）**：
   - 在 [`../../crates/mi-session/src/input.rs`](../../crates/mi-session/src/input.rs) 中新增 `VerifiedInputDecoder`（保持现有 `CandidateInputDecoder` 行为不变，供 `input_observation.rs` 候选诊断测试继续使用；生产会话可通过 `subscribe_verified_input` 或配置切换启用 `WideKeyMerger`）：
     - 维护 `active_wide_switches: BTreeSet<String>`；
     - 当 `AG10` 或 `AG11` 按下时：若集合为空，插入该开关并发出 1 次规范化宽键事件（`kind: InputEventKind::Pressed`, `binding_id: Some("AG10")`, `logical_switch: Some("logical-10")`, `keycap_group: Some("wide-bottom-2u")`, `action_eligible: true`）；若集合非空，仅记录开关，不重复触发；
     - 当 `AG10` 或 `AG11` 释放时：移除该开关；仅当集合变为空时，发出 1 次规范化宽键 `Released` 事件；
     - 收到 `InputStreamGap` 或 `close()` 时：立即清空集合，防止卡键。
3. **硬件透传层健康度检测（`PassThroughHealth`）**：
   - 在 `crates/mi-model/src/host_router.rs` 中定义统一签名：
     `pub fn check_passthrough_health(snapshot: Option<&DeviceConfig>, identity: Option<&HostIdentityStatus>) -> PassThroughHealth`
     - `Disconnected`：`identity` 为 `None`；
     - `SnapshotMissing`：`identity` 存在但 `snapshot` 为 `None`，或固件版本不是 `"0.6.3"` / `"synthetic-0.6.3"`；
     - `Ready`：`snapshot` 中对应 `identity.profile_index`（默认 `0`）与 `identity.layer_index`（默认 `0`）的活动 `Layer.keys` 中，包含全部 13 个 `"KV_OAI_AG00"`..`"KV_OAI_AG12"` 原始字符串绑定（在 [`../../crates/mi-model/src/device.rs`](../../crates/mi-model/src/device.rs) 中被解析为 `Binding::Unknown(Value::String(s))`）；
     - `LayerDriftWarning { active_profile: u32, active_layer: u32, bound_ag_count: u32 }`：设备已连接且已读快照，但当前活动层包含的 `"KV_OAI_AG*"` 键数不足 13 个（例如误触了左下角电容区切到了非透传层）。该状态在连接读取身份/快照及手动刷新身份时更新。

### 4. Phase 2：菜单栏宿主生效 Profile 切换与 macOS 原生动作执行（`T17 / T18 / T20`）

1. **分离「草稿浏览态」与「宿主生效态」并复用 `HostPreferences`**：
   - 扩展 [`../../crates/mi-model/src/storage.rs`](../../crates/mi-model/src/storage.rs) 中的 `HostPreferences`（保持 `#[serde(deny_unknown_fields)]`）：
     新增 `#[serde(default)] pub host_active_profile_id: Option<String>` 与 `#[serde(default)] pub host_execution_enabled: bool`，随 `<workbench>.preferences.json` 原子保存，并在 [`../../crates/mi-ffi/src/lib.rs`](../../crates/mi-ffi/src/lib.rs) 的 `document_response` JSON 中一并返回。
   - 在 [`../../app/MicroInput/Sources/MicroInput/Store.swift`](../../app/MicroInput/Sources/MicroInput/Store.swift) 中维护会话级后台任务 `hostInputTask`（在 `connect` 成功后自动启动、在 `retireConnection` 时取消，且在收到 `.inputStreamGap` 后自动重新调用 `subscribeInput` 恢复监听，不依赖工作台窗口是否打开）。
   - 升级 `StatusPanel`（[`../../app/MicroInput/Sources/MicroInput/MicroInputApp.swift`](../../app/MicroInput/Sources/MicroInput/MicroInputApp.swift)）：展示 `PassThroughHealth` 状态、辅助功能权限状态、「启用宿主动作接管」开关、Profile 切换列表（点击调用 `store.setHostActiveProfile(profile.id.text)` 纯内存切换，不强弹主工作台窗口）以及「安装 Coding Agent 预设」入口。
2. **直接对齐现有 `ActionKind` 的动作路由解析器（`crates/mi-model/src/host_router.rs`）**：
   - `resolve_host_action(doc: &LocalDocumentV2, host_active_profile_id: Option<&str>, binding_id: &str, is_pressed: bool) -> Option<ResolvedHostAction>`：
     - 仅在 `is_pressed == true` 时触发；
     - 定位 `host_active_profile_id` 对应的 `ProfileNode`（若为 `None` 则不触发），取其首选层 `profile.layer_order.first()`；
     - 将 `binding_id`（`"AG00"`..`"AG18"`）通过映射表转为 `logical-{slot}`（若 `"AG10"` 对应 `"logical-10"` 未绑定动作，则回退查找 `"logical-11"`）；
     - 直接从绑定的 [`ActionKind`](../../crates/mi-model/src/draft.rs)（若为 `LocalMulti` 则取其 `tap` 分支引用的 `ActionKind`）转换为 `ResolvedHostAction`，无需发明平行枚举：
       - `ActionKind::Shortcut { key, modifiers, target }` $\rightarrow$ `ResolvedHostAction::Shortcut { key: String, key_code: u16, modifiers: Vec<String>, target_bundle_id: Option<String> }`（当 `target == "frontmost-editor"` 时 `target_bundle_id` 为 `None`，否则若 `target` 含 `.` 则作为目标 Bundle ID）；
       - `ActionKind::Text { text }` $\rightarrow$ `ResolvedHostAction::PasteText { text: String }`；
       - `ActionKind::OpenApp { bundle_id }` $\rightarrow$ `ResolvedHostAction::OpenApp { bundle_id: String }`；
       - `ActionKind::OpenUrl { url }` $\rightarrow$ `ResolvedHostAction::OpenUrl { url: String }`；
       - `ActionKind::AgentCommand { adapter, command }` $\rightarrow$ `ResolvedHostAction::AgentCommand { adapter: String, command: String }`；
       - `ActionKind::FocusSession { adapter, session }` $\rightarrow$ `ResolvedHostAction::FocusSession { adapter: String, session: String }`。
3. **Swift 原生执行器与安全防护（`app/MicroInput/Sources/MicroInput/HostActionExecutor.swift`）**：
   - **前置焦点轮询确认**：当动作带有 `target_bundle_id` 时，先通过 `NSWorkspace.shared.urlForApplication(withBundleIdentifier:)` 激活目标应用，随后以 15ms 间隔轮询 `NSWorkspace.shared.frontmostApplication?.bundleIdentifier == targetBundleId`（上限 300ms）；超时未聚焦则中止按键合成并记录失败回执，防止把快捷键或文本误发给原前台窗口。
   - **重入安全的剪贴板暂存与恢复**：深拷贝当前 `NSPasteboard.general.pasteboardItems` 的所有 `(NSPasteboard.PasteboardType, Data)` 对（单次总大小上限 4 MiB）。若在 150ms 恢复倒计时结束前再次触发 `PasteText`，保留第一次捕获的原始用户剪贴板快照并重置 150ms 计时器，防止连续按键将前一条 Prompt 误存为用户原始剪贴板；倒计时结束时仅当 `changeCount` 仍等于本次写入后的计数时恢复原剪贴板。

### 5. Phase 3：三大 Coding Agent 预设与适配器（`T21–T23`）

在 [`../../crates/mi-model/src/preset.rs`](../../crates/mi-model/src/preset.rs) 中实现 `install_agent_starter_profiles(draft: &mut Draft) -> Result<Vec<String>, String>`。对每个新建 Profile 的首选 Layer，先调用 `draft.set_control_settings(&pid, &lid, ControlSettings { joystick_mode: "logical_sectors".into(), active_sectors: 4 })` 扩展出 `logical-16`..`logical-19` 四个摇杆扇区槽位，再写入以下 19 个槽位绑定：

| 控件位置 | `v.oai.hid` 绑定 | 逻辑槽位 | **Antigravity 预设 (`T21 / #30`)** | **Herdr 预设 (`T22 / #31`)** | **DeepSeek Coder / DSH 预设 (`T23 / #32`)** |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **第 0 行左** | `AG00` | `logical-0` | `OpenApp("com.google.antigravity")` | `FocusSession("herdr", "session-1")` | `FocusSession("dsh", "session-1")` |
| **第 0 行右** | `AG01` | `logical-1` | `Shortcut("l", ["command"], "frontmost-editor")` | `FocusSession("herdr", "session-2")` | `FocusSession("dsh", "session-2")` |
| **第 1 行 4 键** | `AG02`–`AG05` | `logical-2`–`5` | `Text`：注入审查、计划、测试、重构 Prompt（不回车） | `FocusSession("herdr", "session-3".."session-6")` | `Text`：注入 DSH 调试、重构、测试、解释 Prompt（不回车） |
| **第 2 行 4 键** | `AG06`–`AG09` | `logical-6`–`9` | `Shortcut`：切终端(`ctrl+grave`)、新对话(`cmd+n`)、搜索(`cmd+p`)、保存(`cmd+s`) | `AgentCommand("herdr", "new-workspace" / "split-right" / "toggle-stack" / "close-pane")` | `AgentCommand("dsh", "interrupt" / "clear" / "inject-context" / "history-search")` |
| **底部 2U 宽键** | `AG10` + `AG11` | `logical-10` | `Shortcut("return", ["command"], "frontmost-editor")`（同意继续） | `AgentCommand("herdr", "approve-current")` | `AgentCommand("dsh", "approve-current")` |
| **第 3 行右下键** | `AG12` | `logical-12` | `Shortcut("escape", [], "frontmost-editor")`（取消/拒绝） | `AgentCommand("herdr", "deny-current")` | `AgentCommand("dsh", "deny-current")` |
| **左上角旋钮** | `AG13` / `AG14` | `logical-13` / `14` | `Shortcut`：`[` / `]` + `["command", "shift"]`（切 Tab） | `AgentCommand("herdr", "prev-session" / "next-session")` | `AgentCommand("dsh", "prev-session" / "next-session")` |
| **右上角摇杆 4 向** | `AG15`–`AG18` | `logical-16`–`19` | `Shortcut`：`up`/`left`/`down`/`right` + `["command", "option"]` | `AgentCommand("herdr", "pane-north" / "pane-west" / "pane-south" / "pane-east")` | `AgentCommand("dsh", "pane-north" / "pane-west" / "pane-south" / "pane-east")` |

*   **Antigravity 通用预设 (`#30`)**：完全使用标准 `OpenApp`、`Shortcut`、`Text` 动作，由 `LiveHostActionExecutor` 直接执行，无需单独的 `AntigravityAdapter`。
*   **`HerdrAdapter` (`#31`)**：解析可执行文件路径（优先读取环境变量 `MI_HERDR_BIN`，否则按 `["/opt/homebrew/bin/herdr", "/usr/local/bin/herdr", "\(NSHomeDirectory())/.local/bin/herdr"]` 查找），使用串行执行队列（超时 2s）调用非 login 子进程（如 `herdr list --json`、`herdr focus <session>`、`herdr send-text --no-newline <session> <text>` 或 `herdr cmd <command>`），若未找到二进制或目标会话不存在则返回明确错误并写入 `MI_HOST_AUDIT_LOG`。
*   **`DshAdapter` (`#32`)**：解析 `MI_DSH_BIN` 或标准路径下的 `dsh` CLI（或定位目标终端窗口），执行会话枚举、指定会话聚焦与非提交式文本注入；当目标会话不存在或 CLI 未安装时返回结构化不可用回执（关联 `UNKNOWN-13`）。

## Alternatives Considered

### Alternative A: 先解禁硬件 Flash 写入（`S3: fs.writebin`），每次切换 Profile 重刷 `keymap.json`（`Input.app` 原厂模式）

*   **Pros**: 即使退出 Mac 菜单栏宿主程序，普通标准键盘快捷键（`KC_*`）仍驻留在键盘硬件 Flash 中。
*   **Cons**:
    1. 每次在菜单栏切换 Profile 都需要分块传输并擦写硬件 LittleFS，耗时约 2–3 秒，无法做到即时切换；
    2. 受限于当前探测 C 临时文件清理与大文件覆盖写门禁（`T13 / UNKNOWN-08`），会继续阻塞可用时间；
    3. 驻留固件的普通键码模式不支持 `v.oai.thstatus` 单键独立 RGB 状态灯，也无法直接调用 Herdr / DSH 的本地会话接口。
*   **Reason for Rejection**: 本机键盘 `profile_index: 0` 已经包含 `KV_OAI_AG00`–`AG18` 透传绑定。采用宿主内存态路由可实现 `< 1ms` Profile 切换、零 Flash 擦写风险，并直接支持 Agent 本地适配。

### Alternative B: Rust 包揽事件路由与全部 macOS 系统调用（`core-graphics` + `osascript`）

*   **Pros**: 代码改动集中在 Rust 单一语言内，无需扩展 UniFFI 动作执行接口。
*   **Cons**:
    1. macOS 的 `NSWorkspace` 窗口聚焦、`NSPasteboard` 剪贴板读写与 TCC 权限弹窗要求绑定主运行循环（Main RunLoop）和完整 App Bundle 上下文，从 Rust 异步后台线程直接跨 FFI 调用易触发线程安全异常或权限失败；
    2. 若通过派生 `osascript` 子进程实现粘贴与按键，单次触发延迟高达 100–300ms，且字符串转义与剪贴板恢复容易出错；
    3. `cargo test` 会直接耦合 macOS 桌面会话，破坏无副作用自验证。
*   **Reason for Rejection**: 违反 AppKit 主线程约束，且不利于 Agent 在无 GUI 干扰下运行自动化测试。

### Alternative C: 混合分层执行（Swift 负责 macOS UI 动作，Rust 负责 Herdr / DSH 通信）

*   **Pros**: Herdr / DSH 的命令行解析逻辑可以在 Rust 内单独写集成测试。
*   **Cons**:
    1. 将「外部副作用执行」拆成 Rust 与 Swift 两套运行时，当一次按键组合了「前置聚焦终端窗口（Swift）+ 切换指定 Agent 窗格（Rust）」时，必须跨 UniFFI 往返编排时序；
    2. 自验证所需的执行审计日志（`MI_HOST_AUDIT_LOG`）与错误处理分散在两门语言中，维护成本增加。
*   **Reason for Rejection**: 保持「Rust 纯函数做状态与路由决策，Swift `HostActionExecuting` 统一承担全部系统与 Agent 外部副作用」边界清晰、可测性强。

--------------------------------------------------------------------------------

## Non-Functional Requirements

*   **Security & Privacy（安全与隐私）**：
    *   **不自动回车提交**：所有注入 Antigravity、DSH、Herdr 的文本动作严禁附加 `\n` 或合成 `Return` 键（`0x24`），防止误触按键直接执行未确认命令。
    *   **剪贴板隐私保护**：文本注入前后仅在进程内存中深拷贝暂存原 `NSPasteboardItem` 数据（上限 4 MiB）并在 150ms 后恢复；诊断日志与 `MI_HOST_AUDIT_LOG` 仅记录动作类型、目标标识与字符长度摘要（`text_byte_len`），不将剪贴板原文写入磁盘日志。
    *   **TCC 权限最小化与固定标识**：仅在用户触发需合成按键的动作时检查 `AXIsProcessTrusted()`；QA 验证包默认使用 `com.semanticcraft.microinput.qa` 并配排他锁，避免污染系统 TCC 授权列表。
*   **Performance & Latency（性能与延迟）**：
    *   **菜单栏切换延迟**：宿主生效 Profile 切换为纯内存更新加异步偏好落盘，内存态生效耗时 `< 1ms`。
    *   **按键到动作派发延迟**：从 `mi-transport` 收到 64B HID 帧到 Swift `HostActionExecutor` 派发动作（不含跨应用前置聚焦等待）端到端耗时控制在 `< 15ms`。
*   **Rollout & Rollback（演进与回退）**：
    *   菜单栏顶部提供「启用宿主动作接管」总开关（`hostExecutionEnabled`），一键关闭即可立刻停止所有宿主动作派发，无需拔线或修改配置。
    *   全程不向键盘 Flash 写入任何字节，硬件已有 `keymap.json`（7421B）与 `smart_actions.json`（41B）保持原样。

## Risks & Mitigations

| 风险项 | 触发场景与影响 | 缓解与回退措施 |
| :--- | :--- | :--- |
| **R1：硬件层漂移（电容触摸切层）** | 用户误触键盘左下角电容区切到非 `KV_OAI` 层，键盘不再发送 `v.oai.hid` 通知。 | `check_passthrough_health` 检测活动层 `KV_OAI_AG00`–`AG12` 绑定数量，不足 13 个时在菜单栏展示 `LayerDriftWarning` 横幅并提供「重新检查身份/层状态」按钮。 |
| **R2：事件流溢出中断（`InputStreamGap`）** | 广播通道滞后触发 `RecvError::Lagged`，单次 `InputSubscription` 终止。 | `WideKeyMerger` 在缺口处清空按住状态防卡键；`Store.hostInputTask` 捕获 `.inputStreamGap` 后自动重建 `InputSubscription` 并记录告警计数。 |
| **R3：跨应用激活焦点竞态与剪贴板覆盖** | `NSWorkspace` 激活目标应用耗时 > 30ms，或用户在 150ms 内连按两次粘贴键。 | 发送 `CGEvent` 前轮询确认 `frontmostApplication`（上限 300ms）；连续粘贴合并恢复计时器，始终保留首次捕获的用户原始剪贴板快照。 |
| **R4：LaunchServices 非登录 Shell `PATH` 缺失** | 从 Finder/菜单栏启动的应用 `PATH` 极简，找不到 `herdr` / `dsh` 二进制或旋钮高频触发子进程堆积。 | 支持 `MI_HERDR_BIN` / `MI_DSH_BIN` 环境变量覆盖并检索常用安装路径（`/opt/homebrew/bin`、`~/.local/bin`）；适配器内部串行化执行并设 2s 超时。 |
| **R5：`Q1` 实机采样发现摇杆发 `v.oai.rad` 或旋钮按压无 `AG` 码** | 若当前 `profile_index: 0` 下摇杆配置为连续角度通知 `v.oai.rad` 而非 4 扇区 `AG15`–`AG18`。 | `VerifiedInputDecoder` 保留 `AG15`–`AG18` 离散映射的同时，记录 `v.oai.rad` 角度扇区量化候选，不在未获授权写设备的情况下修改硬件配置。 |

--------------------------------------------------------------------------------

## Additional Resources

*   项目总体规格与证据：[`../spec/SPEC.md`](../spec/SPEC.md)、[`../spec/EVIDENCE.md`](../spec/EVIDENCE.md)、[`../implementation-plan.md`](../implementation-plan.md)
*   原厂 `Input 1.1.0` 架构与协议分析：[`../input-architecture.md`](../input-architecture.md)、[`../protocol-readonly.md`](../protocol-readonly.md)、[`../protocol-candidates.md`](../protocol-candidates.md)
*   开源社区参考实现：重点参考 `schacon/micro-manager`、`thefotes/Dispatch`、`camwilso/openboard`、`deepdishgary/CM2-herdr`、`A1mAssist/codex-micro-adapter`。
*   历史协议重建设计文档：[`./input-protocol-reconstruction.md`](./input-protocol-reconstruction.md)

## Future Improvements

*   **单键 RGB 状态灯实时联动（`v.oai.thstatus` / `v.oai.rgbcfg`）**：在宿主执行层稳定后，按当前激活的 Agent Profile 及 Herdr 会话状态点亮 13 颗按键 LED。
*   **前台应用自动切 Profile（`T28 / US-42`）**：监听 `NSWorkspace.didActivateApplicationNotification`，当用户手动切换到 Antigravity 或终端窗口时，可选自动跟随切换宿主生效 Profile。
*   **屏幕无焦点径向菜单与速查浮窗（`T26 / T27`）**：基于 `NSPanel`（`.nonactivatingPanel`）展示右上角摇杆扇区提示与当前 Profile 12 键速查浮层（Cheat Sheet）。

--------------------------------------------------------------------------------

## Detailed Implementation Plan

实现按四个可独立构建与验证的阶段（Phases 0–3）推进，每个阶段结束均设明确的测试检查点。

### Phase 0: 自验证脚手架与 `skills/verify-micro-input`

#### 1. [`../../scripts/run`](../../scripts/run) (Modify)

*   **Type / Scope**: Python 启动器与 QA 隔离环境脚本
*   **Proposed Changes**:
    *   **Rationale**: 修复 `--verify` 在 `/tmp` 下创建应用包及缺乏审计日志与并发锁的问题，支持无人值守合成自验证。
    *   默认将 `--verify` 运行目录创建在 `<repo>/.scratch/qa-runs/<timestamp>-<pid>/`（支持 `MI_QA_RUN_DIR` 覆盖，并通过 `Path.resolve()` 校验绝不位于 `/tmp` 或 `/private/tmp` 下）。
    *   在 `<repo>/.scratch/qa-runs/qa.lock` 获取非阻塞 `flock` 排他锁；默认固定 `CFBundleIdentifier` 为 `com.semanticcraft.microinput.qa`（若传入 `--unique-bundle-id` 则附带短后缀并跳过全局单实例锁）。
    *   设置环境变量 `MI_DOCUMENT_DIR=<run_dir>/documents` 与 `MI_HOST_AUDIT_LOG=<run_dir>/evidence/audit.jsonl`，打印 `QA_APP_PATH=<app>`、`QA_RUN_DIR=<run_dir>` 与 `QA_AUDIT_LOG=<audit_log>`。
    *   在 `finally` 及信号中断中终止子进程、删除 `.app` 副本与 `~/Library/Preferences/com.semanticcraft.microinput.qa*.plist`，保留 `<run_dir>/evidence/` 下的审计日志与运行回执。

#### 2. `../../scripts/tests/test_run_verify_launcher.py` (Create)

*   **Type / Scope**: Python 单元测试（纳入 `scripts/check`）
*   **Proposed Changes**:
    *   **Rationale**: 验证 `scripts/run --verify` 的路径校验、固定 Bundle ID、并发锁冲突拒绝、信号中断回收与证据目录保留逻辑。
    *   测试场景覆盖：拒绝 `/tmp` 与 `/private/tmp` 路径、固定 `CFBundleIdentifier` 写入校验、单实例锁冲突返回非零退出码、子进程超时终止后清理 `.app` 且保留 `evidence/` 目录。

#### 3. `../../skills/verify-micro-input/SKILL.md` & `../../skills/verify-micro-input/features/README.md` (Create)

*   **Type / Scope**: 项目级 Agent 自验证技能（通过 `.agents/skills/verify-micro-input` 软链接线，软链记入 `.git/info/exclude`）
*   **Proposed Changes**:
    *   **Rationale**: 提供符合 `project-verification` 规范的六节验收流程（`Launch`、`Doctor`、`Drive`、`Evidence`、`Cleanup`、`Helpers`）及核心功能地图。
    *   定义 `result.json` 结构：`{"run_id": String, "timestamp_ms": UInt64, "git_sha": String, "scenario": String, "status": "PASS" | "FAIL" | "BLOCKED", "receipt_count": Int, "verified_features": [String], "cleanup_confirmed": Bool}`。
*   **Phase 0 Checkpoint**: `python3 -B -m unittest discover -s scripts/tests -p 'test_*launcher.py'` 退出码为 0。

---

### Phase 1: 物理输入映射、宽键防抖与透传健康检查（`T04 / E-094`）

#### 4. [`../../crates/mi-transport/src/lib.rs`](../../crates/mi-transport/src/lib.rs) (Modify)

*   **Type / Scope**: Rust 传输层合成输入扩展
*   **Proposed Changes**:
    *   **Rationale**: 支持按任意指定 `AG00`–`AG18` 绑定与按/放状态向合成会话注入 64B `v.oai.hid` 帧。
    *   在 `#[cfg(feature = "synthetic-backend")]` 下的 `trait HidTransport` 与 `SyntheticDiscovery` 新增 `fn emit_synthetic_binding(&self, binding_id: &str, pressed: bool) -> Result<(), TransportError>`。

#### 5. [`../../crates/mi-session/src/input.rs`](../../crates/mi-session/src/input.rs) (Modify)

*   **Type / Scope**: Rust 设备会话输入解码与 2U 宽键合并状态机
*   **Proposed Changes**:
    *   **Rationale**: 提供 `AG00`–`AG18` 的物理映射元数据与底部 2U 宽键（`AG10` + `AG11`）合并防抖，同时保持现有 `CandidateInputDecoder` 兼容已有候选观测测试。
    *   定义 `pub struct PhysicalControlSpec { pub binding_id: &'static str, pub logical_switch: &'static str, pub physical_control: &'static str, pub keycap_group: &'static str, pub action_role: &'static str }` 与查询函数 `pub fn verified_physical_mapping(binding_id: &str) -> Option<PhysicalControlSpec>`（严格遵循 Technical Plan §3.1 表格）。
    *   为 `InputEvent` 增加字段：`pub physical_control: Option<String>`、`pub keycap_group: Option<String>`、`pub action_role: Option<String>`。
    *   新增 `pub struct VerifiedInputDecoder` 与 `DeviceSession::subscribe_verified_input(self: &Arc<Self>) -> InputMonitor`：启用 `WideKeyMerger`（`active_wide_switches: BTreeSet<String>`），将 `AG10` 与 `AG11` 合并输出为 `binding_id: Some("AG10")`, `logical_switch: Some("logical-10")`, `action_eligible: true`；在 `InputStreamGap` 或 `close()` 时清空 `active_wide_switches`。

#### 6. `../../crates/mi-model/src/host_router.rs` (Create) & [`../../crates/mi-model/src/lib.rs`](../../crates/mi-model/src/lib.rs) (Modify)

*   **Type / Scope**: Rust 纯内存宿主路由、透传健康检查与键码映射模块
*   **Proposed Changes**:
    *   **Rationale**: 将宿主 Profile 路由、硬件透传层健康检查与 `ActionKind` 到 `ResolvedHostAction` 的转换封装为无副作用的纯 Rust 函数（注：严禁新增或删除 `TODO(UNKNOWN-01..09)` 注释，确保 [`../../scripts/check-boundaries.py`](../../scripts/check-boundaries.py) 精确集合校验通过）。
    *   定义核心数据结构与函数签名：
        *   `pub struct HostIdentityStatus { pub version: Option<String>, pub profile_index: Option<u32>, pub layer_index: Option<u32> }`
        *   `pub enum PassThroughHealth { Ready, LayerDriftWarning { active_profile: u32, active_layer: u32, bound_ag_count: u32 }, SnapshotMissing, Disconnected }`
        *   `pub enum ResolvedHostAction { Shortcut { key: String, key_code: u16, modifiers: Vec<String>, target_bundle_id: Option<String> }, PasteText { text: String }, OpenApp { bundle_id: String }, OpenUrl { url: String }, AgentCommand { adapter: String, command: String }, FocusSession { adapter: String, session: String } }`
        *   `pub fn check_passthrough_health(snapshot: Option<&DeviceConfig>, identity: Option<&HostIdentityStatus>) -> PassThroughHealth`（检查活动层 `layer.keys` 中匹配的 `"KV_OAI_AG00"`..`"KV_OAI_AG12"` 字符串数量）。
        *   `pub fn binding_to_logical_slot(binding_id: &str) -> Option<&'static str>`（含 `AG00` $\rightarrow$ `logical-0`, `AG01` $\rightarrow$ `logical-1`, `AG15`–`AG18` $\rightarrow$ `logical-16`–`logical-19`）。
        *   `pub fn macos_virtual_key_code(key: &str) -> Option<u16>`：提供完整 macOS `CGKeyCode` 映射表（`a`..`z`, `0`..`9`, `return`=`0x24`, `tab`=`0x30`, `space`=`0x31`, `delete`/`backspace`=`0x33`, `escape`=`0x35`, `grave`/`backtick`=`0x32`, `minus`=`0x1B`, `equal`=`0x18`, `leftbracket`/`[`=`0x21`, `rightbracket`/`]`=`0x1E`, `backslash`=`0x2A`, `semicolon`=`0x29`, `quote`=`0x27`, `comma`=`0x2B`, `period`=`0x2F`, `slash`=`0x2C`, `left`=`0x7B`, `right`=`0x7C`, `down`=`0x7D`, `up`=`0x7E`, `f1`..`f12`）。
        *   `pub fn resolve_host_action(doc: &LocalDocumentV2, host_active_profile_id: Option<&str>, binding_id: &str, is_pressed: bool) -> Option<ResolvedHostAction>`。
*   **Phase 1 Checkpoint**: `cargo test --locked -p mi-session -p mi-model` 全部通过。

---

### Phase 2: 宿主偏好持久化、UniFFI 桥接与 Swift 原生执行层（`T17 / T18 / T20`）

#### 7. [`../../crates/mi-model/src/storage.rs`](../../crates/mi-model/src/storage.rs) & [`../../crates/mi-model/src/action_library.rs`](../../crates/mi-model/src/action_library.rs) (Modify)

*   **Type / Scope**: Rust 宿主偏好存储与动作库能力元数据
*   **Proposed Changes**:
    *   **Rationale**: 在 `HostPreferences` 中持久化 `host_active_profile_id` 与 `host_execution_enabled`，并在 `action_library::capability` 中如实开放已接通宿主动作的 `execution_available` 标记。
    *   在 `storage.rs` 的 `HostPreferences` 中新增 `#[serde(default)] pub host_active_profile_id: Option<String>` 与 `#[serde(default)] pub host_execution_enabled: bool`；在 `DocumentStore` 新增 `host_active_profile_id()`, `host_execution_enabled()`, `set_host_active_profile(profile_id: Option<&str>)`, `set_host_execution_enabled(enabled: bool)`。
    *   在 `action_library.rs` 中将 `Shortcut`、`Text`、`OpenUrl`、`OpenApp` 以及 `adapter` 为 `"herdr"` 或 `"dsh"` 的 `AgentCommand` / `FocusSession`（当 `session != "unselected"` 时）的 `execution_available` 设为 `true`，`unavailable_reason` 设为空字符串；`DeviceKey`、`LocalMacro`、`LocalMulti`、`SkillReference` 及未验证适配器保持 `execution_available = false`。

#### 8. [`../../crates/mi-ffi/src/lib.rs`](../../crates/mi-ffi/src/lib.rs), [`../../crates/mi-ffi/src/input_monitor.rs`](../../crates/mi-ffi/src/input_monitor.rs) & [`../../crates/mi-ffi/tests/input_observation.rs`](../../crates/mi-ffi/tests/input_observation.rs) (Modify)

*   **Type / Scope**: UniFFI 跨语言桥接层与输入观测回归测试
*   **Proposed Changes**:
    *   **Rationale**: 使用统一的 `document_response`（`Result<String, BridgeError>`）向 Swift 返回宿主偏好，并在 `InputSubscription` 中透传物理映射字段。
    *   在 `document_response` 的 JSON 中新增 `"host_active_profile_id": store.host_active_profile_id()` 与 `"host_execution_enabled": store.host_execution_enabled()`。
    *   在 `MicroInput` 新增导出方法：
        *   `pub async fn set_host_active_profile(&self, profile_id: Option<String>) -> Result<String, BridgeError>`
        *   `pub async fn set_host_execution_enabled(&self, enabled: bool) -> Result<String, BridgeError>`
        *   `pub async fn check_passthrough_health(&self) -> String`（返回序列化后的 `PassThroughHealth` JSON）
        *   `pub async fn resolve_input_action(&self, binding_id: String, is_pressed: bool) -> Option<String>`（返回序列化后的 `ResolvedHostAction` JSON）
        *   `pub async fn subscribe_verified_input(&self) -> Result<Arc<InputSubscription>, BridgeError>`
        *   `pub async fn install_coding_agent_presets(&self) -> Result<String, BridgeError>`
    *   在 `#[cfg(feature = "synthetic-backend")]` 下新增 `pub async fn emit_synthetic_binding_event(&self, binding_id: String, pressed: bool) -> Result<(), BridgeError>`。
    *   在 `input_monitor.rs` 的 `InputSubscription::next_event` 中透传 `event.physical_control`、`event.keycap_group`、`event.action_role` 与 `event.action_eligible`；在 `input_observation.rs` 新增针对 `subscribe_verified_input` 与 `WideKeyMerger` 的测试（保持原 `candidate_observation_retains_binding_and_never_exposes_payload_or_mapping` 100% 原样通过）。

#### 9. `../../app/MicroInput/Sources/MicroInput/HostActionExecutor.swift` (Create)

*   **Type / Scope**: Swift 原生 macOS 动作执行器（`T17 / T18`）
*   **Proposed Changes**:
    *   **Rationale**: 封装 `CGEvent`、`NSPasteboard`、`NSWorkspace` 与 `MI_HOST_AUDIT_LOG` 审计回执写入，支持注入式单元测试。
    *   定义 `struct HostActionReceipt: Codable, Equatable`：
        `{"timestamp_ms": UInt64, "profile_id": String, "binding_id": String, "action_type": String, "target": String, "text_byte_len": Int?, "outcome": String, "detail": String}`。
    *   定义 `protocol HostActionExecuting` 与 `final class LiveHostActionExecutor: HostActionExecuting`（含 300ms 前置焦点轮询确认、4 MiB 剪贴板深拷贝快照与防重入 150ms 合并恢复定时器、`MI_HOST_AUDIT_LOG` JSONL 追加写入）以及 `final class RecordingHostActionExecutor: HostActionExecuting`。

#### 10. [`../../app/MicroInput/Sources/MicroInput/Store.swift`](../../app/MicroInput/Sources/MicroInput/Store.swift), [`../../app/MicroInput/Sources/MicroInput/Model.swift`](../../app/MicroInput/Sources/MicroInput/Model.swift) & [`../../app/MicroInput/Sources/MicroInput/InputMonitor.swift`](../../app/MicroInput/Sources/MicroInput/InputMonitor.swift) (Modify)

*   **Type / Scope**: SwiftUI 状态容器、模型解码与输入监视面板
*   **Proposed Changes**:
    *   **Rationale**: 在 `Store` 中维护独立于窗口生命周期的 `hostInputTask`，并在 `extension Backend` 提供默认实现以确保 `NavigationTests.swift` 中的 `SpyBackend` / `DelayedBackend` 编译通过。
    *   在 `Model.swift` 的 `DocumentView` 中新增 `let host_active_profile_id: String?` 与 `let host_execution_enabled: Bool?`；新增 `ResolvedHostActionPayload: Codable` 与 `PassThroughHealthView: Codable`。
    *   在 `Store.swift` 的 `protocol Backend` 及 `extension Backend` 中添加新 FFI 方法及默认实现；在 `Store` 中新增 `@Published var hostActiveProfileId: String?`、`@Published var hostExecutionEnabled = false`、`@Published var passThroughHealth = PassThroughHealthView.disconnected`、`@Published var lastHostActionReceipt: HostActionReceipt?` 及 `hostInputTask: Task<Void, Never>?`（在 `InputStreamGap` 时自动重订阅）。
    *   在 `InputMonitor.swift` 的 `eventSummary` 中增加对 `.pressed` 与 `.released` 的展示分支（显示已核实逻辑槽位、物理控件与键帽组）。
*   **Phase 2 Checkpoint**: `scripts/build && swift test --package-path app/MicroInput -Xswiftc -DMI_SYNTHETIC_BACKEND` 通过。

---

### Phase 3: 三大 Coding Agent 预设、适配器与界面集成（`T21–T23`）

#### 11. [`../../crates/mi-model/src/preset.rs`](../../crates/mi-model/src/preset.rs) (Modify)

*   **Type / Scope**: Rust 内置 Coding Agent 预设生成器
*   **Proposed Changes**:
    *   **Rationale**: 一键在当前本地草稿中生成或更新 `Antigravity`、`Herdr`、`DeepSeek Coder (DSH)` 三套完整 Profile（覆盖 `AG00`–`AG18` 对应的 `logical-0`..`logical-14` 及 `logical-16`..`logical-19`）。
    *   新增 `pub fn install_agent_starter_profiles(draft: &mut Draft) -> Result<Vec<String>, String>`：为每个新建 Profile 的首选层调用 `set_control_settings`（`joystick_mode: "logical_sectors"`, `active_sectors: 4`），再按 Technical Plan §5 表格写入动作绑定。

#### 12. `../../app/MicroInput/Sources/MicroInput/AgentAdapters.swift` (Create)

*   **Type / Scope**: Swift Coding Agent 专用适配器（`T22 / T23`）
*   **Proposed Changes**:
    *   **Rationale**: 实现 `HerdrAdapter` 与 `DshAdapter` 的二进制解析（支持 `MI_HERDR_BIN` / `MI_DSH_BIN` 覆盖）、串行超时调用、会话聚焦与非提交式文本注入。

#### 13. [`../../app/MicroInput/Sources/MicroInput/MicroInputApp.swift`](../../app/MicroInput/Sources/MicroInput/MicroInputApp.swift) & [`../../app/MicroInput/Sources/MicroInput/Workbench.swift`](../../app/MicroInput/Sources/MicroInput/Workbench.swift) (Modify)

*   **Type / Scope**: SwiftUI 菜单栏面板、合成场景入口与工作台动作能力视图
*   **Proposed Changes**:
    *   **Rationale**: 在菜单栏提供宿主 Profile 切换、透传健康度提示与预设安装入口；在 `MicroInputApp.swift` 和 `Store.swift` 的 `MI_SYNTHETIC_SCENARIO` 白名单中加入 `"host-execution"`；在 `Workbench.swift` 的 `ActionCapabilityView` 中按 `capability.execution_available` 分支显示「宿主可执行」或「当前不可执行」。

#### 14. `../../crates/mi-model/tests/host_router.rs` & `../../app/MicroInput/Tests/MicroInputTests/HostExecutionTests.swift` (Create)

*   **Type / Scope**: Rust 与 Swift 自动化回归测试
*   **Proposed Changes**:
    *   **Rationale**: 覆盖宽键防抖、第 0 行反序映射、`AG15`–`AG18` 扇区槽位映射、草稿浏览与宿主生效 Profile 隔离、透传层漂移告警、三大 Agent 预设安装以及合成 HID 事件端到端触发 `RecordingHostActionExecutor` 的完整断言。

#### 15. [`../spec/EVIDENCE.md`](../spec/EVIDENCE.md), [`../spec/UNKNOWNS.md`](../spec/UNKNOWNS.md), [`../implementation-plan.md`](../implementation-plan.md), [`../../README.md`](../../README.md), [`../../AGENTS.md`](../../AGENTS.md) (Modify)

*   **Type / Scope**: 项目规格、证据记录与 Agent 验收说明文档
*   **Proposed Changes**:
    *   **Rationale**: 登记宿主执行与 Agent 预设自验证证据，并在 `AGENTS.md` 登记交付前运行 `verify-micro-input` 的约定（保持 `crates/**/*.rs` 中 `TODO(UNKNOWN-01..09)` 集合不变）。
*   **Phase 3 Checkpoint**: 运行完整 `scripts/check` 统一执行并验证退出码为 0，随后执行 `verify-micro-input` 端到端自检，确认 `result.json` 为 `PASS` 且 QA 进程与临时偏好已清理。

### Verification Plan

*   **Compilation & Bridge Generation**:
    *   `scripts/build`（当前构建 C-ABI 静态库与 `target/MicroInput.app`；合成场景在运行时由 `MI_SYNTHETIC_SCENARIO` 选择）。
*   **Unit & Synthetic Bridge Tests**:
    *   `cargo test --locked --workspace --all-features`（包含新增 `crates/mi-model/tests/host_router.rs` 与 `mi-session` 宽键合并测试）。
    *   `swift test --package-path app/MicroInput -Xswiftc -DMI_SYNTHETIC_BACKEND` 与 `swift test --package-path app/MicroInput`（包含新增 `HostExecutionTests.swift`）。
    *   `python3 -B -m unittest discover -s scripts/tests -p 'test_*launcher.py'`（包含新增 `test_run_verify_launcher.py`）。
*   **Static Analysis & Boundary Check**:
    *   `cargo fmt --all -- --check` 与 `python3 scripts/check-boundaries.py`（由 `scripts/check` 统一执行并验证退出码为 0）。
*   **End-to-End Self-Verification (`verify-micro-input`) & Live Hardware Check**:
    *   **合成自验证闭环**：按 `skills/verify-micro-input/SKILL.md` 启动带 `MI_SYNTHETIC_SCENARIO=host-execution` 的隔离 QA 实例，自动注入 `AG00`–`AG18` 帧并切换 `Antigravity` / `Herdr` / `DSH` Profile，回读 `MI_HOST_AUDIT_LOG`，确认 `result.json` 为 `PASS` 且 QA 进程与临时偏好已清理。
    *   **45 秒实机物理采样（`T04 / E-094`）**：由操作者配合运行 `scripts/observe-input --observe-once`，实测 12 个键帽、左上旋钮、右上摇杆物理触发。

--------------------------------------------------------------------------------

<!-- Provenance: Indicates initial structural drafting via EGM. -->

*Drafted with EGM*
