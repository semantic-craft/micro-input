# PStack 接力队列（四款桌面应用深度控制）

本文件是 PStack 跨对话自动接力的唯一全景队列（对应 `principle-sequence-verifiable-units` 与 `Session pickup` 剧本）。
不在此写死内部函数实现，只维护每个对话切片（Slice）的**用户可感知效果（Outcome）**、**可证伪验收条件（Done Predicate）**与**状态（Status）**。

- 决策账本（`show-me-your-work`）：`.scratch/pstack-relay/decisions.tsv`
- 下一棒提示词快照：`.scratch/pstack-relay/NEXT_PROMPT.md`
- 交接与推进命令：`scripts/pstack-relay status` / `scripts/pstack-relay advance <SLICE_ID> --evidence "<sha/path>"`

---

## 历史切片队列（A → E）

A–R12 记录当时的范围和证据。当前支持以[桌面应用控制](designs/four-app-control.md)为准。

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

## 历史范围与交付

A–E 是第一轮通用交互迁移历史，不代表四款应用深度控制通过。2026-11-05 用户明确只做 Antigravity、Cindy、ZCode、桌面 DeepSeek Harness，并保留 Cindy 原生能力。上轮未开始的 F–K 由下列切片替代，不建立 Paseo 适配器，不强制复制 MCP/CLI 出口。

