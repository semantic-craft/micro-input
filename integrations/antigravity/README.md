# Antigravity 官方桌面扩展契约、既有会话目录与状态导航桥

本目录记录安装版 `/Applications/Antigravity.app`（`com.google.antigravity` `2.29.0`，`language_server` `Built at CL: 1013060670`）只读静态与 REA 协议核对结果，并提供在隔离测试工作区接收既有会话目录、执行精确会话聚焦回读确认与接收官方 Lifecycle Hooks 元数据事件的 Unix 域套接字桥 `hooks-bridge.py`。

## 安装版官方接口、只读 REA 契约与能力分区

`scripts/verify-antigravity-installed-static` 在只读模式下核对安装版包体、ASAR 头部文件完整性哈希、`language_server --stamp`、只读 protobuf 符号契约、`agentapi --help` 与 `~/.gemini/antigravity/lib/node_modules/sidecar_sdk`：

1. **包体与核心二进制**：
   - Bundle ID：`com.google.antigravity`，版本 `2.29.0`
   - `Contents/Resources/app.asar` SHA-256：`290e1b7a1caa430a0c5367c7b433e8d284e2ca5d0b305e91e5ce8ec926f43283`
   - `Contents/Resources/bin/language_server` SHA-256：`fb6e0fc3a1e091e773a3e91ce3e5a4dc93a5f0d30bf305a1ddd70008a2a26a74`（`Built at CL: 1013060670`）
2. **`agentapi` CLI 与创建/发送契约（Slice K3）**：
   - 仅暴露 3 个官方子命令：`get-conversation-metadata <conversation_id>`、`new-conversation [--model=...] [--title=...] [--profile=...] <prompt>`、`send-message [--title=...] <recipient_id> <content>`。
   - **创建绑定 `projectId`**：`sidecar_sdk/index.mjs` 的 `startConversation` 通过环境变量 `ANTIGRAVITY_PROJECT_ID` 将 `agentapi new-conversation` 绑定到工作区 `projectId`（记录 `NewConversationEvent`）；若请求 `projectId` 与工作区绑定不一致则返回 `unconfirmed`（`guard.projectIdMismatch`），零新建会话。
   - **发送绑定 `conversationId` 与运行中消息投递语义**：`sendMessage` 调用 `agentapi send-message [--title=...] <conversationId> <content>`（`SendAgentMessageRequest` / `SendMessageEvent`）。官方 `agentapi` CLI 不提供显式 `--steer` 或 `--delivery-strategy` 标志（`agentapi_explicit_steer_flag == false`），`language_server` 内部采用 `MessageDeliveryStrategy`（`MESSAGE_DELIVERY_STRATEGY_NEXT_INVOCATION` / `MESSAGE_DELIVERY_STRATEGY_WHEN_IDLE`）。因此：
     - 目标会话处于 `idle` / `completed` / `failed` / `canceled` 时，发送返回 `started`（`delivery == "started"`，`inFlightSteering == false`）；
     - 目标会话处于运行中（`running` / `runningTool` / `backgroundActive`）时，默认按官方 `agentapi` 契约归入下一轮调用排队 `queued`（`delivery == "nextInvocation"`，`inFlightSteering == false`，绝不误新建会话，也不虚报同步抢占 `steered`）；仅当桥显式开启运行中 steering 模式时才确认 `steered`（`delivery == "steered"`，`inFlightSteering == true`），两类语义单独验证。
   - **CLI 限制**：独立 `agentapi` CLI 不提供既有会话全量枚举、窗口聚焦、停止当前轮次或审批放行；停止、交互审批/问答与模型/思考档位控制走下述经只读 REA 核实的 `language_server` 固定白名单契约（Slice L3）。
