# PStack 接力队列（Paseo 交互迁移与多 Agent 通用工作台）

本文件是 PStack 跨对话自动接力的唯一全景队列（对应 `principle-sequence-verifiable-units` 与 `Session pickup` 剧本）。
不在此写死内部函数实现，只维护每个对话切片（Slice）的**用户可感知效果（Outcome）**、**可证伪验收条件（Done Predicate）**与**状态（Status）**。

- 决策账本（`show-me-your-work`）：`.scratch/pstack-relay/decisions.tsv`
- 下一棒提示词快照：`.scratch/pstack-relay/NEXT_PROMPT.md`
- 交接与推进命令：`scripts/pstack-relay status` / `scripts/pstack-relay advance <SLICE_ID> --evidence "<sha/path>"`

---

## 全景切片队列（A → E）

### [x] Slice A: 全景差距对表、防冲突护栏与接力机关就位
- **Playbook**: `Investigation` + `Authoring or modifying a skill` (`encode-lessons-in-structure`)
- **Outcome**: 梳理 `.scratch/reference/paseo-creator-micro` 与当前 `Micro Input` 的逐项能力对比表；明确 8 套 Agent（重点为 `Herdr`、`Antigravity`、`千问办公`、`Cindy`、`Codex`）的按键/旋钮/摇杆布局与 Codex/Cindy 防冲突边界；装好 `scripts/pstack-relay` 与 `.agents/hooks.json` 自动接力钩子。
- **Done Predicate**: `scripts/pstack-relay selftest` 与 `python3 -B -m unittest discover -s scripts/tests -p 'test_pstack_relay_launcher.py'` 通过；产出下一棒 Slice B 的完整 `/poteto-mode` 提示词并写入 `.scratch/pstack-relay/NEXT_PROMPT.md` 及系统剪贴板。
- **Evidence**: docs/pstack-relay.md;scripts/pstack-relay;.agents/hooks.json

### [x] Slice B: 旋钮对话控制（对标 Paseo Dial 顺/逆时针滚动与按压跳底）
- **Playbook**: `Feature` (`how` -> `architect` -> `poteto-agent` -> `verify-micro-input`)
- **Outcome**: 像 `paseo-creator-micro` 一样，旋钮顺/逆时针（`AG14` / `AG13`，对应 `logical-14` / `logical-13`）及旋钮按压（`logical-15`）支持面向目标 Agent 窗口的平滑对话滚动（慢滚/快滚步进合并）与一键跳到底部/聚焦输入区，适配 `Antigravity`、`千问办公`、`Cindy`、`Codex`（`Herdr` 保留已有的面板缩放/侧栏切换或可自定义切换），不抢占非目标窗口焦点，且可在工作台自由改绑。
- **Done Predicate**: 新增滚动/跳底动作或 Agent 命令通过 `cargo test --locked -p mi-model` 与 `swift test --package-path app/MicroInput`；`scripts/verify-offline`（或合成 QA 场景）生成 `PASS` 回执并记录到 `.scratch/pstack-relay/decisions.tsv`。
- **Evidence**: .scratch/qa-runs/1793782564987-84802/evidence/result.json

### [x] Slice C: 跨 Agent 键盘打开历史回溯（Back / Forward）与待办轮询强化
- **Playbook**: `Feature` (`principle-model-the-domain` + `verify-micro-input`)
- **Outcome**: 对标 `paseo-creator-micro` 的 Key 0 / Key 1 导航历史栈：记录通过小键盘打开或聚焦过的 Agent / 应用 / 会话历史（最多 50 条，后退保留前进栈、新打开截断前进栈），无需依赖单一应用内部插件 API 即可在 `Herdr`、`Antigravity`、`千问办公`、`Cindy`、`Codex` 之间跨应用后退/前进，并与“下一个待处理会话”无冲突配合。
- **Done Predicate**: 状态机单测覆盖后退、前进、分叉截断、重复去重与容量上限；`scripts/check` 全量通过；更新 `.scratch/pstack-relay/decisions.tsv`。
- **Evidence**: .scratch/qa-runs/1793784205220-43399/evidence/result.json

