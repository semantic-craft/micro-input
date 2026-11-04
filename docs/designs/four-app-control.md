# 四款桌面应用的键盘控制计划

让 Creator Micro 2 深度控制 Antigravity、Cindy、ZCode、桌面 DeepSeek Harness，达到参考项目控制 Paseo 的能力。Paseo 不作为目标应用。Cindy 原生集成保留，只补缺口。现有 Codex、千问、Herdr 等 Profile 保留用户配置，但不计入本轮验收。

本轮已完成源码、官方接口和安装元数据的只读核查，尚未实现新桥或执行目标应用动作。事实与固定版本统一见[接口证据](../four-app-interfaces.md)。[Paseo 对照](paseo-coverage.md)只用于查漏，不再决定必须复制的模块。

## 执行与验收规则

每片交付一个可观察的结果，按下表依赖推进。四应用调查可并行，代码保持一个写入负责人；需要修改目标应用源码时建立该项目独立工作区，先读其 AGENTS，再固定基线，不直接改安装包。公开源码不等于本机同构建。

默认先用官方接口，再在开源应用中增加受限桥，Antigravity 缺口才做版本绑定的 REA 验证。MCP/CLI/Socket 是按需要选择的通道，不设复制 Paseo 五工具的验收项。不新增定时任务。本轮交付为调查、计划与工单整理，未启动新桥实现。后续按用户明确的四应用目标逐片推进；设备映射写入仍须满足具体计划与恢复条件。

代码片跑各源码仓自己的检查和 Micro Input `scripts/verify-offline`。通过机器验收后可按项目约定更新日常版；原生 API、安装版与实体效果分开记证据，不因人工暂不可用阻塞独立软件片，也不提前勾选硬件通过。文档片只查引用、任务依赖和接力，不跑无关构建。

单应用基准用两个可清理测试会话，覆盖正常、同名不同工作区、目标删除、请求过期、实例重启、断流和重复输入。会话正文与凭据不进入诊断。接口回执区分 accepted/delivered、confirmed、failed、unknown；只有收到目标状态才能称 confirmed。

性能在各适配器第一片记录连接/事件到 HUD 延时和空闲资源基线，暂不编造预算或速度提升。若实测妨碍操作，再以同一场景测前后并设具体门槛。

## 架构边界

Micro Input 继续拥有键盘协议、宿主路由和工作台。应用适配器把原生状态与动作接到准确目标，Cindy 原生输入和灯光由 Cindy 保留。目标地址至少区分应用、运行实例、工作区、会话，动作再带 execution/request ID；字段按应用原生身份映射，不能统一退化成标题或数组序号。

状态需保留来源代次、修订与已知/未知。running、needs-input、failed、completed、unread 是不同事实。无未读来源就保持未知；不把 completed 涂成未读绿灯。能力按只读、精确导航、发送、停止、审批、模型/思考、语音分别声明。

这只是待实现的契约，不先生成一套空框架。第一款真实适配器跑通后提取共同部分。现有 Rust/Swift 两份历史状态机须确定单一运行所有者。四应用之外的配置不删除，也不扩充为本轮工作。

## 工单与来源