3. **Official Sidecars (`sidecar_sdk` `1.0.0`) 与只读 REA 会话目录/聚焦契约**：
   - 官方 Sidecar 仅绑定 `127.0.0.1`，要求宿主注入 `ANTIGRAVITY_LS_ADDRESS`、`ANTIGRAVITY_CSRF_TOKEN`、`ANTIGRAVITY_SIDECAR_WEB_PORT` 及 `X-Sidecar-Token`（`ANTIGRAVITY_SIDECAR_UI_TOKEN`），暴露 `/_sidecar/get-conversation-metadata`、`/_sidecar/new-conversation`、`/_sidecar/send-message`。
   - `preload.js` 通过 `window.addEventListener('message', ...)` 接收宿主推送的 `workspace-change`（`payload.workspaceUris`），并通过 `window.sidecar.ui.toggleConversation(conversationId)` 向父窗口发送 `postMessage({ type: 'toggle-conversation', payload: { conversationId } }, '*')`。
   - 对 `language_server`（`CL 1013060670`）的只读 REA 确认：`LanguageServerService/GetConversationMetadata`（`GetConversationMetadataRequest` / `GetConversationMetadataResponse`）与 `GetAllCascadeTrajectories`（`CascadeTrajectorySummary` + `CortexTrajectoryMetadata` + `CortexWorkspaceMetadata.workspace_folder_absolute_uri`）在 `x-codeium-csrf-token` 保护下将会话身份 `(conversation_id, workspace_folder_absolute_uri)` 与 `summary` / `title`、`project_id` 绑定。
   - 经上述契约核对后，状态与导航桥支持既有会话目录与精确导航（`supportsSessionCatalog = true`、`supportsTargetFocus = true`、`schemaVerified = true`）。
4. **Slice L3 只读 REA 停止、审批/问答与模型/思考档位契约（固定白名单，绝不开放任意 RPC）**：
   - **停止当前调用（`antigravity-stop-v1`）**：经只读 REA 核实 `/exa.language_server_pb.LanguageServerService/CancelCascadeInvocation`、`CancelCascadeInvocationRequest`（`cascade_id=1`, `kill_background_tasks=2`, `notify_parent=3`）与 `CancelCascadeStepsRequest`，受 `x-codeium-csrf-token` 保护（`stopSchemaVerified = true`、`supportsStopTurn = true`）。桥接层 `stop` 操作强制核验 `selected == target` 及可选 `expectedInvocationNum` / `expectedExecutionNum`：迟到或轮次不匹配的 `stop` 返回 `noop`（`guard.stopTargetChanged`），不误停新轮次或另一会话；运行中目标会话确认取消后转为 `completed`（`terminationReason == "USER_CANCELED"`，`fullyIdle == true`）。
   - **请求审批与用户提问回答（`antigravity-interaction-v1`）**：经只读 REA 核实 `/exa.language_server_pb.LanguageServerService/HandleCascadeUserInteraction`、`HandleCascadeUserInteractionRequest`（`cascade_id=1`, `interaction=2`）与 `CascadeUserInteraction`（`trajectory_id=1`, `step_index=2`, `run_command=5`, `file_permission=19`, `permission=21`, `ask_question=22`, `approval_interaction=23`, `selected_option_ids=4`），配合 Lifecycle Hooks 双重门控（`approvalSchemaVerified = true`、`supportsApprovalGate = true`）。桥接层 `resolve` 操作按 `(interactionId, interactionType)` 严格区分权限审批（`runCommand` / `filePermission` / `permission` / `approvalInteraction`）与用户提问（`askQuestion`）：禁止将 `askQuestion` 当作权限放行（`guard.questionCannotApproveAsPermission` 返回 `unconfirmed`）；已过期或已处理的交互返回 `noop`（`guard.interactionAlreadyResolved`），重复 `requestId` 幂等返回 `duplicate = true`，零误批、零跨会话串台。
   - **模型与思考档位切换（`antigravity-model-config-v1`）**：经只读 REA 核实 `/exa.language_server_pb.LanguageServerService/GetCascadeModelConfigs`（`GetCascadeModelConfigsRequest` / `GetCascadeModelConfigsResponse.client_model_configs=1` / `ClientModelConfig` / `ModelOrAlias.model_or_alias=2` / `supported_thinking_levels=9`）与 `CascadeConfig.planner_config=1` / `CascadePlannerConfig`（`requested_model=13`, `thinking_level=19`：`THINKING_LEVEL_LOW` / `THINKING_LEVEL_MEDIUM` / `THINKING_LEVEL_HIGH`）（`modelConfigSchemaVerified = true`、`supportsModelConfig = true`）。桥接层与客户端在发送前核验目标模型 `supportedThinkingLevels`，不支持的思考档位在本地或桥接层直接拒绝（`guard.unsupportedThinkingLevel`，零底层变更）；切换后必须经 `(modelName, thinkingLevel)` 回读一致才返回 `modelSwitched`（不一致返回 `guard.modelConfigReadbackMismatch`）。
   - **不开放任意 RPC 与升级/鉴权失效关闭**：桥仅允许固定白名单操作（`subscribe`、`read`、`open`、`create`、`send`、`query`、`stop`、`resolve`、`switch-model`、`close`），保持 `arbitraryRpcExposed == false`；任何未列入白名单的 RPC 请求立即拒绝（`denied`，`error.unsupportedOperation`），且当安装包升级导致 schema 校验失效或 `csrfAuthVerified == false` 时立即失败关闭（`denied`）。
