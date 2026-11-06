# Paseo 能力覆盖与架构决策

## 结论与基线

目标已于 2026-11-05 明确收敛：只在 Antigravity、Cindy、ZCode、桌面版 DeepSeek Harness 达到参考项目控制 Paseo 的深度，不使用 Paseo。保留 Cindy 原生集成，只补缺口。当前接入证据见[四应用接口](../four-app-interfaces.md)，执行任务见[四应用控制计划](four-app-control.md)。

本页保留参考项目的能力基准，用于发现遗漏。下表描述本地 e9d435e 之前的 Micro Input 实现，不包含目标应用自身已经提供的全部能力，尤其不能用它判断 Cindy 原生能力缺失。

比较基线：上游 [aca834c3094a64d2d2db001fb1c89f9c4d36e584](https://github.com/mattt/paseo-creator-micro/tree/aca834c3094a64d2d2db001fb1c89f9c4d36e584)，Micro Input `dead00254b6729af77b1fd220bcfd65edacd0c42`。上游源码与测试已阅读，未构建运行上游、未向设备发送命令。以下上游路径均相对此固定提交。

**尚未全覆盖。** 配置编辑、保留未知字段、备份、读回及恢复方面我们范围更广；Paseo 的会话状态、工作区选择、逐键反馈和完整控制闭环仍明显领先。A–E 完成只表示第一轮通用交互迁移，不表示全面对齐，更不表示七套应用分别验收。

## 架构及可取之处

上游分三层：`CreatorMicro`（Swift IOKit、HID/RPC、输入与灯光）→ `PaseoMicroCore` / 菜单栏 App（会话、浮窗、滚动、Unix socket 与 MCP）→ TypeScript Paseo 插件（SDK 状态订阅、工作区和 Agent 选择、动作规划、灯光计算）。插件调用 Paseo SDK/CLI，App 执行 macOS 操作；并不是键盘固件直接理解 Agent。

- `Sources/CreatorMicro/HIDTransport.swift`、`Report.swift`、`RPC.swift`、`KeypadClient.swift`：vendor usage、64 字节报告、61 字节载荷、JSON 重组、请求关联、超时与取消。支持 PID 8297/8298 是上游匹配策略，不扩大我们的型号/固件承诺。请求 ID 随机起点也不等于跨客户端互斥。
- `Sources/PaseoMicroCore/KeypadSession.swift`、`InputLayers.swift`：读取 keymap 识别含 `KV_OAI_` 的输入层及可单独亮灯的键；丢失事件明确结束流；保留期望灯光并在重连后重发。检测 ChatGPT，相关层暂停灯光，其他层定期重绘。这是协调策略，不是对其他客户端的独占权。
- `Plugin/server/controller.ts`、`actions.ts`：输入时冻结选择与动作，队列执行前再次核对目标，合并滚动/思考级别变化，过期目标不重定向。`actionable.ts` 以事件身份记录一轮访问，新增请求可重新入队，比单纯按最近活动排序准确。
- `BridgeServer.swift`：Unix socket 0600、目录 0700、检查占用与所有权，仅删除自己创建的 socket。MCP 为独立 loopback HTTP 出口，CLI 复用桥接。值得借鉴接口分层，不必照搬整个服务拓扑。
- 上游明确拒绝设备文件写入；自定义层的准备没有受支持工具。当前固件的出厂层不提供其全部摇杆/逐键灯光体验；代码存在不是我们的实机证明。

我们继续采用 `mi-transport → mi-proto → mi-session → mi-model/mi-ffi → SwiftUI Store/执行器`。设备连接与写入应只有现有 session 拥有，避免再接入一套 Swift HID 后端。真正需要增加的是**目标应用适配器的会话/权限/状态能力**，以及灯光所有权与反馈服务。现有 Rust `navigation_history.rs` 与 Swift `KeypadNavigationHistory` 有两份相近状态机；实际 Store 使用 Swift 版本，后续应明确单一所有者，不能把 Rust 单测当成 UI 运行链证明。

## 完整覆盖清单

“部分”包括实现语义不同、仅部分适配器接通或缺少最终效果验证。动作名来自上游 `Plugin/shared/actions.ts`，包含全部 27 个非空动作及 `none`。

| 能力 / 上游动作 | 当前证据与差距 | 上轮接力编号 |
| --- | --- | --- |
| USB、RPC、热插拔、层识别、宽键合并 | 已有独立后端、唯一 reader、固件门禁及 AG/ACT/ENC 映射；完整断连/睡眠实体验收仍待补。上游的 `kb.radial` 不因源码发现而自动开放 | K |
| `none`、自定义映射、输入区聚焦 `focusComposer` | 本地动作与目标应用聚焦已实现；各应用可达性分别判断 | F/K |
| `scrollUp/Down`、`scrollUpFast/DownFast`、`jumpToBottom` | `DesktopAgents.swift` / `HostRouting.swift` 已有 3/15 行、100 行合并上限、1000 行分段跳底与按住加速；滚动事件投递不保证实际对话到底，遮挡/多屏与实体旋钮需验 | K |
| `pushToTalk`、`submit` | `PushToTalk.swift`、`DeviceOperations.swift` 已有固定目标、宽键合并及先停止听写再提交；Cindy 原生集成必须优先，不能假设通用路由阻止原生重复消费；八应用并非全部实测通过 | F/K |
| `navigateBack/Forward` | `HostRouting.swift`、`DeviceOperations.swift` 有 50 项历史；支持 app/session/T3 目标，但应用聚焦不等于精确会话恢复，T3 深链 delivered 不等于已打开确认 | G/K |
| `nextActionableAgent`、`nextStuckWorkspace` | T3 只有专用待办导航；缺跨适配器统一的失败/权限/未读状态及工作区轮询。完成状态不能推测未读 | G |
| `previousProject/nextProject`、`previousWorkspace/nextWorkspace` | 个别应用快捷键不能替代稳定项目/工作区身份、排序、排除归档和选择规则；通用模型未覆盖 | G |
| `selectNextWorkspace/selectPreviousWorkspace/openSelectedWorkspace` | 本地 Profile 选择与应用打开不是工作区选择；未建立等价接口 | G |
| Recent agents / Workspaces / Pinned workspaces / Off 顶部六键模式 | Micro Input 通用路径缺稳定六槽、固定工作区、单击选择/双击打开、子 Agent 筛选、亮度表示当前选择及选择恢复；Cindy 原生路径另见四应用接口 | G/H |
| `newAgent` | 千问、ZCode 等有新任务动作；各适配器缺统一创建后目标身份确认，不能算全覆盖 | G |
| `approvePermission/denyPermission` | 尚无通用请求 ID 绑定的审批/拒绝执行；通用 Return 或快捷键不等价，过期请求必须零操作 | G |
| `stopAgent`、`thinkingUp/thinkingDown`、`toggleVoiceMode` | 未形成跨适配器可核验能力；按住听写不等于语音回答开关；思考级别须读取目标模型可用选项 | G |
| `sendPrompt` | 已有四向/八槽文本及显式提交，默认仅粘贴；Paseo 可把文本投递给准确 Agent，运行中作为 steering。跨应用输入框粘贴不能证明同一语义 | G/I |
| 八向径向手势、禁用/取消、槽位外观 | `RadialMenuState.swift` 为旋钮确认/回中取消；上游 `RadialSelector.swift` 为快速回弹确认/慢回中取消。当前预设以 Submit 代替上游 Stop，图标/颜色配置与 Escape 拦截也未对齐 | I |
| 逐键状态与双区运行时反馈 | 当前 RGB 编辑/预览不等于 `v.oai.thstatus` / `v.oai.rgbcfg` 的实时状态灯；红失败/黄待输入/蓝运行/绿未读、选中亮度、失败闪烁与思考级别提示缺闭环 | H |
| 灯光效果、当前层可见性、竞争/重连 | 上游另含 shallowBreathing=6 候选、AG 与 ACT 的逐键灯光差异、期望状态重发；我们需核验当前固件并提供释放/恢复，不凭 setter ACK 宣称恢复 | H |
| MCP / CLI / 仿真 | 上游有状态、逐键/分区灯光、清灯、等待输入五项 MCP 工具，以及 socket CLI / 仿真。我们只有本地草稿 CLI 和隔离 QA；此项不是本次用户目标。仅当四应用接入需要时采用 MCP/CLI，不复制 Paseo 的五工具产品出口 | J |
| 登录启动、权限、安装/升级、状态界面 | 已有菜单栏/诊断/QA，但未找到等价 SMAppService 登录启动；安装包升级及权限维持仍归 #43，不以开发构建通过替代 | K |

本地入口：`app/MicroInput/Sources/MicroInput/{DesktopAgents,HostRouting,DeviceOperations,PushToTalk,RadialMenuState,RadialMenuRouting}.swift`；`crates/mi-model/src/navigation_history.rs`；协议与配置见 [模块契约](../module-contracts.md)。

## 复用决定与验证边界

上游根 `LICENSE.md` 为 MIT，允许复用，复制或实质移植时保留 Mattt 版权及许可；本轮没有引入第三方代码。优先复用/移植纯状态算法及其行为向量：`actionable.ts`、`slots.ts`、`projects.ts`、`workspaces.ts`、`RadialSelector.swift`、灯光计算。不建立 Paseo 适配器；四款目标应用各用自己的可用接口，不能宣称自动获得同等能力。上游带副作用的模拟桥会执行真实 Paseo 动作，不能当离线测试入口。

“四应用达到同等级控制深度”是验收目标：缺少接口的应用明确记缺口，不用 disabled 或快捷键占位关闭。Cindy 已有原生键盘能力应保留；先逐能力确定唯一消费者，再补其没有的能力。旧 F–K 是上轮范围，已由四应用控制计划与现行接力队列替代；灯光与设备文件写入仍须具体目标、备份/恢复条件，不能从本次评估推导任意改写授权。

本轮复核了 A–E 引用的 E 回执：`1793801884388-98896/evidence/result.json` 是 host-execution 的 24 项回执，status=PASS、physical_key/lighting_visual=NOT_RUN，记录基线 `95012d0` 且 working_tree_changes=true；它不是当前 clean HEAD 的全量验收证明。安装版与本地 target 的主可执行文件 SHA-256 相同（`00d266d68fd5aaccfc0b006bccbaad7abca023984f8b6b6df2319687a0a2e2c4`），只能证明两份二进制相同，不证明源码对应关系、整个 app bundle 或实时体验。

参考 Input 研究 IA-04/07/10/11/16/17：保留队列、来源身份、宿主边界及设备写入恢复约束。没有新增设备操作、原生 UI 验收或全量构建；本轮只验证文档链接、差异、接力脚本及当前 Git 状态。#44 继续保持完整产品最终验收入口。