总入口为 [#81](https://github.com/semantic-craft/micro-input/issues/81)，继续属于 #44 的完整产品范围。Cindy 使用 [#82](https://github.com/semantic-craft/micro-input/issues/82)，Antigravity 使用 [#83](https://github.com/semantic-craft/micro-input/issues/83)，ZCode 与 DSH 复用 [#33](https://github.com/semantic-craft/micro-input/issues/33)、[#32](https://github.com/semantic-craft/micro-input/issues/32)，共享交互用 [#84](https://github.com/semantic-craft/micro-input/issues/84)，实体灯光与径向验收仍关联 #24/#35。工单记录状态，本页维护执行细节；接力仅指向下一片。

## 逐片任务

F、G、H、I 无互相依赖；默认先做 F 保护 Cindy，再选 G 获得首个完整状态/导航结果。H 与 I 的只读准备可并行。后续同应用写动作必须等对应只读身份和导航确认，不能先做无目标快捷键占位。

### F · 保留 Cindy 原生输入和灯光所有权

- 应用：Cindy。依赖：无。责任票：[#82](https://github.com/semantic-craft/micro-input/issues/82)。
- 结果：增加 Cindy 原生所有权的活跃握手和代次确认；原生持有期间暂停 Micro Input 重叠按键、PTT 与灯光，未知时保持暂停。
- 实现入口：Micro Input HostRouting/PushToTalk；Cindy InputDeviceAction、WorkLouderCodexLightingController。
- 验收：同一宽键/旋钮输入投递两端后业务动作仅一次；切 Profile、失联和旧代回调不双执行；不切换 Cindy 设备开关或改 keymap。 不能只看进程存在；失联标 unknown，移交其他应用前取得释放回执，未确认就不接管。

### G · 接通 ZCode 同实例状态与精确导航

- 应用：ZCode。依赖：无。责任票：[#33](https://github.com/semantic-craft/micro-input/issues/33)。
- 结果：通过最小桌面桥读取 v4 任务索引，在两个同名会话间准确打开目标。
- 实现入口：ZCode client、windowController、zcodeAgent v4；Micro Input 专用适配器。
- 验收：两个工作区与同名任务身份不混淆；selected session 回读吻合；断连/换代/删除后不改开其他任务；默认不读正文。 未授权或越权请求零元数据读取。

### H · 接通 DSH 同 Host 状态与精确导航

- 应用：DSH。依赖：无。责任票：[#32](https://github.com/semantic-craft/micro-input/issues/32)。
- 结果：先核验 0.2.1-rc.5 接口，再在同 Host 读取状态并打开准确会话。
- 实现入口：DSH SessionController、WebController、uiWorkspace.openSession；Micro Input 专用适配器。
- 验收：activate=false 的订阅不启动后台会话；pendingInput 变化不漏；导航回读 ID；停止桥不停止 Host；无认证零读取。

### I · 验证 Antigravity 官方桌面扩展与状态事件

- 应用：Antigravity。依赖：无。责任票：[#83](https://github.com/semantic-craft/micro-input/issues/83)。
- 结果：只读确认安装版 agentapi/Sidecars/Hooks 契约，再在隔离测试工作区接元数据事件。
- 实现入口：Antigravity Sidecar/Hook 插件；Micro Input 状态入口。
- 验收：记录版本及能力；两会话事件不串、重复事件去重、断流标 stale；Stop fullyIdle=false 不报完成；不读 transcriptPath。

### I2 · 核验 Antigravity 既有会话目录与精确导航

- 应用：Antigravity。依赖：I。责任票：[#83](https://github.com/semantic-craft/micro-input/issues/83)。
- 结果：先用官方可用入口，缺少时用 REA 验证会话元数据、工作区与聚焦协议，仅限只读和测试导航。
- 实现入口：Antigravity 版本绑定元数据/聚焦适配器；官方入口或已验证原生 schema。
- 验收：两个既有测试会话目录身份与 UI 选中回读一致；同名跨工作区不混；缺 schema/合法认证就失败关闭；不读取 token 或绕过鉴权。

### J · 补 Cindy 既有任务目录和导航确认

- 应用：Cindy。依赖：F。责任票：[#82](https://github.com/semantic-craft/micro-input/issues/82)。
- 结果：在原生服务旁加受限元数据桥，沿用 session 深链并回读目标。
- 实现入口：Cindy 会话服务、deepLink、原生 IPC；Micro Input 适配器。
- 验收：只列显式授权范围的既有任务；同名会话正确；账号/实例换代失效；delivered 与 confirmed 分开；不放宽 Local Agent API 隔离。 未授权或越权请求零元数据读取。

### K1 · 向 ZCode 准确发送并停止指定执行

- 应用：ZCode。依赖：G。责任票：[#33](https://github.com/semantic-craft/micro-input/issues/33)。
- 结果：接 createSession、sendText 和携带 execution ID 的 stop；运行中发送的 steering 语义单独确认。
- 实现入口：ZCode command v4、queryConversationCommandsV4。
- 验收：两个会话只影响选中者；迟到 stop 不停止新 execution；重复命令不重复发送；超时结果未知时不换 ID 重放。 新建绑定工作区并回读新 ID；运行中插话不误建新会话，不把排队当 steering。

### K2 · 向 DSH 准确发送并停止当前轮次

- 应用：DSH。依赖：H。责任票：[#32](https://github.com/semantic-craft/micro-input/issues/32)。
- 结果：接 create、prompt queue/steer、requestId 与 cancel。补同 Host 的预期 turn 核对，分别显示当前轮次和队列。
- 实现入口：DSH ISession、SessionController；同 Host 原子核对预期 turn 的受限入口，不能在客户端 read-then-cancel。
- 验收：queue 与 steer 两种结果分别确认；cancel 后队列可能继续时不报全部停止；重复请求无双发；切会话无误停。 新建绑定工作区并回读新 ID；同会话旧 Stop 遇新轮次必须零取消，未能证明原子性就拒绝发送。

### K3 · 用 Antigravity agentapi 创建和发送

- 应用：Antigravity。依赖：I、I2。责任票：[#83](https://github.com/semantic-craft/micro-input/issues/83)。
- 结果：创建绑定 projectId，发送绑定 conversationId，取得结果确认。
- 实现入口：Antigravity Sidecar agentapi；Micro Input 适配器。
- 验收：两个测试会话只改目标；不存在目标零重定向；超时不重发；运行中消息是否 steering 单独验证。

### L1 · 接 ZCode 请求审批与思考档位

- 应用：ZCode。依赖：K1。责任票：[#33](https://github.com/semantic-craft/micro-input/issues/33)。
- 结果：按 interactionId/type 处理批准拒绝、问题和计划，复用 switchModelConfig。
- 实现入口：ZCode resolveInteraction、switchModelConfig。
- 验收：过期/重复请求零误批；问题不当权限批准；模型无该档位不发送；回读模型和档位一致。

### L2 · 接 DSH 请求审批与思考档位

- 应用：DSH。依赖：K2。责任票：[#32](https://github.com/semantic-craft/micro-input/issues/32)。
- 结果：先证明当前桌面 approval 接线，再接 request 级回答和 selectModel。
- 实现入口：DSH UserApproval、remote-access、selectModel。
- 验收：取消/迟到答案零副作用；会话切换不改批其他请求；真实模型档位回读；无法证明接线时保留缺口。

### L3 · 补 Antigravity 请求与模型控制缺口

- 应用：Antigravity。依赖：I2、K3。责任票：[#83](https://github.com/semantic-craft/micro-input/issues/83)。
- 结果：逐能力用 REA 核实停止、审批、模型/思考入口，按能力分 PR；语音归 O1。
- 实现入口：版本绑定的 Antigravity 协议适配器；REA 只读定位。
- 验收：每条候选都取得 schema、合法鉴权和两个测试会话结果；字符串存在不算完成；升级/认证失效关闭；不开放任意 RPC。

### M · 补 Cindy 原生任务动作缺口

- 应用：Cindy。依赖：J。责任票：[#82](https://github.com/semantic-craft/micro-input/issues/82)。
- 结果：复用原生服务补准确停止、请求绑定、目标 Prompt/steering 及工作区/待办动作。
- 实现入口：Cindy InputDeviceAction、会话服务、原生消费端。
- 验收：已有任务槽/PTT/思考/灯光行为不退化；停止/审批固定身份，过期零操作；原生未读事实和待办轮次独立核验。

### N · 交付四应用稳定槽位和历史待办导航

- 应用：共享。依赖：G、H、I2、J；写动作按 K/L/M 能力就绪。责任票：[#84](https://github.com/semantic-craft/micro-input/issues/84)。
- 结果：共享稳定目标选择、六槽固定/最近、项目/工作区导航、历史和本轮不重复的待办轮询。
- 实现入口：mi-model 目标/选择模型；HostRouting、DeviceOperations；四适配器。
- 验收：四应用相同名称/不同实例不混淆；新事件重新入队；未知未读不推断；历史回到准确会话；Cindy 原生槽位可复用且不双消费。

### O1 · 逐应用补齐听写和语音回复控制

- 应用：共享。依赖：F；对应应用身份与动作片就绪。责任票：[#84](https://github.com/semantic-craft/micro-input/issues/84)。
- 结果：Cindy 复用原生语音；其余逐款确认并接入按住听写、释放停止、提交前停止，以及独立的语音回复开关。缺少原生能力的开源应用按其语音服务增量实现。
- 实现入口：四应用语音服务/实际接口；Micro Input PushToTalk；Antigravity 未公开部分按 REA 证据。
- 验收：四应用分别确认开始/停止和目标绑定；听写转写与发送分开；开启/关闭语音回复须回读真实模式并验证输出。模型/接口不支持时保持该应用未完成，不以禁用或听写成功代替。

### O · 补齐四应用实体交互与状态反馈

- 应用：共享。依赖：F、N、O1；对应动作能力就绪。责任票：[#84](https://github.com/semantic-craft/micro-input/issues/84)。
- 结果：逐应用接滚动/跳底、八向 Prompt/Stop、可选回弹与逐键/双区反馈，宽键沿用 O1 已验语音能力。
- 实现入口：PushToTalk、RadialMenuState/Routing、灯光服务；Cindy 原生路径。
- 验收：每应用独立证明功能：取消零动作、听写先停后交、真实状态对应 HUD 和灯光；唯一灯光发送者，切层/退出释放明确；语音缺口单列子片，不能以 disabled 签收。

### P · 完成四应用安装与日常验收

- 应用：共享。依赖：F 至 O 的全部任务，含 I2、K1–K3、L1–L3、O1。责任票：[#84](https://github.com/semantic-craft/micro-input/issues/84)。
- 结果：最终构建在四应用中完成相同任务脚本，记录软件/安装/实体三层证据。
- 实现入口：scripts/verify-offline、verify-micro-input；四适配器验证配方。
- 验收：clean commit 全量机器验收通过；每应用两会话完成导航/发送/停止/审批/状态反馈；升级/断连恢复与原映射保留；未完成项不关闭总票。

## 完成范围与已知缺口

四款应用均需覆盖会话/工作区选择与历史、最近/固定槽、待办导航、创建/发送/插话/停止、审批/拒绝、思考控制、语音、旋钮、径向和状态反馈。具体语义以各应用可证实的概念映射。原生已有功能可作为交付组成部分，仍须验证当前安装版；协议暂不可用只能留下缺口，不能删掉承诺或以不支持关闭总目标。

Cindy 的原生操作可大幅减少新增代码；其外部目录桥仍未实现。ZCode 与 DSH 有内部服务但同安装实例外部桥尚未证明。Antigravity 官方 Sidecar/Hooks 是首选，2.29.0 的契约仍需现场核验。三者语音完整控制各自需补证，不能用 Cindy 能力代表它们。

每款适配器分别保留升级兼容检查、合法认证和卸载路径。桥被停止只停止桥；不关闭用户运行中的 Host 或任务。授权/端点重建不自动重放旧发送或审批。Cindy 功能开关可能重写 keymap，不能在握手实验中随手开关。