5. **Official Lifecycle Hooks (`hooks.json`)**：
   - 支持六个事件：`SessionStart`、`PreInvocation`、`PostInvocation`、`PreToolUse`、`PostToolUse`、`Stop`。
   - 标准输入 JSON 含 `conversationId`、`workspacePaths`、`transcriptPath`、`artifactDirectoryPath`、`modelName`、`lastUserInput` 及事件字段（`invocationNum`、`stepIdx`、`toolCall`、`executionNum`、`terminationReason`、`error`、`fullyIdle`）。

## 状态与导航桥不变量及隐私边界

- **两阶段边界剥离（零读取 `transcriptPath` 与 Token）**：`hooks-bridge.py catalog` 与 `hooks-bridge.py emit` 从 `stdin` 读取原始 JSON 后，立即在内存中丢弃 `transcriptPath`、`artifactDirectoryPath`、`lastUserInput`、`toolCall.args`、`csrfToken`、`oauthToken`、`sidecarUiToken` 与 `apiKey`，绝不读取用户 Token、Cookie、状态库或 `transcriptPath` 文件（`transcriptReads == 0`、`tokenReads == 0`）。
- **复合主键与同名跨工作区隔离**：会话身份固定为 `AntigravitySessionKey(conversationId, workspacePath)`，并校验 `workspacePath` 属于 `allowedWorkspaces`；两个不同工作区下的同名会话（相同 `title` / `summary`）互不覆盖、互不串台。
- **精确聚焦与 UI 选中回读确认**：`open` 请求必须携带 `schemaVersion: "antigravity-catalog-nav-v1"` 与完整目标 `(conversationId, workspacePath)`；仅当回读的 `selected` 与请求目标 `(conversationId, workspacePath)` 完全一致时才返回 `.confirmed`（写入 `antigravity_open` `confirmed` 审计回执）；若 UI 选中回读不一致或目标已删除，返回 `.unconfirmed`，绝不重定向到同名跨工作区会话。
- **`agentapi` 创建/发送目标门控与超时对账不重发（Slice K3）**：`create`、`send`、`query` 请求必须携带 `schemaVersion: "antigravity-agentapi-v1"` 与 `requestId`；提示词与消息正文在桥接层校验后立即在内存丢弃，绝不落盘、绝不回显，审计日志 `audit.jsonl` 保持 `text_byte_len == nil`。`send` 强制要求 `selected == target`（两会话仅修改当前选中目标）；不存在或已删除目标返回 `.unconfirmed`（`guard.targetNotFound`），零重定向、零新建会话；超时或 `unknown` 回执锁定原 `requestId` 并通过 `query` 对账，绝不盲目重发。
- **缺 Schema 或合法认证失败关闭**：当 `schemaVerified == false`、`schemaVersion` 不匹配、缺少合法 capability token / `subscriptionId`、工作区越权或 `instanceId` / `generation` 不一致时，立即失败关闭（`.denied` / `.stale`），零状态污染。
- **重复事件去重与 `Stop(fullyIdle=false)` 语义**：重复 Hook 事件不推进 `cursor`、不递增 `transitionCount`；`Stop(fullyIdle=false)` 归入 `backgroundActive`（`后台仍在运行`），仅 `fullyIdle == true` 且 `terminationReason == "NO_TOOL_CALL"` 才归入 `completed`。
- **断流标 `stale`**：Hook 流断开、心跳超时或实例换代时，桥返回 `status: "stale"`，客户端清空旧会话行并标记过期。

## 验证入口

```sh
scripts/verify-antigravity-installed-static --output .scratch/antigravity-status/installed-static.json
scripts/verify-antigravity-paired
```