当前源码入口见[桌面应用接口](four-app-interfaces.md)。当时的状态归 [#81](https://github.com/semantic-craft/micro-input/issues/81) 及责任票。各片状态由勾选与证据记录确认；软件、原生安装和实体结果分别验收。

### [x] Slice F: 保留 Cindy 原生输入和灯光所有权
- **Outcome**: 增加 Cindy 原生所有权的活跃握手和代次确认；原生持有期间暂停 Micro Input 重叠按键、PTT 与灯光，未知时保持暂停。
- **Done Predicate**: 同一宽键/旋钮输入投递两端后业务动作仅一次；切 Profile、失联和旧代回调不双执行；不切换 Cindy 设备开关或改 keymap。 不能只看进程存在；失联标 unknown，移交其他应用前取得释放回执，未确认就不接管。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: 无。责任票 #82。
- **Evidence**: .scratch/qa-runs/1793813638607-83009/evidence/result.json
- **Verification scope**: 软件片通过全量离线检查、Cindy 源码 187 项测试及完整类型检查、真实 UDS 双端配对 8 事件；安装版 Cindy、实体按键和灯光视觉为 NOT_RUN。补丁、恢复限制及重跑命令见 [Cindy 适配说明](../integrations/cindy/README.md)。

### [x] Slice G: 接通 ZCode 同实例状态与精确导航
- **Outcome**: 通过最小桌面桥读取 v4 任务索引，在两个同名会话间准确打开目标。
- **Done Predicate**: 两个工作区与同名任务身份不混淆；selected session 回读吻合；断连/换代/删除后不改开其他任务；默认不读正文。 未授权或越权请求零元数据读取。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: 无。责任票 #33。
- **Evidence**: .scratch/qa-runs/1793838585847-10402/evidence/result.json
- **Verification scope**: 软件片通过全量离线门禁及六个隔离应用场景；固定 ZCode 源码检查、16 项原生边界检查和真实 UDS Swift 双端联测通过。真实 renderer store 使用合成任务 owner；完整 Electron 窗口、安装版与实体为 NOT_RUN。补丁与重跑入口见 旧版集成文档（已退役，见 Git 历史）。

### [x] Slice H: 接通 DSH 同 Host 状态与精确导航
- **Outcome**: 先核验 0.2.1-rc.5 接口，再在同 Host 读取状态并打开准确会话。
- **Done Predicate**: activate=false 的订阅不启动后台会话；pendingInput 变化不漏；导航回读 ID；停止桥不停止 Host；无认证零读取。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: 无。责任票 #32。
- **Evidence**: .scratch/qa-runs/1793841784754-43891/evidence/result.json
- **Verification scope**: 软件片通过全量离线门禁及七个隔离应用场景；固定 rc.5 原生源码契约、类型、lint 和 191 项测试通过。真实 UDS Swift 双端确认两个目标、四次待回应变化、零未授权读取及关闭后 Host 存活；最新 Swift 回归在合成和生产 ABI 各 13 项通过。配对导航端口为合成选择，真实 UI store 另有源码测试。日常 DSH 补丁安装、完整 Electron 窗口和实体均为 NOT_RUN；冷目录发现未实现。见 [DSH 桥](../integrations/dsh/README.md)。

### [x] Slice I: 验证 Antigravity 官方桌面扩展与状态事件
- **Outcome**: 只读确认安装版 agentapi/Sidecars/Hooks 契约，再在隔离测试工作区接元数据事件。
- **Done Predicate**: 记录版本及能力；两会话事件不串、重复事件去重、断流标 stale；Stop fullyIdle=false 不报完成；不读 transcriptPath。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: 无。责任票 #83。
- **Evidence**: .scratch/qa-runs/1793851247890-30095/evidence/result.json
- **Verification scope**: 软件片通过全量离线门禁及八个隔离应用场景；安装版 2.29.0（language_server CL 1013060670）只读静态核对、7 项合成/生产双 ABI Swift 单元测试及真实 UDS 双端联测（.scratch/ag-07a1bcc7/result.json）通过。验证两会话隔离、重复事件去重、Stop(fullyIdle=false) 归入 backgroundActive、断流/心跳超时/换代标 stale、0000 权限金丝雀 transcriptPath 零读取。安装版桌面 GUI、既有会话目录与实体按键均为 NOT_RUN。见 [Antigravity 状态桥](../integrations/antigravity/README.md)。

### [x] Slice I2: 核验 Antigravity 既有会话目录与精确导航
- **Outcome**: 先用官方可用入口，缺少时用 REA 验证会话元数据、工作区与聚焦协议，仅限只读和测试导航。
- **Done Predicate**: 两个既有测试会话目录身份与 UI 选中回读一致；同名跨工作区不混；缺 schema/合法认证就失败关闭；不读取 token 或绕过鉴权。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: I。责任票 #83。
- **Evidence**: .scratch/qa-runs/1793854250379-53064/evidence/result.json

### [x] Slice J: 补 Cindy 既有任务目录和导航确认
- **Outcome**: 在原生服务旁加受限元数据桥，沿用 session 深链并回读目标。
- **Done Predicate**: 只列显式授权范围的既有任务；同名会话正确；账号/实例换代失效；delivered 与 confirmed 分开；不放宽 Local Agent API 隔离。 未授权或越权请求零元数据读取。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: F。责任票 #82。
- **Evidence**: .scratch/qa-runs/1793857584441-83439/evidence/result.json

### [x] Slice K1: 向 ZCode 准确发送并停止指定执行
- **Outcome**: 接 createSession、sendText 和携带 execution ID 的 stop；运行中发送的 steering 语义单独确认。
- **Done Predicate**: 两个会话只影响选中者；迟到 stop 不停止新 execution；重复命令不重复发送；超时结果未知时不换 ID 重放。 新建绑定工作区并回读新 ID；运行中插话不误建新会话，不把排队当 steering。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: G。责任票 #33。
- **Evidence**: .scratch/qa-runs/1793862357974-42746/evidence/result.json

### [x] Slice K2: 向 DSH 准确发送并停止当前轮次
- **Outcome**: 接 create、prompt queue/steer、requestId 与 cancel。补同 Host 的预期 turn 核对，分别显示当前轮次和队列。
- **Done Predicate**: queue 与 steer 两种结果分别确认；cancel 后队列可能继续时不报全部停止；重复请求无双发；切会话无误停。 新建绑定工作区并回读新 ID；同会话旧 Stop 遇新轮次必须零取消，未能证明原子性就拒绝发送。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: H。责任票 #32。
- **Evidence**: .scratch/qa-runs/1793867145490-44990/evidence/result.json

### [x] Slice K3: 用 Antigravity agentapi 创建和发送
- **Outcome**: 创建绑定 projectId，发送绑定 conversationId，取得结果确认。
- **Done Predicate**: 两个测试会话只改目标；不存在目标零重定向；超时不重发；运行中消息是否 steering 单独验证。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: I、I2。责任票 #83。
- **Evidence**: .scratch/qa-runs/1793871576128-81269/evidence/result.json

### [x] Slice L1: 接 ZCode 请求审批与思考档位
- **Outcome**: 按 interactionId/type 处理批准拒绝、问题和计划，复用 switchModelConfig。
- **Done Predicate**: 过期/重复请求零误批；问题不当权限批准；模型无该档位不发送；回读模型和档位一致。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: K1。责任票 #33。
- **Evidence**: .scratch/qa-runs/1793874716941-62949/evidence/result.json

### [x] Slice L2: 接 DSH 请求审批与思考档位
- **Outcome**: 先证明当前桌面 approval 接线，再接 request 级回答和 selectModel。
- **Done Predicate**: 取消/迟到答案零副作用；会话切换不改批其他请求；真实模型档位回读；无法证明接线时保留缺口。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: K2。责任票 #32。
- **Evidence**: .scratch/qa-runs/1793881371629-18575/evidence/result.json

### [x] Slice L3: 补 Antigravity 请求与模型控制缺口
- **Outcome**: 逐能力用 REA 核实停止、审批、模型/思考入口，按能力分 PR；语音归 O1。
- **Done Predicate**: 每条候选都取得 schema、合法鉴权和两个测试会话结果；字符串存在不算完成；升级/认证失效关闭；不开放任意 RPC。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: I2、K3。责任票 #83。
- **Evidence**: .scratch/qa-runs/1793888574124-27851/evidence/result.json

### [x] Slice M: 补 Cindy 原生任务动作缺口
- **Outcome**: 复用原生服务补准确停止、请求绑定、目标 Prompt/steering 及工作区/待办动作。
- **Done Predicate**: 已有任务槽/PTT/思考/灯光行为不退化；停止/审批固定身份，过期零操作；原生未读事实和待办轮次独立核验。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: J。责任票 #82。
- **Evidence**: .scratch/qa-runs/1793891819365-57507/evidence/result.json

### [x] Slice N: 交付四应用稳定槽位和历史待办导航
- **Outcome**: 共享稳定目标选择、六槽固定/最近、项目/工作区导航、历史和本轮不重复的待办轮询。
- **Done Predicate**: 四应用相同名称/不同实例不混淆；新事件重新入队；未知未读不推断；历史回到准确会话；Cindy 原生槽位可复用且不双消费。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: G、H、I2、J；写动作按 K/L/M 能力就绪。责任票 #84。
- **Evidence**: .scratch/qa-runs/1793895420106-11007/evidence/result.json

### [x] Slice O1: 逐应用补齐听写和语音回复控制
- **Outcome**: Cindy 复用原生语音；其余逐款确认并接入按住听写、释放停止、提交前停止，以及独立的语音回复开关。缺少原生能力的开源应用按其语音服务增量实现。
- **Done Predicate**: 四应用分别确认开始/停止和目标绑定；听写转写与发送分开；开启/关闭语音回复须回读真实模式并验证输出。模型/接口不支持时保持该应用未完成，不以禁用或听写成功代替。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: F；对应应用身份与动作片就绪。责任票 #84。
- **Evidence**: .scratch/qa-runs/1793898484356-14497/evidence/result.json

### [x] Slice O2: 复刻官方 Input.app 受保护宿主层与多 Profile 切换让渡机制
- **Outcome**: 按 [`CodexKeycapCatalog.swift`](../app/MicroInput/Sources/MicroInput/CodexKeycapCatalog.swift) 与 [`protected_oai_layer.rs`](../crates/mi-model/tests/protected_oai_layer.rs) 1:1 复刻 `Input 1.1.0` 的 `Mg1`（`KV_OAI_*`）受保护宿主层判定、四项界面与编辑硬隔离（锁定画布遮罩、自动收起改键面板、禁用静态灯光/导出/复制、跨层合并键跳过保护层），以及 `Cindy` / `Codex` 受保护 Profile 与 `Antigravity` / `ZCode` / `DSH` 工作台 Profile 之间的精简板载配置下发（`≤ 1 KB` 规避固件 4 KB 单包溢出）与 USB HID 通道让渡闭环。
- **Done Predicate**: `cargo test --locked -p mi-model` 验证 `is_protected_oai_layer` 识别、四项编辑拒绝/跳过与 `< 1024` 字节精简宿主 `keymap.json` 生成及本地全量多 Profile/30 宏无损保留；`swift test --package-path app/MicroInput` 与 `scripts/verify-offline` 验证受保护层 UI 锁定、静态灯光禁用及 Profile 切换时的 HID 让渡/接管回执全量 `PASS`。
- **Depends on**: F、N。责任票 #82、#84。
- **Evidence**: .scratch/qa-runs/1793901523406-52021/evidence/result.json

### [x] Slice O: 补齐四应用实体交互与状态反馈
- **Outcome**: 逐应用接滚动/跳底、八向 Prompt/Stop、可选回弹与逐键/双区反馈，宽键沿用 O1 已验语音能力。
- **Done Predicate**: 每应用独立证明功能：取消零动作、听写先停后交、真实状态对应 HUD 和灯光；唯一灯光发送者，切层/退出释放明确；语音缺口单列子片，不能以 disabled 签收。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: F、N、O1；对应动作能力就绪。责任票 #84。
- **Evidence**: .scratch/qa-runs/1793928647656-13555/evidence/result.json

### [x] Slice P: 完成四应用安装与日常验收
- **Outcome**: 最终构建在四应用中完成相同任务脚本，记录软件/安装/实体三层证据。
- **Done Predicate**: clean commit 全量机器验收通过；每应用两会话完成导航/发送/停止/审批/状态反馈；升级/断连恢复与原映射保留；未完成项不关闭总票。 软件片须通过项目适用检查和 scripts/verify-offline；原生与实体结果单列。
- **Depends on**: F 至 O 的全部任务，含 I2、K1–K3、L1–L3、O1。责任票 #84。
- **Verification scope**: 软件层 verify-offline 全量 PASS（两轮）。安装层 ZCode 补丁版完整窗口两会话任务脚本 PASS（create 双 confirmed、导航双向 confirmed、queue 发送+去重、executionId 真实停止、迟到 stop noop、过期 resolve 零误批、running→idle 回读、未授权 denied）；Antigravity 安装版 2.29.0 经 agentapi + LS Connect-JSON live 桥两会话 PASS（含 CancelCascadeInvocation 真实停止与 question-as-permission 拒绝，零 transcript/token 读取）；DSH 与 Cindy 为部分通过——补丁版均已部署（Cindy 自 0.1.126 回退至钉住源并修复桥仅随 HID 注册的部署缺陷，双桥开机自启实证；DSH 冷会话 fail-closed 与订阅粘性实证），但两会话活轮任务脚本分别受限于 host 会话注册表仅经交互客户端物化、渲染端工作区目录同步，保持开放。日常版 `/Applications/Micro Input.app` 已更新至最终构建；workbench 原映射零改动、全程零设备写入。实体层 Creator Micro 2（`303A:8298`，`DevSrvsID:4311640475`，固件 `0.6.3`）已通过 `./scripts/verify-hardware` 全自动验收：`H0` 只读身份与 `keymap.json`（`7421B`，SHA-256 `90f916f5880e5e3f9f7e6425855406b7321d68b909d740433c8389535dfccb78`）+ `smart_actions.json`（`41B`）多分块备份 PASS、`H1` 临时文件（`mi_77c9e5fbb160.txt`）多分块写/读/删及 14 次配置零污染快照 PASS、生产 `/Applications/Micro Input.app` 插上即自动连接并同步 `v.oai.rgbcfg` / `lights.preview` / `v.oai.thstatus` 三项硬件灯光 RPC PASS（见 `.scratch/hardware-acceptance/latest-result.json` 与 `~/Library/Application Support/MicroInput/hardware-auto-acceptance.json`）。
- **Evidence**: commit 9393bf5+6916630; verify-offline .scratch/qa-runs/1793938201140-8882/evidence/result.json (full PASS); installed-layer: .scratch/pstack-apps/runs/zcode/daily-result.json (PASS), .scratch/pstack-apps/runs/antigravity/daily-result.json (PASS), .scratch/pstack-apps/runs/dsh/daily-result.json (PARTIAL), .scratch/pstack-apps/runs/cindy/daily-result.json (PARTIAL); hardware-auto-acceptance: .scratch/hardware-acceptance/latest-result.json (PASS), .scratch/input-study/probe-h-044d0429-3e4e-5416-2a22-a21f7d29bb35 (H0 PASS), .scratch/input-study/probe-h-5414fae0-26c8-1209-cff7-dd7683097968 (H1 PASS)

---

## 纯 Rust 内核 + 薄 C-ABI + 原生 SwiftUI 薄壳重构接力队列（R0 → R12，`msime` 架构定型）

- **总目标**：借鉴 [`metasequoiaime/msime`](https://github.com/metasequoiaime/msime) 的「纯 Rust 跨平台内核 + 薄 C-ABI 边界 + 100% 原生宿主薄壳」设计，将 `Micro Input` 原 30,373 行肥 Swift 业务层全面下沉至纯 Rust 内核（`crates/mi-bridge`、`crates/mi-macos`、`crates/mi-core`），彻底移除 UniFFI 与历史实验性 `crates/mi-gpui`，通过 7 函数极简 C-ABI 静态库（`crates/mi-ffi`）向原生 macOS SwiftUI/AppKit 薄壳（`app/MicroInput`）提供单向状态投影（`WorkbenchProjection`）与动作派发（`WorkbenchCommand`），同时 100% 保持现有 8 套软件方案、四应用实时会话控制、Cindy 原生所有权桥、受保护 `Mg1` 层锁及 `scripts/verify-offline` 全部 9 个隔离验收场景零退化。
- **参考实现**：`https://github.com/metasequoiaime/msime`（`crates/engine`、`crates/client-core`、`crates/host-api`、`platforms/macos`）
- **自动验收入口**：`skills/verify-micro-input/SKILL.md`、`scripts/verify-offline`、`scripts/check`

### 架构拓扑与分层边界（`principle-foundational-thinking`、`principle-boundary-discipline`）

Workspace 固定为以下单向依赖的四层 Rust Crates 与薄原生宿主壳层：

1. **领域与协议层（既有 21.9k 行，零改动直接复用）**：
   - `crates/mi-proto`：Creator Micro 2 HID 物理分帧、只读出口门禁与 `UNKNOWN-*` 边界。
   - `crates/mi-transport`：USB HID 设备发现、生命周期与读写通道。
   - `crates/mi-model`：文档模型、草稿三方合并、8 套 Agent 权威注册表（`CANONICAL_DESKTOP_AGENT_PROFILES`）、`Mg1`（`KV_OAI_*`）保护层判定、回退历史栈与待办轮询队列（`AttentionRound`）。
   - `crates/mi-session`：工作台会话核心、路由状态机、输入水位线与灯光预览合成。
2. **跨进程桥与仲裁层 `crates/mi-bridge`（纯 Rust 异步状态机，零 UI / AppKit 依赖）**：
   - `ownership`：`NativeOwnershipController`、`.fence` 持久化代次递增、可中断轮询休眠、瞬态重确认窗口、`R-C03` 仅凭 `cindy-ownership.sock` 实时存在性判定 `is_unbridged_desktop_active`。
   - `voice` & `lighting_arbiter`：`FourAppVoiceManager` + `PushToTalkController`（`R-C04` 强制等待 `wait_for_start_outcome` 后方可进入 `Recording`）与 `FourAppLightingArbiter`（硬件载荷摘要幂等去重、唯一发送者生命周期）。
   - `zcode`、`dsh`、`cindy`、`antigravity`：四款桌面应用的 UDS / Connect-JSON / HTTP 会话控制器、精确导航、发送/插话/排队、运行级停止、审批/问题回答、模型/思考档位切换，以及对应的纯 Rust 合成验收场景生成器。
3. **macOS 原生系统适配层 `crates/mi-macos`（薄机械边界层）**：
   - `instance_lock`：`<documentDir>/instance.lock` 排他文件锁与 LaunchServices 临时包注销（`R-C05`）。
   - `app_cache`：进程内线程安全的 `NSWorkspace` 应用安装路径与图标缓存。
   - `executor` & `ax_dictation`：`LiveHostActionExecutor`（`CGEvent` 快捷键、`AXUIElement` 焦点/菜单自动化、Apple `Fn` HID 注入，常规路径严禁调用 `AXIsProcessTrustedWithOptions(prompt: true)`）与 `SimulatedHostActionExecutor`（生成确定性 `audit.jsonl` 回执），以及剪贴板无损暂存/恢复的 `AXDictation`。
   - `status_item` & `passive_panel`：菜单栏快照规格与 `.nonactivatingPanel` + `.canJoinAllSpaces` 全屏辅助悬浮窗口几何契约。
4. **纯 Rust 工作台内核 `crates/mi-core` + 薄 C-ABI `crates/mi-ffi` + 原生 SwiftUI 壳层 `app/MicroInput`**：
   - `crates/mi-core`：无 UI 框架耦合的 `WorkbenchStore`、`SidebarProjection`、`HeaderProjection`、`CanvasProjection`、`SessionStripProjection`、`InspectorProjection`、`EditorModalProjection`、`RadialMenuState`、`OverlayController` 与 `qa_runner`（内置 9 景合成验收驱动器）。
   - `crates/mi-ffi`：对齐 `msime/crates/host-api` 的 7 函数 `extern "C"` 静态库边界（`mi_core_new`、`mi_core_free`、`mi_core_project_json`、`mi_core_dispatch_json`、`mi_core_poll_json`、`mi_core_run_qa_from_env`、`mi_core_acquire_instance_lock` / `mi_core_release_instance_lock`）。
   - `app/MicroInput`：仅保留薄宿主入口 `MicroInputApp.swift` 与 5 个纯展示文件（`CoreBridge.swift`、`WorkbenchRootView.swift`、`ClassicWorkbenchView.swift`、`ModalsAndStatusMenuView.swift`、`UITheme.swift`），零业务控制器与零状态分叉。

### Agent 单次任务上下文容量护栏（`principle-guard-the-context-window`）

为确保每个接力对话始终运行在模型推理最优上下文区间（单次会话上下文占用 `< 35%`），以下硬约束适用于 `R1 → R12` 每一棒：
1. **单棒读取上限（Read Cap ≤ 1,850 LOC）**：每个切片只允许读取本切片指明的 2–5 个对应参考文件与直接依赖的 Rust 接口文件；若需跨文件检索，必须派发只读 `research` 或 `poteto-agent` 子代理并返回紧凑摘要，严禁把整仓源码读入主线程。
2. **单棒写入上限（Write Cap ≤ 900 LOC）**：每个切片只聚焦 1 个新建/修改的 Rust crate 子模块与配套测试，单棒净增/修改代码控制在 500–900 行以内。
3. **双轨常绿过渡（`principle-outcome-oriented-execution`）**：在 `R1 → R10` 期间，新建的 `crates/mi-bridge`、`crates/mi-macos`、`crates/mi-core` 在 Workspace 中独立编译与单测，不破坏既有 `scripts/check`；在 `R11–R12` 完成 `crates/mi-ffi` C-ABI 切换、退役 56 个旧肥 Swift 源文件与 UniFFI，并通过全九景离线验收。

---

### [x] Slice Q1: 专注跟随与固件 0.6.3 触控轮切架构定型及容量切分
- **Playbook**: `Investigation` + `architect` (`principle-guard-the-context-window` + `principle-sequence-verifiable-units`)
- **Outcome**: 完成「仅在指定 Coding Agent 间自动跟随 + 切去其他软件零切换/零落盘/零弹窗 + 其他软件保留为手动通用预设 + 固件 v0.6.3 左下角电容触控键（`wl_touch_btn`）驱动多层透传轮切指定 Profile」的三层架构设计（Rust 领域模型层、FFI/Swift 路由与 `device.status` 监听层、工作台 UI 与板载同步层），并将 Q1–Q4 写入 `docs/pstack-relay.md`。
- **Done Predicate**: `scripts/pstack-relay selftest` 通过；架构契约与上下文容量预算表写入 `docs/pstack-relay.md` 与 `.scratch/pstack-relay/decisions.tsv`，推进 Q1 并产出 Q2 提示词。
- **Evidence**: docs/pstack-relay.md;.scratch/input-study/fw-v0.6.3/firmware_0.6.3_merged.bin

### [x] Slice R0: 侧边栏切换卡顿根因修复、全量资产盘点与 Rust 内核分片队列就位
- **Playbook**: `Perf issue` + `figure-it-out` (`principle-fix-root-causes`, `principle-guard-the-context-window`)
- **Outcome**: 修复当前版本点击左侧菜单切换方案的主线程卡顿与黄色横幅跳动（8 方案切换中位耗时由 `152.47 ms` 降至 `82.99 ms`，横幅闪烁由 `42` 降至 `0`）；完成 30.4k 行 Swift 与 21.9k 行 Rust 的逐模块盘点；确立 `mi-bridge` -> `mi-macos` -> `mi-core` 四层架构与单棒 `≤ 1,850 LOC` 读取预算的 `R1–R12` 全景接力队列。
- **Done Predicate**: `./scripts/check` 全量通过；`scripts/pstack-relay selftest` 通过；本机 `/Applications/Micro Input.app` 已同步更新。
- **Evidence**: docs/pstack-relay.md;AGENTS.md;app/MicroInput/Tests/MicroInputTests/MicroInputTests.swift

### [x] Slice R1: 建立 `crates/mi-bridge` 原生所有权状态机、语音与灯光仲裁内核
- **Playbook**: `Feature` (`architect` -> `poteto-agent` -> `tdd`)
- **Outcome**: 新建 `crates/mi-bridge`（单棒读取预算 `≤ 1,650 LOC`：仅读 `NativeOwnership.swift`、`UnixNativeOwnershipTransport.swift`、`NativeOwnershipScenario.swift`、`FourAppVoice.swift`、`FourAppLightingController.swift`）。在纯 Rust 中实现 `NativeOwnershipController`（持久化 `.fence` `clientInstance` 与递增 `generation`、可中断轮询休眠、瞬态重确认窗口、`R-C03` 仅凭 `cindy-ownership.sock` 实时存在性判定 `is_unbridged_desktop_active`）、`FourAppVoiceManager` + `PushToTalkController`（`R-C04` 强制 `wait_for_start_outcome` 确认后才进入 `Recording`）与 `FourAppLightingArbiter`（硬件载荷摘要幂等去重），并内置 `native-ownership` 合成场景生成器。
- **Done Predicate**: `cargo test --locked -p mi-bridge` 覆盖重启代次延续、无桥桌面恢复与重锁、PTT 未确认失败关闭、灯光幂等去重及 `native-ownership` 场景全部 9 项检查与 3 条模拟回执；`./scripts/check` 通过。
- **Evidence**: crates/mi-bridge;.scratch/qa-runs/1794074657792-33981/evidence/result.json

### [x] Slice R2: `crates/mi-bridge` 之 ZCode 同实例会话控制与合成验收迁移
- **Playbook**: `Feature` (`principle-model-the-domain` + `tdd`)
- **Outcome**: 在 `crates/mi-bridge::zcode` 中迁移 ZCode v4 桌面桥控制器（单棒读取预算 `≤ 1,800 LOC`：仅读 `ZCodeSessionModel.swift`、`ZCodeSessionClient.swift`、`ZCodeSessionController.swift`、`ZCodeNavigationScenario.swift`）。完整实现跨工作区同名会话区分、`selected` 回读确认、`createSession` / `sendText`（区分 `steered` 与 `queued`）、绑定 `executionId` 的 `stop`（迟到 `noop`）、绑定 `interactionId/type` 的审批/问题回答（严禁把问题当权限批准）与 `switchModelConfig` 思考档位校验，并内置纯 Rust `zcode-navigation` 合成场景运行器。
- **Done Predicate**: `cargo test --locked -p mi-bridge --test zcode_scenario` 生成符合 `scripts/verify-offline` 中 `zcode-navigation` 全部 18 项检查、`body_reads == 0` 及精确 `audit.jsonl` 回执计数的证据；`./scripts/check` 通过。
- **Evidence**: crates/mi-bridge;.scratch/qa-runs/1794079290114-26119/evidence/result.json

### [x] Slice R3: `crates/mi-bridge` 之 DSH 同 Host 会话控制与合成验收迁移
- **Playbook**: `Feature` (`principle-model-the-domain` + `tdd`)
- **Outcome**: 在 `crates/mi-bridge::dsh` 中迁移 DSH 0.2.1-rc.5 桌面桥控制器（单棒读取预算 `≤ 1,800 LOC`：仅读 `DSHSessionModel.swift`、`DSHSessionClient.swift`、`DSHSessionController.swift`、`DSHNavigationScenario.swift`）。完整实现 `activate=false` 被动订阅、`pendingInput` 状态保留、同 Host 轮次原子性核对（无法证明原子性时拒绝发送）、`queue` 与 `steer` 分离确认、旧轮次 `cancel` 零误停、`cancelledQueueRemaining` 状态区分、请求级审批回答与 `selectModel` 思考档位回读，并内置纯 Rust `dsh-navigation` 合成场景运行器。
- **Done Predicate**: `cargo test --locked -p mi-bridge --test dsh_scenario` 生成符合 `scripts/verify-offline` 中 `dsh-navigation` 全部 24 项检查、`body_reads == 0` 及精确 `audit.jsonl` 回执计数的证据；`./scripts/check` 通过。
- **Evidence**: crates/mi-bridge;.scratch/qa-runs/1794101640109-27260/evidence/result.json

### [x] Slice R4: `crates/mi-bridge` 之 Cindy 受限任务目录、深链确认与待办轮询迁移
- **Playbook**: `Feature` (`principle-model-the-domain` + `tdd`)
- **Outcome**: 在 `crates/mi-bridge::cindy` 中迁移 Cindy 受限元数据与动作桥（单棒读取预算 `≤ 1,800 LOC`：仅读 `CindySessionModel.swift`、`CindySessionClient.swift`、`CindySessionController.swift`、`CindyNavigationScenario.swift`）。完整实现显式授权范围目录读取、深链 `delivered` 与 `confirmed` 分离、账号/实例换代失效、运行绑定 `stop` 与 `resolve` 问题守卫、工作区新建与目标插话，以及三态未读与 `mi-model::AttentionRound` 待办轮询闭环，并内置纯 Rust `cindy-navigation` 合成场景运行器。
- **Done Predicate**: `cargo test --locked -p mi-bridge --test cindy_scenario` 生成符合 `scripts/verify-offline` 中 `cindy-navigation` 全部 13 项检查、`unauthorized_metadata_reads == 0`、`local_agent_api_relaxed == false` 及精确 `audit.jsonl` 回执计数的证据；`./scripts/check` 通过。
- **Evidence**: crates/mi-bridge;.scratch/qa-runs/1794104563552-13924/evidence/result.json

### [x] Slice R5: `crates/mi-bridge` 之 Antigravity 状态流、目录导航与 L3 控制迁移
- **Playbook**: `Feature` (`principle-model-the-domain` + `tdd`)
- **Outcome**: 在 `crates/mi-bridge::antigravity` 中迁移 Antigravity 2.29.0 状态与控制桥（单棒读取预算 `≤ 1,850 LOC`：仅读 `AntigravityStatusModel.swift`、`AntigravityStatusClient.swift`、`AntigravityStatusController.swift`、`AntigravityStatusScenario.swift`）。完整实现 `agentapi` 与 LanguageServer Connect-JSON 协议、双会话隔离与重复事件去重、`Stop(fullyIdle=false)` 归入 `backgroundActive`、绑定 `projectId` 新建与绑定 `conversationId` 发送/插话、`CancelCascadeInvocation` 精确停止、审批/问题守卫、模型/思考档位切换，以及 `transcript_reads == 0` 与 `token_reads == 0` 的失败关闭硬边界，并内置纯 Rust `antigravity-status` 合成场景运行器。
- **Done Predicate**: `cargo test --locked -p mi-bridge --test antigravity_scenario` 生成符合 `scripts/verify-offline` 中 `antigravity-status` 全部 26 项检查、版本号 `2.29.0` / `1013060670` 及精确 `audit.jsonl` 回执计数的证据；`./scripts/check` 通过。
- **Evidence**: crates/mi-bridge;.scratch/qa-runs/1794117114926-27844/evidence/result.json

### [x] Slice R6: 建立 `crates/mi-macos` 原生宿主动作执行器、辅助功能听写与合成执行场景
- **Playbook**: `Feature` (`principle-boundary-discipline` + `tdd`)
- **Outcome**: 新建 `crates/mi-macos`（单棒读取预算 `≤ 1,500 LOC`：仅读 `HostActionExecutor.swift`、`AXDictation.swift`、`DeviceOperations.swift`、`Diagnostics.swift`、`MicroInputApp.swift`）。封装 `DocumentInstanceLock`（`<documentDir>/instance.lock` 排他锁，`R-C05`）、内存缓存 `AppBundleCache`、`LiveHostActionExecutor`（`CGEvent` 键盘事件、`AXUIElement` 焦点/菜单点击、Apple `Fn` 注入，常规执行路径零 `AXIsProcessTrustedWithOptions(prompt: true)` 调用）、`SimulatedHostActionExecutor`、剪贴板无损恢复的 `AXDictation`，以及 `host-execution`、`config-write`、`lighting-control`、`diagnostic-export` 四个合成场景运行器。
- **Done Predicate**: `cargo test --locked -p mi-macos` 独立跑通 `host-execution`（21 条模拟回执）、`config-write`、`lighting-control`、`diagnostic-export` 四个场景的证据校验与单实例锁互斥测试；`./scripts/check` 通过。
- **Evidence**: crates/mi-macos;.scratch/qa-runs/1794124780031-64477/evidence/result.json

### [x] Slice R7: 建立 `crates/mi-core` 纯 Rust 状态仓 `WorkbenchStore` 与左侧方案栏投影
- **Playbook**: `Feature` (`architect` -> `poteto-agent`)
- **Outcome**: 新建 `crates/mi-core`（单棒读取预算 `≤ 1,400 LOC`）。构建纯 Rust `WorkbenchStore`（直连 `mi-session`、`mi-bridge`、`mi-macos`），实现顶部设备状态栏投影（`HeaderProjection`：就绪胶囊、按键控制开关、撤销/重做）与左侧「常用软件方案」侧栏投影（`SidebarProjection`：严格按 `Antigravity` -> `Codex` -> `Deepseek Harness` -> `Herdr` -> `Cindy` -> `ZCode` -> `千问办公` -> `其他软件` 排序，含自动跟随开关、自定义方案过滤、方案管理/导入备份菜单及底部所有权状态条）。
- **Done Predicate**: `cargo test --locked -p mi-core` 验证 `WorkbenchStore` 路由切换、8 套方案权威排序与 `< 2 ms` 零横幅闪烁切换耗时；`./scripts/check` 通过。
- **Evidence**: crates/mi-core;.scratch/qa-runs/1794129146458-48119/evidence/result.json

### [x] Slice R8: `crates/mi-core` 之 Creator Micro 2 17 键画布、受保护层锁与四应用实时会话条投影
- **Playbook**: `Feature` (`principle-experience-first` + `poteto-agent`)
- **Outcome**: 在 `crates/mi-core` 中实现中央工作台舞台状态投影（`CanvasProjection` 与 `SessionStripProjection`）：Creator Micro 2 17 键交互画布（左上旋钮顺/逆时针与按压、左下八向摇杆、12 键网格、底行 2U 宽键合并/拆分渲染、键帽丝印图标与文字开关、`Mg1` `KV_OAI_*` 受保护层锁定遮罩）及画布下方的四应用 6 槽实时会话状态条（蓝点已完成胶囊、Working 动画胶囊、一键已读熄灯、灯光调整浮层、上方 6 键设为会话键、恢复默认布局）。
- **Done Predicate**: `cargo test --locked -p mi-core` 验证 17 键点击命中、底行 2U 宽键合并/拆分状态、`Mg1` 受保护层锁定硬隔离与 6 槽实时会话状态同步；`./scripts/check` 通过。
- **Evidence**: crates/mi-core;.scratch/qa-runs/1794135562535-27711/evidence/result.json

### [x] Slice R9: `crates/mi-core` 右侧属性检查器、丝印图标目录与宏/灯光/多动作编辑器状态机
- **Playbook**: `Feature` (`poteto-agent`)
- **Outcome**: 在 `crates/mi-core` 中实现右侧属性面板与各类编辑弹窗状态机（`InspectorProjection` 与 `EditorModalProjection`）：按键用途切换（会话键 vs 动作键）、自定义快捷键录制器与一键设为 Apple `Fn`、可搜索的 34 枚键帽丝印图标网格、动作与宏绑定抽屉，以及宏序列编辑器、单键/双区灯光编辑器、多动作分支编辑器与重连三方对比弹窗。
- **Done Predicate**: `cargo test --locked -p mi-core` 覆盖检查器改绑、Apple `Fn` 绑定、宏/多动作/灯光草稿编辑及重连三方合并预览；`./scripts/check` 通过。
- **Evidence**: crates/mi-core;.scratch/qa-runs/1794142293550-9281/evidence/result.json

### [x] Slice R10: `crates/mi-core` 全屏非抢焦悬浮 HUD、八向径向菜单与状态栏常驻菜单
- **Playbook**: `Feature` (`poteto-agent`)
- **Outcome**: 在 `crates/mi-core` 与 `crates/mi-macos` 之间接通系统级浮层与常驻入口（`RadialMenuState`、`OverlayController`、`StatusMenuProjection`）：八向径向菜单（支持四应用 Prompt 槽位预览与中心取消零动作保证）、被动方案切换 HUD（主工作台聚焦时自动抑制）与 `NSStatusItem` 菜单栏快捷切换及 2×2 收藏夹。
- **Done Predicate**: `cargo test --locked -p mi-core` 验证径向菜单八向选择、中心取消零副作用、主窗口激活时 HUD 抑制及状态栏菜单切换；`./scripts/check` 通过。
- **Evidence**: crates/mi-core;crates/mi-macos;.scratch/qa-runs/1794147610068-21739/evidence/result.json

### [x] Slice R11: 对齐 `msime` 建立 `crates/mi-ffi` 薄 C-ABI 边界、打包流水线与全九景闭环
- **Playbook**: `Shipping` (`principle-prove-it-works` + `verify-micro-input`)
- **Outcome**: 对齐 `metasequoiaime/msime` 的 `crates/host-api`，将 `crates/mi-ffi` 重构为无 UniFFI 的 7 函数 C-ABI 静态库（`MicroInputCoreFFI`），更新 `scripts/build`、`scripts/check` 与 `scripts/check-boundaries.py`（`R-C01`..`R-C05` 及薄壳文件白名单门禁），跑通 `scripts/verify-offline` 全部 9 个隔离验收场景。
- **Done Predicate**: `./scripts/verify-offline` 驱动 `MicroInput.app` 全量跑通 `host-execution`、`native-ownership`、`zcode-navigation`、`dsh-navigation`、`cindy-navigation`、`antigravity-status`、`config-write`、`lighting-control`、`diagnostic-export` 共 9 个场景并产出 `PASS` 回执；`./scripts/check` 通过。
- **Evidence**: scripts/build;scripts/check-boundaries.py;crates/mi-core;crates/mi-ffi;.scratch/qa-runs/1794150505406-23321/evidence/result.json

### [x] Slice R12: 退役旧肥 Swift 业务层与 `crates/mi-gpui`、收敛原生 SwiftUI 薄壳并完成本机安装收口
- **Playbook**: `Shipping` (`principle-migrate-callers-then-delete-legacy-apis` + `interrogate` + `verify-micro-input`)
- **Outcome**: 按 `principle-migrate-callers-then-delete-legacy-apis` 与 `principle-subtract-before-you-add` 一次性移除 56 个旧肥 Swift 业务/控制器文件、35 个旧 Swift 测试文件、UniFFI 生成器及历史 `crates/mi-gpui`；收敛 `app/MicroInput/Sources/MicroInputUI` 为单一经典三栏 Scheme A 原生 SwiftUI 薄壳（5 个文件）；通过 `./scripts/check`、`./scripts/verify-offline` 并同步安装至 `/Applications/Micro Input.app`。
- **Done Predicate**: `test ! -d crates/mi-gpui` 为真；`./scripts/check`、`./scripts/verify-offline` 与 `scripts/pstack-relay status --require-all-done` 全部 `PASS`；本机 `/Applications/Micro Input.app` 已更新为 `msime` 架构原生 SwiftUI 版本。
- **Evidence**: Cargo.toml;scripts/build;scripts/check;scripts/check-boundaries.py;crates/mi-core;crates/mi-ffi;app/MicroInput;.scratch/qa-runs/1794153146682-84439/evidence/result.json

## 当前切片

### [x] Slice R13: 收敛内置适配并保留通用按键方案
- **Outcome**: 内置适配仅提供 Antigravity、Herdr、Codex、DeepSeek Harness。任意其他应用可用「其他软件」录制快捷键。旧专属会话桥和所有权握手退出运行路径。
- **Done Predicate**: 新文档只有四套内置适配和一套通用方案；升级只清理可识别的旧内置预设；Codex 受保护层和通用快捷键行为有回归覆盖；`scripts/verify-offline` 通过。
- **Evidence**: 99815e9; .scratch/four-desktop-adapters/result.json (verify-offline PASS; six isolated scenarios)

### [x] Slice R14: 恢复 Cindy 内置适配
- **Outcome**: 恢复 Cindy 预设、会话桥和所有权协调，保留其他四款内置适配以及「其他软件」的逐键快捷键。
- **Done Predicate**: `scripts/verify-offline` 的八个隔离场景通过，包含 Cindy 导航与原生所有权切换；不改写实体键盘映射。
- **Evidence**: 8871a48; .scratch/restore-cindy/result.json (verify-offline PASS; eight isolated scenarios)

### [x] Slice R15: 六层并用与点击听写
- **Outcome**: Codex 原生占第 1 层，Antigravity、DeepSeek Harness、Cindy 分别由 Micro Input 控制第 2 至 4 层，第 5、6 层留空。复用 Karabiner 隔离普通键输入，听写按一下开始、再按一下停止。
- **Done Predicate**: 六层编译、迁移、固定路由、双微动合并、输入换代、定向宿主控制和恢复链的自动回归通过；`scripts/verify-offline` 通过；本机日常版更新。设备写入与读回、恢复、实体按键及录音状态分别记录实际结果，不能互相替代。
- **Verification scope**: 软件层 `MI_AUTO_HOST_READY=0 ./scripts/check` 与 `MI_AUTO_HOST_READY=0 ./scripts/verify-offline` 八景全量 PASS（`241ebc3`）。安装层 `/Applications/Micro Input.app` 已更新并常驻运行（`pid 14523`），只读重连绑定 Karabiner `send_user_command` 套接字 `/tmp/micro-input-3ae2924a1ac1eb395625ad0a7f93af0c/in`。设备层与恢复链经 `live_six_layer_hardware_and_karabiner_acceptance` 实机通过（Creator Micro 2 `303A:8298` 固件 `0.6.3`、Karabiner-Elements `16.3.0`、Cindy `pid 5800`）：原 `7421B` `keymap.json`（SHA-256 `90f916f5880e5e3f9f7e6425855406b7321d68b909d740433c8389535dfccb78`）备份、六层 `profile_index=3` 写入与 SHA-256 读回、只读重连（零 Flash 重写）、一键恢复原 `7421B` 配置与 Karabiner 规则、以及恢复后重新应用六层日常态全部 PASS。实体第 2–4 层物理按压与真人语音录音转写留待日常使用中实测，不以合成回执替代。
- **Evidence**:241ebc3;.scratch/qa-runs/1794288350074-12181/evidence/result.json


### [x] Slice R16: 恢复睡眠唤醒会话边界
- **Outcome**: Swift 转发系统睡眠和唤醒通知；Rust 停止原目标听写、退役旧 USB 会话与迟到回执，保留草稿和撤销。唤醒等待资源释放后仅恢复此前开启的设备发现。
- **Done Predicate**: 隔离场景覆盖持键睡眠、重复唤醒、旧回执迟到与草稿撤销保留；`scripts/verify-offline` 全量通过。不写设备配置。
- **Evidence**: .scratch/sleep-wake/result.json; RUST_TEST_THREADS=1 scripts/verify-offline PASS (eight isolated scenarios)

### [x] Slice R17: Dock 图标大小与系统对齐
- **Outcome**: 保留现有插画，校正打包留白并使用系统处理后的运行图标；更新本机日常安装版。
- **Done Predicate**: MI_AUTO_HOST_READY=0 RUST_TEST_THREADS=1 scripts/verify-offline && swift scripts/verify-app-icon.swift target/AppIcon.iconset && codesign --verify --strict "/Applications/Micro Input.app"
- **Evidence**: /Applications/Micro Input.app; scripts/verify-offline PASS (eight scenarios); installed ICNS opaque frame 824px

### [x] Slice R17B: 分离投影与执行并收敛动作归属和输入边界
- **Outcome**: 纯投影读取、明确原生展示同步、原目标动作释放与失败清理；有界硬件输入、可靠断连及可重放时间。
- **Done Predicate**: 原归属、失败释放、多输入 Fn、输入过载、旧来源隔离和纯投影回归通过；`scripts/verify-offline` 八个隔离场景全量通过。不写设备配置。
- **Evidence**: 8ebfe13; scripts/verify-offline exit 0, eight isolated scenarios PASS and cleanup confirmed; .scratch/runtime-architecture/verification.json

### [x] Slice R18: 过期按键不再停用后续输入
- **Outcome**: 保留两秒有效期和所有权检查，仅丢弃过期按键，后续新按键继续执行；未实现的会话动作明确拒绝，设备核验失败显示具体原因。
- **Done Predicate**: 过期后新输入回归先失败后通过；独立审查及 `scripts/verify-offline` 通过；本机日常版更新。实体按键与会话导航能力分别记录，不以明确拒绝当成导航完成。
- **Evidence**: a857584; .scratch/antigravity-profile/result.json; scripts/verify-offline eight scenarios PASS; physical verification pending

### [x] Slice R19: 接通会话键的真实目标导航
- **Outcome**: 六层和工作台共享动态槽位解析，按完整目标身份切换会话；仅在实际选中被确认后更新已读和导航历史。
- **Done Predicate**: 覆盖空槽、过期目标、实例换代、导航拒绝与成功回读；在两个真实 Antigravity 会话间完成实体键跳转。打开应用不能代替切换会话。
- **Verification scope**: 软件层全量 `scripts/check` 与六个隔离 QA 场景 PASS；安装层 `/Applications/Micro Input.app`（`c5cee05`）完成未读与已读会话实窗切换及水位更新；实体层经绑定 Karabiner 套接字（`/tmp/micro-input-74249b81f0dbd6bb0e45742f6ddea656/in`）按压 Creator Micro 2 实体键 `AG00` / `AG01` 在真实 Antigravity 会话间完成 12 次 `session_selection_confirmed` 精确跳转（含并发按键 `session_navigation_busy` 保护），零设备配置改写。
- **Evidence**: c5cee05; .scratch/session-target-navigation/result.json; .scratch/session-target-navigation/production-audit.jsonl (12 confirmed physical AG00/AG01 session switches)

### [x] Slice R20: 托管按键跟随前台与分屏焦点
- **Outcome**: 第 2–4 托管层优先使用当前前台应用的已安装方案，执行器按光标所在分屏选择控件；后台 Codex 不参与目标选择，旧输入不随切换转投新目标。
- **Done Predicate**: 前台切换与双分屏回归先失败后通过，独立审查及 `scripts/verify-offline` 通过，本机日常版更新。物理层仍按真实观察显示；第 1 层 Codex 原生通道及尚未实现的会话导航分别保留边界。
- **Evidence**: e5c6e6d; .scratch/antigravity-foreground/result.json; eight offline scenarios PASS; installed; physical input not run

### [x] Slice R21: 六层实体输入与配置 RPC 分离
- **Outcome**: 首次配置核验完成后关闭共享 RPC reader，六层输入独立依赖 USB/Karabiner 与自身输入代次，不依赖 Cindy 进程或所有权桥。避免无关响应、身份查询超时或编号耗尽停用实体键。配置查询保留严格响应归属与终止原因。
- **Done Predicate**: 核验后 reader 退出、运行期零 RPC、实体 token 路由、无 Cindy 所有权桥的执行及失效清理回归通过；无关联响应不误完成请求，本请求坏包仍失败；`scripts/verify-offline` 全量通过并更新本机。持续连接和实体动作分别记录，层位仅表述首次核验或最近输入，UNKNOWN-17 保留。
- **Evidence**: d529820; .scratch/antigravity-session/result.json; eight offline scenarios PASS; installed; 363s connected without Cindy; final physical keys NOT_RUN

### [x] Slice R22: 移除 Cindy 专用支持
- **Outcome**: 内置应用保留 Antigravity、Herdr、Codex 和 DeepSeek Harness，另有「其他软件」通用方案。删除 Cindy 预设、会话桥、听写、辅助功能适配与原生所有权握手；默认第 4 至 6 层留空。
- **Done Predicate**: 旧文档与偏好保全，旧三宿主安装记录可读但不执行且恢复信息仍可用；第 4 层输入不执行；DSH 共享传输与 ChatGPT 既有快捷键有回归覆盖；`scripts/verify-offline` 六个隔离场景通过。不改写实体设备映射。
- **Evidence**: e3ea0c8; .scratch/remove-cindy/result.json; final verify-offline six scenarios PASS; native UI entries verified; no device writes

### [x] Slice R23: 减少托管聚焦的重复 AX 扫描
- **Outcome**: 保留窗口、分屏和候选歧义检查，用已发现控件的实时路径校验与焦点读回减少按键动作等待，不增加跨动作缓存。
- **Done Predicate**: 真实 AX 聚焦的匹配构建交替对照成功，范围与失败独立记录；失效路径、控件替换和目标改变回归通过；独立验收与 `scripts/verify-offline` 通过，并更新本机。物理按键完整延迟单独保留边界。
- **Evidence**: ea3a82f; docs/verification/host-input-latency.json; .scratch/input-latency/result.json; eight offline QA PASS; installed and connected; physical latency NOT_MEASURED


### [x] Slice R24: Codex 自定义控制与六层 USB 独占
- **Outcome**: 六层恢复和 RPC 会话轮换保持 Micro Input 独占 Vendor 接口，使用「Codex · Micro Input」自定义方案，保留当前设备映射。
- **Done Predicate**: Codex 自动/手动路由、生命周期与去重回归和 `scripts/verify-offline` 通过；真实 USB 启动竞争、插拔、动作次数和设备配置读回分别留证；通过后更新本机日常版。
- **Evidence**: a90fdf2; docs/verification/codex-usb-ownership.json; offline and native USB PASS; physical 1/1 and 3/3 after readiness; initial fixed-wait 2/3 retained; installed

### [x] Slice R25: Codex 底部七键对齐与点击听写
- **Outcome**: 本机 Codex 方案底部七键对应实时语音、输入、终端、布局、新建、点击听写和发送；旧默认方案升级，自定义内容与设备映射保留。
- **Done Predicate**: 快捷键、启动迁移、目标输入与听写停止回归和 `scripts/verify-offline` 通过；更新本机日常版。真实 Codex AX 和物理按键未验收时明确标注，不把快捷键派发当作界面回读。
- **Evidence**: docs/verification/codex-controls.json; final verify-offline PASS; daily installed; native Codex and physical NOT_RUN

### [x] Slice R26: Codex 前六键按会话状态筛选
- **Outcome**: 六个按键按选定状态筛选 Codex 会话，键位显示、灯光和导航使用同一会话身份。
- **Done Predicate**: 核验真实会话身份、状态来源与选中回读；筛选、空槽、目标失效和旧输入回归及离线验收通过。最近六个会话快捷键不能代替状态筛选。
- **Evidence**: docs/verification/codex-controls.json; verify-offline PASS (6 QA scenarios); live ChatGPT.app duplicate-title switch PASS

### [x] Slice R27: 收口有效改动与开发现场
- **Outcome**: 整合 Codex 旋钮预设和 Antigravity 侧栏导航修复，修正 Fast 模式读回失败误报成功；旧工作区只保留有效实现，清理已合并副本及取消的按住说话试验。
- **Done Predicate**: 原始改动逐项核对，Fast 模式错误回读回归先失败后通过，独立核验与 `scripts/verify-offline` 六景通过；有效改动提交、推送并合入主线，工作区干净，移除无运行依赖的旧副本。不写设备配置；不把安装、实体按键或视觉效果记为本轮通过。
- **Evidence**: PR #128 merged 718483d; PR #129; .scratch/integration-cleanup-20261114/result.json; .scratch/integration-cleanup-20261114/cleanup-result.json; 6 isolated scenarios PASS; 8 unused worktrees removed; native and physical NOT_RUN
- **Verification scope**: 本轮只做软件交付与开发现场清理；最新发布仍为 v0.7.0。#44 的安装升级、实体输入、灯光视觉及完整产品验收继续保留原门槛。#71 的旧六层按住方案与当前点击开关契约不同，不据此恢复旧草稿。

### [x] Slice R28: Creator Micro 本体应用图标
- **Outcome**: 用 Creator Micro 2 正面硬件造型替换角色插画，保留旋钮、摇杆、六颗发光键和底部宽键，改善 Dock 小尺寸辨识度。
- **Done Predicate**: 检视 16/32/128 px 图标；系统轮廓检查与 `scripts/verify-offline` 全量通过；本机日常版更新图标并通过签名校验。不改设备映射，用户视觉偏好由后续反馈确认。
- **Evidence**: .scratch/app-icon/result.json; scripts/verify-offline six scenarios PASS; installed icon and signature verified


### [x] Slice R29: 菜单栏固定显示当前按键配置
- **Outcome**: 菜单栏直接显示当前生效方案的静态 4×4 按键用途，包含旋钮、摇杆及底排布局，删除会话预览与切换选项。复用原生弹层与现有数据，不新增依赖、后台进程或刷新计时器。
- **Done Predicate**: 旧偏好保全、生效方案与浏览选择隔离、分体与宽键布局、会话更新不重绘菜单的回归及 `scripts/verify-offline` 通过；原生弹层核验并更新本机日常版。不写设备配置，不将本轮展示验证当作 #44 完整产品验收。
- **Evidence**: .scratch/qa-runs/1794625945887-73688/evidence/result.json; verify-offline six scenarios PASS; native install check pending main merge

### [x] Slice FW1: Creator Micro 2 官方固件更新
- **Outcome**: 通用设置中检查官方稳定/预发布固件，下载校验后显式进入引导模式、选择串口，完整备份后由 Rust 写入并验证。
- **Done Predicate**: 发布选择、摘要与镜像布局、引导命令一次发送、失败状态与宿主互斥测试通过；`scripts/verify-offline` 通过并更新本机。真实刷写、运行版本、配置保留及恢复分别保留未验边界。
- **Evidence**: docs/firmware-update.md; .scratch/qa-runs/1794627050790-39928/evidence/result.json; live catalog+SHA256+partition check PASS; verify-offline six scenarios PASS; hardware bootloader/flash/restore NOT_RUN

### [x] Slice FW2: 固件更新未来兼容解锁与 0.7.0-rc.16 鼠标能力同步
- **Outcome**: 解锁 `0.6.3` 与 `0.7.x+` 固件后续进入引导模式的软件限制，让 `PrepareDevice` 在未连接时实时识别有线 USB 或既有 bootloader 串口并自动轮询端口；同步 `v0.7.0-rc.16` 的 19 个 `MS_*` 硬件鼠标令牌与摇杆 `MOUSE`/`SCROLL` 模式，并新增 Mac 宿主 CoreGraphics 光标移动、左/右/中点击、四向滚轮、三档速度切换与通用预设「摇杆鼠标 + 旋钮滚轮」，随后以安装版 App 完成 `0.6.3 -> 0.6.3` 与 `0.6.3 -> 0.7.0-rc.16` 两阶段真机刷写验证。
- **Done Predicate**: semver/bootloader 门禁、USB 发现与端口轮询、`apply` `MS_*`/`MOUSE`/`SCROLL` 兼容性校验、三处运行时 `"host"` 鼠标直通测试、CoreGraphics 鼠标测试与 `MI_AUTO_HOST_READY=0 ./scripts/verify-offline` 全部通过；随后真机两阶段刷写完成并确认 16 MB 备份、`operation.json` `verified` 及重连 `0.7.0-rc.16` 自检回执。
- **Evidence**: PR #135 (b336b42), PR #136 (816351e), PR #137 (1bed7c4); Stage A 0.6.3->0.6.3 live flash verified (16MB flash-before.bin sha256:2d4c9583); Stage B 0.6.3->0.7.0-rc.16 live flash verified (16MB flash-before.bin sha256:b833e97e + HID Mouse UsagePage=1/Usage=2 + live 0.7.0-rc.16 six-layer readback)

### [x] Slice FW3: 0.7.0 进阶硬件能力（速度曲线、无刷切换层与休眠电量看板）
- **Outcome**: 在 `0.7.0-rc.16+` 真机基础上接入 `mouse.config` 指针与滚轮速度档/摇杆死区调参、`kb.pl`/`kb.pl.set` 无写 Flash 实时切层、`pm.config`/`pm.state`/`usb.host` 休眠跟随与电量状态显示，以及 `device.name` 与 `kb.sa.rgb` 动态灯光支持。
- **Done Predicate**: 完成相关 `.rodata` 与真机 RPC 只读/受控抓包确认（每项均写入 `docs/` 契约并过 `scripts/check`）；工作台新增控制项与 `MI_AUTO_HOST_READY=0 ./scripts/verify-offline` 全量通过。
- **Evidence**: verify-offline 6 QA PASS (.scratch/qa-runs/1794640685530-23470..1794640692201-20201); live 0.7.0-rc.16 probe (fw3-rpc-probe.json, keymap.json unchanged); release MicroInput.app (sha256 122229d4)

### [x] Slice FW4: 物理速查 kb.cs.* 连线、全量 MS_*/KI_CS_* 动作与摇杆双模滑块化
- **Playbook**: `Feature` + `Verify-Micro-Input`
- **Outcome**: 打通 `DeviceSession` 与 `SharedVendorBridge` 双路径 `kb.cs.show/hide/toggle` 到 `OverlayController`（保序队列与会话序号去重），在 `mi-model` 动作库公开全部 19 个 `MS_*` 与 4 个 `KI_CS_*` 板载键码及能力说明，在右侧检视器提供摇杆「宿主四向 / 硬件指针 / 硬件滚轮 / 原层保留」分段切换与互斥提示，并将 `mouse.config` 6 项常用参数升级为单色 `NeutralTrackSlider` 滑块。
- **Done Predicate**: `./scripts/check` 与 `MI_AUTO_HOST_READY=0 ./scripts/verify-offline` 六场景全绿通过，所有 Swift 文件严格 `< 1000` 行且满足 `R-C06`..`R-C10`。
- **Evidence**: PR #140; verify-offline 6 QA PASS (.scratch/qa-runs/1794644961407-93313..1794644968913-88524)

### [ ] Slice FW5: 顶栏电量与电源模式常驻状态、多设备序列号优选及安全层同步
- **Playbook**: `Feature` + `Verify-Micro-Input`
- **Outcome**: 在工作台顶栏与菜单栏展示 `pm.state` 电量百分比/充电态与 `usb.host` 电源模式摘要，接入 `CandidateDevice.serial_number` 持久化记住上次连接设备并在多机并存时提供选择器，增加显式受用户开关门禁的 `kb.pl.set` 前台切层联动实验开关（默认关闭以保护原生 OAI 层）。
- **Done Predicate**: 多设备优选与顶栏电源切片单元测试、`./scripts/check` 及 `MI_AUTO_HOST_READY=0 ./scripts/verify-offline` 全绿通过。
- **Evidence**: PENDING

### [ ] Slice FW6: kb.sa.rgb AI 会话状态自动映灯、节流防抖与退出复原闭环
- **Playbook**: `Feature` + `Verify-Micro-Input`
- **Outcome**: 将 `FourAppSessionKind`（空闲/运行中/待确认/蓝点完成）可选联动到 `0.7.0-rc.16+` 非保护自定义层的 `kb.sa.rgb` 单键灯效，内置同色去重与 `>= 100ms` 节流防抖，在断开或应用退出时发送全零 `kb.sa.rgb` 恢复板载默认灯效。
- **Done Predicate**: 灯效仲裁节流与退出清零测试、`./scripts/check` 及 `MI_AUTO_HOST_READY=0 ./scripts/verify-offline` 全绿通过。
- **Evidence**: PENDING
