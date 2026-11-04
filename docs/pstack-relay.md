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
- **Outcome**: 梳理 固定提交的 `mattt/paseo-creator-micro` 与当前 `Micro Input` 的逐项能力对比表；明确 8 套 Agent（重点为 `Herdr`、`Antigravity`、`千问办公`、`Cindy`、`Codex`）的按键/旋钮/摇杆布局与 Codex/Cindy 防冲突边界；装好 `scripts/pstack-relay` 与 `.agents/hooks.json` 自动接力钩子。
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

## 当前覆盖与接续

A–E 是第一轮通用交互迁移的历史完成记录；其回执不证明 Paseo 全功能或八套应用实机覆盖。原对表已由[当前能力清单与架构决策](designs/paseo-coverage.md)替代，避免同一能力同时标记“完成”和“待补齐”。

### [ ] Slice F: 原生集成所有权与适配器能力边界
- **Outcome**: Cindy 保留原生键盘集成；逐动作确定 Cindy/Codex 与 Micro Input 的唯一消费者，缺失能力才由本项目补充。
- **Done Predicate**: 原生能力与宿主补充能力逐项有源码定位；用可重放输入证明单次物理事件不会双执行；不支持的目标明确显示缺口。更新现有适配器入口与适用回归，不修改用户设备映射。

### [ ] Slice G: 会话与工作区语义控制
- **Outcome**: 覆盖能力清单的项目/工作区/最近与固定六槽、准确会话历史、待办轮询、创建、权限批准/拒绝、停止、思考级别、语音模式与准确目标 Prompt/steering。
- **Done Predicate**: 适配器返回稳定身份与真实状态；目标/请求过期零操作；工作区选择、子 Agent 筛选和未读语义均有测试及实际适配器证据。快捷键占位不能关闭缺口；实施时按适配器拆为可独立验收的小片。

### [ ] Slice H: 逐键与双区运行时状态灯
- **Outcome**: 状态、选中项、失败与思考级别能够反馈到正确键和灯区，并遵守当前层及其他客户端的所有权。
- **Done Predicate**: 核实当前固件的 thstatus/rgbcfg、可亮灯键、效果及释放/恢复；原始映射保留；断连/重连和共存有证据。视觉验收与协议/合成检查分开。

### [ ] Slice I: 完整径向交互与可配置槽位
- **Outcome**: 在现有确认方式之外提供可选的快速回弹确认/慢回中取消，补齐 Stop、图标/颜色与取消按键消费，保留四向 Prompt。
- **Done Predicate**: 边界轨迹、Escape/普通键/旋钮取消均不误发；目标冻结、听写先停后提交通过回归；当前设备的角度/速度与实际手感分别核验。

### [ ] Slice J: 设备服务 MCP 与 CLI 出口
- **Outcome**: 提供与上游五项工具等价的状态、逐键灯、分区灯、清灯和等待输入，以及 CLI/仿真接入。
- **Done Predicate**: 复用唯一设备 session；明确本机访问边界、超时/取消/退出清理；仿真零真实副作用；灯光出口依赖 H 的已验证能力。

### [ ] Slice K: 日常交付与全覆盖收口
- **Outcome**: 登录启动、权限、升级、诊断、实体旋钮/宽键、准确目标、所有灯光/摇杆体验形成可使用的交付版。
- **Done Predicate**: 最终 clean commit 的 scripts/verify-offline 全量通过；记录构建与安装身份、原生及物理缺口；逐项关闭能力清单并核对 #44，不能只用接力全部勾选证明交付。