### [x] Slice D: 摇杆可配置 Prompt 槽位与防冲突提交控制（对标 Paseo Radial & Submit）
- **Playbook**: `Feature` + `interrogate`
- **Outcome**: 对标 `paseo-creator-micro` 的八向径向菜单与四向快捷指令：支持将常用工程指令（如 Continue、Test、Review、Commit、Summary 或自定义文本）绑定到摇杆扇区或径向槽位，派发到 `Antigravity`、`千问办公`、`Cindy`、`Codex` 时自动聚焦目标输入区、安全暂存并恢复原剪贴板，且严格遵守“默认只粘贴不自动回车，仅在显式配置或按提交键时停止活跃听写并提交”。
- **Done Predicate**: 路由与执行器测试覆盖剪贴板保护、听写先停后交与防误发；通过 `/interrogate` 审查与 `scripts/check`；更新 `.scratch/pstack-relay/decisions.tsv`。
- **Evidence**: .scratch/qa-runs/1793786742047-36222/evidence/result.json

### [x] Slice E: 全量回归、日常盲操速查 HUD 与本机安装收口
- **Playbook**: `Shipping` + `verify-micro-input`
- **Outcome**: 汇总核对 8 套 Agent 预设的按键速查 HUD 显示、配置导入导出兼容性与未修改预设的无损升级；跑通 `scripts/verify-offline` 全套机器自动门禁（覆盖旋钮滚动/跳底、跨 Agent 历史后退/前进、工程 Prompt 粘贴/提交、2U 宽键合并与本地宏共 24 条宿主回执），并同步更新本机 `/Applications/Micro Input.app` 供日常直接使用（使用中发现问题再随时修复）。
- **Done Predicate**: `scripts/verify-offline` 与 `scripts/pstack-relay status --require-all-done` 全部 `PASS`。
- **Evidence**: .scratch/qa-runs/1793801884388-98896/evidence/result.json

---

## Slice A 成果：`paseo-creator-micro` vs `Micro Input` 全景能力对表与防冲突边界

| 能力维度 | `mattt/paseo-creator-micro` 做法 | `Micro Input` 当前状态 | 差距与后续切片动作 |
| :--- | :--- | :--- | :--- |
| **适用范围与配置方式** | 仅限 Paseo 0.13+ 插件体系，设置存于 Paseo 插件内 | **已完成**：Rust + SwiftUI 独立原生工作台 + 菜单栏，支持任意自定义 Profile/Layer/宏/Multi，零 Flash 写入切换 | 保持通用架构，所有新增 Agent 能力均作为可自由改绑的 `ActionKind` 提供 |
| **Codex / Cindy 防冲突** | 要求用户手动退出 ChatGPT App 防止抢键 | **已完成**：按 `hostActiveProfileId` + `target_bundle_id` / AX 控件定向路由；菜单栏支持一键暂停与前台自动跟随 | 在 Slice B–D 中严禁发送无目标全局快捷键，所有动作必须带目标应用确认 |
| **底部 2U 宽键按住说话** | `ACT10`+`ACT11` 合并，按下 `Cmd+D`、松开停止 | **已完成（PR #74）**：`WideKeyMerger` 合并 `AG10`+`AG11`，AX 录音状态实测回读（支持 Cindy、Codex、千问办公、Antigravity、Qoder CN） | 在 Slice D 中补齐“若正在听写时触发提交，先停止听写再提交”的联动保护 |
| **下一个待处理 Agent** | Key 6 按等待时长轮询待处理 Agent | **已完成（PR #76）**：T3 Code 待处理会话只读导航与菜单栏/径向菜单接入 | 在 Slice C 中与跨应用历史栈联动记录 |
| **摇杆八向径向菜单** | 8 槽固定/可配（Back/Test/Continue/Commit/Forward/Review/Stop/Summary） | **已完成基础框架（PR #77）**：八槽 UI、旋钮确认、回中取消，当前固定为 T3 导航 + 速查 + 6 个应用打开 | **Slice D** 增加可配置 Prompt / 快捷动作槽位支持 |
| **旋钮滚动与按压跳底** | 顺/逆时针滚动窗口 3 行（按住 15 行，步进合并上限 100 行），按压跳到底部 | **待补齐**：当前 `Herdr` 绑定了侧栏，其他对话型 Agent 的 `logical-13..15` 尚未预置对话滚动与跳底 | **Slice B** 实现目标窗口安全滚动与跳底动作 |
| **Key 0/1 历史前进/后退** | 记录小键盘打开过的最近 50 个 Agent/Workspace 供 Key 0/1 回溯 | **待补齐**：当前 Key 0/1 固定为打开应用和聚焦输入框 | **Slice C** 实现宿主级小键盘导航历史栈（Back / Forward） |
