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

## DeepSeek Harness 桌面版

官方仓库 [deepseek-ai/deepseek-harness](https://github.com/deepseek-ai/deepseek-harness/tree/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159)，SHA `fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159`，MIT。本机 `com.deepseek.dsh`、`0.2.1-rc.5`；公开源码桌面包为 `0.2.2-rc.1`，先核验版本差异，不直接按主线接口调用安装版。

| 源码定位 | 已观察接口 | 待验证边界 |
| --- | --- | --- |
| [Desktop README](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/apps/desktop/README.md) | Electron 启动 bundled Host，HTTP/WS 接认证 Web Host，随机 loopback 端口 | 凭据只给所属窗口；不提取 cookie。Python SDK 的独立 runtime 不能替代同 Host 接入 |
| [SessionController](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/api/session-controller/src/index.ts) | ctx.remote.session 的 list/search/create/selectModel/prompt/updateQueue/cancel/follow/projections | 需要同 Host 插件或受限本机桥，外部接入尚未实测 |
| [types.ts](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/api/session-controller/src/types.ts) | active/running/queue/pendingInput；SessionControlFrame baseline+projection | pending 变化不独立触发 observeState，不得漏掉待回应；follow 默认为 activate=false，首片保持不激活 |
| [ISession](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/api/session-controller/src/client/contract/session.ts) | prompt 的 queue/steer 与 requestId，cancel | cancel 只取消当前 turn，队列仍可能继续，原生契约未提供 expected turn。K2 须补同 Host 原子核对，客户端先读再取消不构成保证；停止与清队列分开定义 |
| [UI apply.ts](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/client/ui-chat/src/client/apply.ts) | ctx.uiWorkspace.openSession(childId) | 候选精确导航；dsh://open 仅聚焦主窗，不能作为会话确认 |
| [UserApproval](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/interaction/user-approval/src/index.ts) | ask/answerer、取消与迟到答案丢弃 | 仍须核对桌面 Remote 请求/答案接线；不能套用独立 SDK 协议 |

推荐同 Host 插件连接 sessionController 与 UI 导航。模型档位以实际 provider 返回为准；语音输入、语音回答开关仍是独立缺口。

## 本轮验证与后续入口

以上足以确定调查顺序和实现入口，不代表外部桥已存在或最终接口可用。当前 Micro Input 的 `DesktopAgents.swift`、`HostActionExecutor.swift`、`DeviceOperations.swift` 主要是 AX、快捷键、滚动和通用历史，尚未接入上述四应用完整会话 API。

本轮由主代理抽查 Cindy Local Agent API 隔离契约、ZCode v4 命令字段、DSH ISession 契约与官方 Antigravity Sidecars/Hooks，核对三路调查结论。没有运行这些应用的构建/接口集成测试。临时克隆已清理，脱敏简报与 REA 回执保留在 `.scratch/four-app-control/`。

参考 Input IA-04/10/11/16/17 与[研究接入规则](agents/research-context.md)：设备会话、宿主动作、原生灯光所有权与配置写入分别处理。具体任务与验收见[四应用控制计划](designs/four-app-control.md)。
