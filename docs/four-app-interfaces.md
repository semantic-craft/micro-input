# 四款桌面应用的控制接口

2026-11-05 只读调查。范围为 Antigravity、Cindy、ZCode、桌面 DeepSeek Harness。Paseo 是行为参照，不是适配目标。以下区分公开契约、源码观察和待验证推断；没有读取用户会话正文、凭据或数据库，没有启用插件、切设备模式、注入运行进程或执行会话动作。

## Cindy

公开源码 [makecindy/cindy](https://github.com/makecindy/cindy/tree/3568430ec3751a53d5746a19fb9a1002b8b3b8e7)，SHA `3568430ec3751a53d5746a19fb9a1002b8b3b8e7`，Apache-2.0。本机安装版 `0.1.125`；尚未证明安装包对应这个提交。

| 已观察能力 | 固定源码定位 | 接入限制 |
| --- | --- | --- |
| Creator Micro 2、任务槽、动作布局 | [workLouderCodex.ts](https://github.com/makecindy/cindy/blob/3568430ec3751a53d5746a19fb9a1002b8b3b8e7/apps/desktop/src/shared/workLouderCodex.ts)：`WorkLouderCodexAction`、`WORKLOUDER_CREATOR_MICRO_2_DEFAULT_LAYOUT` | 原生保留；槽位 sessionId 与项目/工作区语义不能混用 |
| 新建/fork/归档/pin、输入、思考强度 | [inputDevices.ts](https://github.com/makecindy/cindy/blob/3568430ec3751a53d5746a19fb9a1002b8b3b8e7/apps/desktop/src/shared/inputDevices.ts)：`INPUT_DEVICE_COMMAND_IDS`、`InputDeviceAction` | 原生动作优先，缺少的命令才增量扩展 |
| 审批/拒绝/计划确认 | [CCAgentSessionView.tsx](https://github.com/makecindy/cindy/blob/3568430ec3751a53d5746a19fb9a1002b8b3b8e7/apps/desktop/src/renderer/features/cc-agent/CCAgentSessionView.tsx)：`ownsHardwareTaskActions`、pendingPermission、pendingPlanReview | 原生当前会话路径已存在；外部控制仍需稳定请求身份 |
| 旋钮切会话/滚动/思考、摇杆滚动/侧栏、逐键与双区状态灯 | [WorkLouderCodexLightingController.ts](https://github.com/makecindy/cindy/blob/3568430ec3751a53d5746a19fb9a1002b8b3b8e7/apps/desktop/src/main/worklouder-codex/WorkLouderCodexLightingController.ts)：`handleHidInput`、`handleEncoderInput`、`handleJoystickInput`、`suspendTaskSlots` | Micro Input 不重复消费输入或同时发送灯光 |
| 短按启停、长按松开停止听写 | [workLouderCodexVoiceGesture.ts](https://github.com/makecindy/cindy/blob/3568430ec3751a53d5746a19fb9a1002b8b3b8e7/apps/desktop/src/renderer/lib/workLouderCodexVoiceGesture.ts)：`createWorkLouderCodexVoiceGesture` | 不叠加 Micro Input 通用 PTT；安装版实际行为待验 |
| Local Agent API、插件任务 API | [local-agent-api.md](https://github.com/makecindy/cindy/blob/3568430ec3751a53d5746a19fb9a1002b8b3b8e7/docs/local-agent-api.md)、[plugin-task-api-implementation.md](https://github.com/makecindy/cindy/blob/3568430ec3751a53d5746a19fb9a1002b8b3b8e7/docs/dev-rules/plugin-task-api-implementation.md) | 只拥有调用方创建的隔离会话/任务，不能接管用户已有任务 |
| `cindy://session/<sessionId>` | [deepLink.ts](https://github.com/makecindy/cindy/blob/3568430ec3751a53d5746a19fb9a1002b8b3b8e7/apps/desktop/src/main/deepLink.ts) | 可作导航入口，但系统接受 URL 仍须与 UI 确认分开 |

`window.electronAPI.workLouderCodex` 是内部 IPC，不是外部 API。推荐先补原生动作，确需跨应用目录和协调时，在 Cindy 源码内加入受限的桌面状态/动作桥，沿用原生服务，不放宽现有隔离 Agent API。

[workLouderCodexHostProcess.ts](https://github.com/makecindy/cindy/blob/3568430ec3751a53d5746a19fb9a1002b8b3b8e7/apps/desktop/src/main/worklouder-codex/workLouderCodexHostProcess.ts) 有 keymap 备份、重绑和恢复；开关原生功能可能触及设备，不能作为无副作用握手。[sdkResolver.ts](https://github.com/makecindy/cindy/blob/3568430ec3751a53d5746a19fb9a1002b8b3b8e7/apps/desktop/src/main/worklouder-codex/sdkResolver.ts) 可加载厂商 SDK 或其他应用内 device-kit，Cindy 的 Apache 许可不覆盖这些依赖。[CodexMicroGuardService.ts](https://github.com/makecindy/cindy/blob/3568430ec3751a53d5746a19fb9a1002b8b3b8e7/apps/desktop/src/main/worklouder-codex/CodexMicroGuardService.ts) 当前为 cleanup-only，不能当作已生效的冲突隔离。

Slice J 已在固定提交的 Cindy `worklouder-codex` 原生服务旁加入受限任务元数据与深链导航确认桥（见 [Cindy 原生所有权与任务导航桥](../integrations/cindy/README.md)）：仅列出 `cindy-grant.json` 显式授权工作区（`scopes: [{ workspacePath }]`）内的既有活跃可见任务（过滤归档、隐藏与 Orca worker），不创建任务、不暴露消息正文，且保持 `apps/desktop/src/main/local-agent/` 与 `cindy.tasks` 插件隔离零改动；导航请求先经 `buildSessionDeepLink` + `parseDeepLink` 校验 `cindy://session/<sessionId>`，渲染端尚未提交目标路由时返回 `delivered`（`delivered = target`, `selected = nil`），渲染端经 `createSessionEntryNavigator.onLocationCommitted` 回报路由提交且窗口聚焦确认后才返回 `confirmed`（`delivered = target`, `selected = target`）；账号/实例换代（`instanceId` / `generation` / `ownerEpoch` / `sourceGeneration`）使旧目录与选择立即 `stale` 失效，删除目标绝不回退打开同名兄弟会话，未授权或越权请求零元数据读取。

## Antigravity

本机为独立 Electron App `2.29.0`，bundle `com.google.antigravity`，不是旧 VS Code IDE。`app.asar` SHA-256 为 `290e1b7a1caa430a0c5367c7b433e8d284e2ca5d0b305e91e5ce8ec926f43283`。

| 官方入口 | 已核实契约 | 未证明的部分 |
| --- | --- | --- |
| [Sidecars](https://antigravity.google/docs/sidecars/) | App 管理生命周期；`agentapi new-conversation` 绑定 projectId；`send-message` 接受 conversation ID | 本机 2.29.0 可用性、执行结果回读、运行中 steering 尚未实测 |
| [Hooks](https://antigravity.google/docs/hooks/) | 会话 ID、工作区与生命周期；Stop 有 fullyIdle | PreToolUse 不证明等待批准；Stop 不等于所有后台工作结束；不读取 transcriptPath 正文 |
| [Plugins](https://antigravity.google/docs/build-plugins/) | 分发 hooks、MCP、skills、subagents，可结合 Sidecars | MCP 不是外部任意会话控制权 |
| [Remote Control](https://antigravity.google/docs/remote-control/) | 官方 UI 能控制桌面已有会话 | 未证明第三方可用 API，本轮未启用、抓包或访问账号实例 |
| [Python SDK](https://antigravity.google/docs/sdk/overview/)、[CLI ACP](https://antigravity.google/docs/cli/acp/) | 创建自有 runtime 或 CLI 子进程 | 不能据此接管已有桌面会话 |

REA 对上述安装包作静态部分分析：`dist/languageServer.js:startLanguageServer` 启动随包服务；`dist/hostBridgeServer.js:buildRoutes` 只含更新接口，不能当会话桥。`preload.js` 有 deepLinkAPI，但精确 conversation URI 未证实。随包 `language_server` 的路由字符串含 `GetConversationMetadata`、`StreamAgentStateUpdates`、`SendUserCascadeMessage`、`CancelCascadeInvocation`、`HandleCascadeUserInteraction`，只作为候选，schema、鉴权和实际行为均未核验。

本地证据为 `.scratch/four-app-control/antigravity/rea-evidence.json`，parent Evidence `ev_3120193348899ade4fa5dca5cc986cd6a08051c5c00e36c57ed13b268bf459b9` 与 `ev_14b4cad671897cd858021eb13e2d0725c520eb7c23d6d6efdc3feb0a659d1e37`。原始包内容不提交。推荐官方 Sidecar/Hooks 优先，REA 只针对剩余元数据、导航与请求控制，不扩成任意 RPC 代理。

Slice I、Slice I2、Slice K3 与 Slice L3 已由 `scripts/verify-antigravity-installed-static` 在只读模式下核对安装版 `/Applications/Antigravity.app`（`2.29.0`，`language_server` `Built at CL: 1013060670`）、`agentapi --help`（暴露 `get-conversation-metadata`、`new-conversation`、`send-message`，且 `send-message` 无显式 `--steer` CLI 标志）、`sidecar_sdk` `1.0.0`（绑定 `127.0.0.1`，要求 `ANTIGRAVITY_LS_ADDRESS`、`ANTIGRAVITY_CSRF_TOKEN`、`ANTIGRAVITY_SIDECAR_WEB_PORT`、`ANTIGRAVITY_PROJECT_ID` 与 `X-Sidecar-Token`，并通过 `workspace-change` 接收 `workspaceUris`、通过 `toggle-conversation` 请求 UI 聚焦）及 `language_server` 的 `GetConversationMetadata` / `GetAllCascadeTrajectories` / `CascadeTrajectorySummary` / `CortexWorkspaceMetadata.workspace_folder_absolute_uri` / `SendAgentMessageRequest` / `MessageDeliveryStrategy` 与 L3 的 `CancelCascadeInvocationRequest`（`cascade_id`, `kill_background_tasks`, `notify_parent`）、`HandleCascadeUserInteractionRequest` / `CascadeUserInteraction`（`trajectory_id`, `step_index`, `run_command`, `file_permission`, `permission`, `ask_question`, `approval_interaction`, `selected_option_ids`）、`GetCascadeModelConfigs` / `CascadeConfig.planner_config` / `CascadePlannerConfig`（`requested_model`, `thinking_level`：`THINKING_LEVEL_LOW` / `THINKING_LEVEL_MEDIUM` / `THINKING_LEVEL_HIGH`、`supported_thinking_levels`）protobuf 符号契约与 `x-codeium-csrf-token` 鉴权要求。隔离测试工作区经 [Antigravity 状态与导航桥](../integrations/antigravity/README.md) 与 `scripts/verify-antigravity-paired` 验证：两个既有测试会话目录身份与 UI `selected` 回读一致（`confirmed`）、同名跨工作区按 `(conversationId, workspacePath)` 复合主键严格隔离、错误选中回读与已删除目标返回 `unconfirmed` 且绝不重定向、缺 Schema 或合法认证立即失败关闭（`denied` / `stale`）、六个官方 Lifecycle Hooks 元数据事件去重与 `Stop(fullyIdle=false)` 归入 `backgroundActive`、`agentapi` 创建绑定 `projectId` 并回读确认 `selected`、发送绑定 `conversationId` 且两测试会话仅改当前选中目标、不存在/已删除目标零重定向零新建、超时/未知结果锁定原 `requestId` 通过 `query` 对账不重发、运行中会话消息默认归入 `nextInvocation` 排队与显式 `steered` 单独验证、L3 停止仅作用于当前选中会话且迟到 `expectedInvocationNum` 返回 `noop`、交互审批/提问回答严格阻断将 `askQuestion` 当作权限批准（`guard.questionCannotApproveAsPermission`）且已过期交互返回 `noop`、模型与思考档位切换前置校验 `supportedThinkingLevels` 并经回读确认、绝不开放任意 RPC（非白名单操作立即返回 `denied` / `error.unsupportedOperation`），全程零次读取用户 Token 与 `transcriptPath`。语音控制归入后续 Slice O1。

## ZCode

官方仓库 [zai-org/ZCode](https://github.com/zai-org/ZCode/tree/a9fe401eede6e6e5e01be9048679c206fe6f02d9)，SHA `a9fe401eede6e6e5e01be9048679c206fe6f02d9`，Apache-2.0。本机 `dev.zcode.app`，版本 `3.17.2`、build `3.17.2.9661`；公开源码版本同为 3.17.2，但不证明逐字对应。

| 源码定位 | 已观察接口 | 待验证边界 |
| --- | --- | --- |
| [client/index.ts](https://github.com/zai-org/ZCode/blob/a9fe401eede6e6e5e01be9048679c206fe6f02d9/packages/client/src/index.ts) | connectViaMessagePort、connectViaWebSocket、RemoteServiceAccess | 尚未证明外部客户端可接当前桌面的同实例，不另起 Web runtime 冒充 |
| [zcodeSession.ts](https://github.com/zai-org/ZCode/blob/a9fe401eede6e6e5e01be9048679c206fe6f02d9/packages/services/src/zcode-session/zcodeSession.ts) | getWorkspaceRuntimeIdentity、listSessions、readSession、createSession、setThoughtLevel | 默认只投影元数据，不读取正文 |
| [windowController.ts](https://github.com/zai-org/ZCode/blob/a9fe401eede6e6e5e01be9048679c206fe6f02d9/packages/services/src/window-controller/windowController.ts) | listTaskList、subscribeControllerV4、onDynamicControllerFrame、onDynamicTaskUnreadSignal、mutateTask open | `mutateTask(open)` 只校验来源，不导航；精确打开须由同窗口 renderer 选择与回读，完整地址包含 sourceServer、remoteSession、workspace、taskId |
| [zcodeAgent.ts](https://github.com/zai-org/ZCode/blob/a9fe401eede6e6e5e01be9048679c206fe6f02d9/packages/services/src/zcode-agent/zcodeAgent.ts) | helloConversationV4、subscribeConversationV4、resyncConversationV4、subscribeSessionsIndexV4、sendConversationCommandV4、queryConversationCommandsV4 | 旧 onDynamicSessionEvent 已弃用；重连与补帧以 v4 为准 |
| [command.ts](https://github.com/zai-org/ZCode/blob/a9fe401eede6e6e5e01be9048679c206fe6f02d9/packages/shared/src/zcode-protocol-v4/command.ts) | sendText、stop、resolveInteraction、switchModelConfig | stop 的 expectedForegroundExecutionId 虽为可选字段，键盘适配器应携带；审批绑定 interactionId 与类型 |

现有 `zcode://workspace/open` 仅有工作区打开证据；没有建立稳定 session 深链。MCP Apps UI 插件也不是全局会话控制 SDK。推荐在原桌面 host 服务层增加最窄的显式本机桥，复用 v4；语音输入与语音回答控制尚未确认。

Slice G、Slice K1 与 Slice L1 已在固定提交 `a9fe401eede6e6e5e01be9048679c206fe6f02d9` 的 ZCode 源码上接通受限 UDS 导航、受控执行与请求审批/思考档位桥（见 [ZCode 同实例导航与受控执行桥](../integrations/zcode/README.md)）：通过 `windowControllerService.readMicroInputIndex` 仅投影授权 `ZCodeScope` 内的地址、标题、`liveStatus`、运行中 `executionId`、脱敏待处理交互元数据 `pendingInteraction`（区分 `permission`、`question`、`plan`，零正文载荷）与模型思考档位配置 `modelConfig`，通过 `sendMicroInputCommand` / `queryMicroInputCommand` 复用同实例 `zcodeAgent.sendConversationCommandV4` 与 `queryConversationCommandsV4` 执行 `createSession`、`sendText`、带 `expectedForegroundExecutionId` 的 `stop`、按 `(interactionId, interactionType)` 绑定的 `resolveInteraction` 和带 CAS `baseRevision` 的 `switchModelConfig`。两个会话严格限制为仅影响当前选中目标（`selected == target`）；运行中会话发送消息始终走 `sendText`（绝不误调 `createSession`），并按 `inputAccepted.delivery` 区分实时 steering（`guide` → `steered`）与普通排队（`queue` → `queued`）；迟到 `stop` 因 `expectedForegroundExecutionId` 不匹配返回 `noop`（`guard.stopTargetChanged`），不停止新 execution；过期或已解决交互返回 `noop`（`proto.alreadyResolved`），重复 `commandId` 幂等返回 `duplicate: true` 零误批；问题（`AskUserQuestion`）绝不当作权限批准（`guard.questionCannotApproveAsPermission` 零底层调用）；目标模型不支持所请求的思考档位时不发送 `switchModelConfig`（`guard.unsupportedThoughtLevel`），切换后回读 `(provider, model, thought)` 一致才确认。

## DeepSeek Harness 桌面版

本机 `com.deepseek.dsh` 的版本为 `0.2.1-rc.5`。本轮改用对应官方 tag [dsh-v0.2.1-rc.5](https://github.com/deepseek-ai/deepseek-harness/tree/b830ec950faba0a46dda564be3e2d854afed94f1)，固定 SHA `b830ec950faba0a46dda564be3e2d854afed94f1`，MIT。此前调查的 `fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159` 为较新源码，不能作为安装版契约。

`scripts/verify-dsh-installed-static` 核验 Info.plist、ASAR 文件完整性和随包接口。安装包 ASAR SHA-256 为 `8dad52f295bdbe9fa71948fdf38ac2ce3c910213d3b89c37174b2af77db9fb31`。这证明本轮检查了安装包静态接口，不证明源码与整个二进制逐字对应，也没有读取账户凭据或会话数据库。

| rc.5 固定源码入口 | 本轮确认 | H 的接入约束 |
| --- | --- | --- |
| `apps/desktop-host/src/index.ts` | 桌面启动已有 Host Context，生命周期由桌面拥有 | 桥装入同一个 Host；不得另起 SDK runtime，也不提取 Web Host cookie |
| `packages/api/session-controller/src/history.ts`、`types.ts` | `follow` 调用 `sourceFor(address, signal, true)` 并返回历史记录；`SessionFollowRequest` 没有 `activate` 字段 | 不使用这个正文流。桥的 `subscribe(activate:false)` 是新加的限定契约，认证后只读已加载会话的内存状态 |
| `packages/api/session-controller/src/control.ts` | control 的基线和更新是投影；不存在此前描述的原生 `observeState`、`pendingInput` API | 桥只投影标题、workspace/session ID、active/running 和待回应数量，不传播投影正文 |
| `packages/interaction/user-questions/src/index.ts`、`types.ts` | 定时提问有 `userQuestions.active` 投影；旧式阻塞提问由服务持有请求 | H 给生产服务增加只读待回应计数与观察口，覆盖订阅前已存在的提问和之后的变化，不把排队消息当成等待用户 |
| `packages/interaction/user-approval/src/index.ts` | approval request 生命周期由服务拥有，历史审计含 asked/decided | H 在生产者保存当前待审批计数，桥无需扫描历史；`pendingInput` 为提问与审批数之和 |
| `packages/client/ui-workspace/src/client/navigation.ts` | `openSession` 沿真实 selection 与 layout 选择会话 | 原桌面窗口回读选中 session ID；workspace 和 Host 代次也必须吻合，才返回 confirmed |

[DSH 导航桥](../integrations/dsh/README.md) 默认关闭，只连接显式授权的 session ID。授权范围内尚未加载、当前元数据尚未就绪或已经消失的会话单列为不可用，不伪装为空闲，也不为了列举而加载或启动会话。订阅返回基线，后续按连续序号返回状态变化；即使同一次轮询前等待状态已经出现又消失，两个变化仍保留。日志缓冲耗尽返回 gap，客户端清空旧目标并要求重新连接。

每次连接有独立 subscriptionId；旧订阅的迟到关闭不能清除新连接，已有订阅也不接受另一个 ID 的替换。旧连接尚未完成时先排空，再清理并重连；清理无法确认时拒绝接管，须重置桥恢复。停止桥只释放自己的观察器、订阅和端点，不停止 Host、Agent 或会话。Slice K2 已在同 Host 桥上接通 `create`（绑定授权 `workspaceId` 并回读 `selected == createdSessionId`）、`prompt`（区分 `steer` 与 `queue`）、`requestId` 幂等去重/`query` 对账与带同 Host 同步 `expectedTurn` 原子核对的 `cancel`（分别投影 `currentTurn` 与 `queuedCount`；`queuedCount > 0` 时返回 `cancelledQueueRemaining` 不报全部停止；切会话返回 `unconfirmed` 不误停；同会话旧 Stop 遇新轮次零取消 `noop`；无法证明轮次原子性时拒绝执行 `guard.unprovableTurnAtomicity`）。审批、提问回答与模型档位仍属于 L2。安装版原生窗口、已安装桥和实体控件的结果与源码配对验收分别记录。

## 本轮验证与后续入口

以上足以确定调查顺序和实现入口，不代表外部桥已存在或最终接口可用。当前 Micro Input 的 `DesktopAgents.swift`、`HostActionExecutor.swift`、`DeviceOperations.swift` 主要是 AX、快捷键、滚动和通用历史，尚未接入上述四应用完整会话 API。

本轮由主代理抽查 Cindy Local Agent API 隔离契约、ZCode v4 命令字段、DSH ISession 契约与官方 Antigravity Sidecars/Hooks，核对三路调查结论。没有运行这些应用的构建/接口集成测试。临时克隆已清理，脱敏简报与 REA 回执保留在 `.scratch/four-app-control/`。

参考 Input IA-04/10/11/16/17 与[研究接入规则](agents/research-context.md)：设备会话、宿主动作、原生灯光所有权与配置写入分别处理。具体任务与验收见[四应用控制计划](designs/four-app-control.md)。
