# PStack 接力队列（四款桌面应用深度控制）

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

A–E 是第一轮通用交互迁移历史，不代表四款应用深度控制通过。2026-11-05 用户明确只做 Antigravity、Cindy、ZCode、桌面 DeepSeek Harness，并保留 Cindy 原生能力。上轮未开始的 F–K 由下列切片替代，不建立 Paseo 适配器，不强制复制 MCP/CLI 出口。

源码与边界见[四应用接口](four-app-interfaces.md)，实现细节、依赖和完整验收见[四应用计划](designs/four-app-control.md)，状态归 [#81](https://github.com/semantic-craft/micro-input/issues/81) 及责任票。各片状态由勾选与证据记录确认；软件、原生安装和实体结果分别验收。

### [x] Slice F: 保留 Cindy 原生输入和灯光所有权
- **Outcome**: 增加 Cindy 原生所有权的活跃握手和代次确认；原生持有期间暂停 Micro Input 重叠按键、PTT 与灯光，未知时保持暂停。
- **Done Predicate**: 同一宽键/旋钮输入投递两端后业务动作仅一次；切 Profile、失联和旧代回调不双执行；不切换 Cindy 设备开关或改 keymap。 不能只看进程存在；失联标 unknown，移交其他应用前取得释放回执，未确认就不接管。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: 无。责任票 #82。
- **Evidence**: .scratch/qa-runs/1793813638607-83009/evidence/result.json
- **Verification scope**: 软件片通过全量离线检查、Cindy 源码 187 项测试及完整类型检查、真实 UDS 双端配对 8 事件；安装版 Cindy、实体按键和灯光视觉为 NOT_RUN。补丁、恢复限制及重跑命令见 [原生所有权桥](../integrations/cindy/README.md)。

### [x] Slice G: 接通 ZCode 同实例状态与精确导航
- **Outcome**: 通过最小桌面桥读取 v4 任务索引，在两个同名会话间准确打开目标。
- **Done Predicate**: 两个工作区与同名任务身份不混淆；selected session 回读吻合；断连/换代/删除后不改开其他任务；默认不读正文。 未授权或越权请求零元数据读取。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: 无。责任票 #33。
- **Evidence**: .scratch/qa-runs/1793838585847-10402/evidence/result.json
- **Verification scope**: 软件片通过全量离线门禁及六个隔离应用场景；固定 ZCode 源码检查、16 项原生边界检查和真实 UDS Swift 双端联测通过。真实 renderer store 使用合成任务 owner；完整 Electron 窗口、安装版与实体为 NOT_RUN。补丁与重跑入口见 [ZCode 桥](../integrations/zcode/README.md)。

### [x] Slice H: 接通 DSH 同 Host 状态与精确导航
- **Outcome**: 先核验 0.2.1-rc.5 接口，再在同 Host 读取状态并打开准确会话。
- **Done Predicate**: activate=false 的订阅不启动后台会话；pendingInput 变化不漏；导航回读 ID；停止桥不停止 Host；无认证零读取。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: 无。责任票 #32。
- **Evidence**: .scratch/qa-runs/1793841784754-43891/evidence/result.json
- **Verification scope**: 软件片通过全量离线门禁及七个隔离应用场景；固定 rc.5 原生源码契约、类型、lint 和 191 项测试通过。真实 UDS Swift 双端确认两个目标、四次待回应变化、零未授权读取及关闭后 Host 存活；最新 Swift 回归在合成和生产 ABI 各 13 项通过。配对导航端口为合成选择，真实 UI store 另有源码测试。日常 DSH 补丁安装、完整 Electron 窗口和实体均为 NOT_RUN；冷目录发现未实现。见 [DSH 桥](../integrations/dsh/README.md)。

### [ ] Slice I: 验证 Antigravity 官方桌面扩展与状态事件
- **Outcome**: 只读确认安装版 agentapi/Sidecars/Hooks 契约，再在隔离测试工作区接元数据事件。
- **Done Predicate**: 记录版本及能力；两会话事件不串、重复事件去重、断流标 stale；Stop fullyIdle=false 不报完成；不读 transcriptPath。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: 无。责任票 #83。

### [ ] Slice I2: 核验 Antigravity 既有会话目录与精确导航
- **Outcome**: 先用官方可用入口，缺少时用 REA 验证会话元数据、工作区与聚焦协议，仅限只读和测试导航。
- **Done Predicate**: 两个既有测试会话目录身份与 UI 选中回读一致；同名跨工作区不混；缺 schema/合法认证就失败关闭；不读取 token 或绕过鉴权。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: I。责任票 #83。

### [ ] Slice J: 补 Cindy 既有任务目录和导航确认
- **Outcome**: 在原生服务旁加受限元数据桥，沿用 session 深链并回读目标。
- **Done Predicate**: 只列显式授权范围的既有任务；同名会话正确；账号/实例换代失效；delivered 与 confirmed 分开；不放宽 Local Agent API 隔离。 未授权或越权请求零元数据读取。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: F。责任票 #82。

### [ ] Slice K1: 向 ZCode 准确发送并停止指定执行
- **Outcome**: 接 createSession、sendText 和携带 execution ID 的 stop；运行中发送的 steering 语义单独确认。
- **Done Predicate**: 两个会话只影响选中者；迟到 stop 不停止新 execution；重复命令不重复发送；超时结果未知时不换 ID 重放。 新建绑定工作区并回读新 ID；运行中插话不误建新会话，不把排队当 steering。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: G。责任票 #33。

### [ ] Slice K2: 向 DSH 准确发送并停止当前轮次
- **Outcome**: 接 create、prompt queue/steer、requestId 与 cancel。补同 Host 的预期 turn 核对，分别显示当前轮次和队列。
- **Done Predicate**: queue 与 steer 两种结果分别确认；cancel 后队列可能继续时不报全部停止；重复请求无双发；切会话无误停。 新建绑定工作区并回读新 ID；同会话旧 Stop 遇新轮次必须零取消，未能证明原子性就拒绝发送。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: H。责任票 #32。

### [ ] Slice K3: 用 Antigravity agentapi 创建和发送
- **Outcome**: 创建绑定 projectId，发送绑定 conversationId，取得结果确认。
- **Done Predicate**: 两个测试会话只改目标；不存在目标零重定向；超时不重发；运行中消息是否 steering 单独验证。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: I、I2。责任票 #83。

### [ ] Slice L1: 接 ZCode 请求审批与思考档位
- **Outcome**: 按 interactionId/type 处理批准拒绝、问题和计划，复用 switchModelConfig。
- **Done Predicate**: 过期/重复请求零误批；问题不当权限批准；模型无该档位不发送；回读模型和档位一致。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: K1。责任票 #33。

### [ ] Slice L2: 接 DSH 请求审批与思考档位
- **Outcome**: 先证明当前桌面 approval 接线，再接 request 级回答和 selectModel。
- **Done Predicate**: 取消/迟到答案零副作用；会话切换不改批其他请求；真实模型档位回读；无法证明接线时保留缺口。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: K2。责任票 #32。

### [ ] Slice L3: 补 Antigravity 请求与模型控制缺口
- **Outcome**: 逐能力用 REA 核实停止、审批、模型/思考入口，按能力分 PR；语音归 O1。
- **Done Predicate**: 每条候选都取得 schema、合法鉴权和两个测试会话结果；字符串存在不算完成；升级/认证失效关闭；不开放任意 RPC。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: I2、K3。责任票 #83。

### [ ] Slice M: 补 Cindy 原生任务动作缺口
- **Outcome**: 复用原生服务补准确停止、请求绑定、目标 Prompt/steering 及工作区/待办动作。
- **Done Predicate**: 已有任务槽/PTT/思考/灯光行为不退化；停止/审批固定身份，过期零操作；原生未读事实和待办轮次独立核验。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: J。责任票 #82。

### [ ] Slice N: 交付四应用稳定槽位和历史待办导航
- **Outcome**: 共享稳定目标选择、六槽固定/最近、项目/工作区导航、历史和本轮不重复的待办轮询。
- **Done Predicate**: 四应用相同名称/不同实例不混淆；新事件重新入队；未知未读不推断；历史回到准确会话；Cindy 原生槽位可复用且不双消费。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: G、H、I2、J；写动作按 K/L/M 能力就绪。责任票 #84。

### [ ] Slice O1: 逐应用补齐听写和语音回复控制
- **Outcome**: Cindy 复用原生语音；其余逐款确认并接入按住听写、释放停止、提交前停止，以及独立的语音回复开关。缺少原生能力的开源应用按其语音服务增量实现。
- **Done Predicate**: 四应用分别确认开始/停止和目标绑定；听写转写与发送分开；开启/关闭语音回复须回读真实模式并验证输出。模型/接口不支持时保持该应用未完成，不以禁用或听写成功代替。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: F；对应应用身份与动作片就绪。责任票 #84。

### [ ] Slice O: 补齐四应用实体交互与状态反馈
- **Outcome**: 逐应用接滚动/跳底、八向 Prompt/Stop、可选回弹与逐键/双区反馈，宽键沿用 O1 已验语音能力。
- **Done Predicate**: 每应用独立证明功能：取消零动作、听写先停后交、真实状态对应 HUD 和灯光；唯一灯光发送者，切层/退出释放明确；语音缺口单列子片，不能以 disabled 签收。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: F、N、O1；对应动作能力就绪。责任票 #84。

### [ ] Slice P: 完成四应用安装与日常验收
- **Outcome**: 最终构建在四应用中完成相同任务脚本，记录软件/安装/实体三层证据。
- **Done Predicate**: clean commit 全量机器验收通过；每应用两会话完成导航/发送/停止/审批/状态反馈；升级/断连恢复与原映射保留；未完成项不关闭总票。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: F 至 O 的全部任务，含 I2、K1–K3、L1–L3、O1。责任票 #84。
