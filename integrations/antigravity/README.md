# Antigravity 官方桌面扩展契约、既有会话目录与状态导航桥

本目录记录安装版 `/Applications/Antigravity.app`（`com.google.antigravity` `2.29.0`，`language_server` `Built at CL: 1013060670`）只读静态与 REA 协议核对结果，并提供在隔离测试工作区接收既有会话目录、执行精确会话聚焦回读确认与接收官方 Lifecycle Hooks 元数据事件的 Unix 域套接字桥 `hooks-bridge.py`。

## 安装版官方接口、只读 REA 契约与能力分区

`scripts/verify-antigravity-installed-static` 在只读模式下核对安装版包体、ASAR 头部文件完整性哈希、`language_server --stamp`、只读 protobuf 符号契约、`agentapi --help` 与 `~/.gemini/antigravity/lib/node_modules/sidecar_sdk`：

1. **包体与核心二进制**：
   - Bundle ID：`com.google.antigravity`，版本 `2.29.0`
   - `Contents/Resources/app.asar` SHA-256：`290e1b7a1caa430a0c5367c7b433e8d284e2ca5d0b305e91e5ce8ec926f43283`
   - `Contents/Resources/bin/language_server` SHA-256：`fb6e0fc3a1e091e773a3e91ce3e5a4dc93a5f0d30bf305a1ddd70008a2a26a74`（`Built at CL: 1013060670`）
2. **`agentapi` CLI**：
   - 仅暴露 3 个官方子命令：`get-conversation-metadata <conversation_id>`、`new-conversation [--model=...] [--title=...] [--profile=...] <prompt>`、`send-message [--title=...] <recipient_id> <content>`。
   - **CLI 限制**：独立 CLI 不提供既有会话全量枚举、窗口聚焦、停止当前轮次（`supportsStopTurn = false`）或审批放行（`supportsApprovalGate = false`）。
3. **Official Sidecars (`sidecar_sdk` `1.0.0`) 与只读 REA 会话目录/聚焦契约**：
   - 官方 Sidecar 仅绑定 `127.0.0.1`，要求宿主注入 `ANTIGRAVITY_LS_ADDRESS`、`ANTIGRAVITY_CSRF_TOKEN`、`ANTIGRAVITY_SIDECAR_WEB_PORT` 及 `X-Sidecar-Token`（`ANTIGRAVITY_SIDECAR_UI_TOKEN`），暴露 `/_sidecar/get-conversation-metadata`、`/_sidecar/new-conversation`、`/_sidecar/send-message`。
   - `preload.js` 通过 `window.addEventListener('message', ...)` 接收宿主推送的 `workspace-change`（`payload.workspaceUris`），并通过 `window.sidecar.ui.toggleConversation(conversationId)` 向父窗口发送 `postMessage({ type: 'toggle-conversation', payload: { conversationId } }, '*')`。
   - 对 `language_server`（`CL 1013060670`）的只读 REA 确认：`LanguageServerService/GetConversationMetadata`（`GetConversationMetadataRequest` / `GetConversationMetadataResponse`）与 `GetAllCascadeTrajectories`（`CascadeTrajectorySummary` + `CortexTrajectoryMetadata` + `CortexWorkspaceMetadata.workspace_folder_absolute_uri`）在 `x-codeium-csrf-token` 保护下将会话身份 `(conversation_id, workspace_folder_absolute_uri)` 与 `summary` / `title`、`project_id` 绑定。
   - 经上述契约核对后，状态与导航桥支持既有会话目录与精确导航（`supportsSessionCatalog = true`、`supportsTargetFocus = true`、`schemaVerified = true`）。
4. **Official Lifecycle Hooks (`hooks.json`)**：
   - 支持六个事件：`SessionStart`、`PreInvocation`、`PostInvocation`、`PreToolUse`、`PostToolUse`、`Stop`。
   - 标准输入 JSON 含 `conversationId`、`workspacePaths`、`transcriptPath`、`artifactDirectoryPath`、`modelName`、`lastUserInput` 及事件字段（`invocationNum`、`stepIdx`、`toolCall`、`executionNum`、`terminationReason`、`error`、`fullyIdle`）。

## 状态与导航桥不变量及隐私边界

- **两阶段边界剥离（零读取 `transcriptPath` 与 Token）**：`hooks-bridge.py catalog` 与 `hooks-bridge.py emit` 从 `stdin` 读取原始 JSON 后，立即在内存中丢弃 `transcriptPath`、`artifactDirectoryPath`、`lastUserInput`、`toolCall.args`、`csrfToken`、`oauthToken`、`sidecarUiToken` 与 `apiKey`，绝不读取用户 Token、Cookie、状态库或 `transcriptPath` 文件（`transcriptReads == 0`、`tokenReads == 0`）。
- **复合主键与同名跨工作区隔离**：会话身份固定为 `AntigravitySessionKey(conversationId, workspacePath)`，并校验 `workspacePath` 属于 `allowedWorkspaces`；两个不同工作区下的同名会话（相同 `title` / `summary`）互不覆盖、互不串台。
- **精确聚焦与 UI 选中回读确认**：`open` 请求必须携带 `schemaVersion: "antigravity-catalog-nav-v1"` 与完整目标 `(conversationId, workspacePath)`；仅当回读的 `selected` 与请求目标 `(conversationId, workspacePath)` 完全一致时才返回 `.confirmed`（写入 `antigravity_open` `confirmed` 审计回执）；若 UI 选中回读不一致或目标已删除，返回 `.unconfirmed`，绝不重定向到同名跨工作区会话。
- **缺 Schema 或合法认证失败关闭**：当 `schemaVerified == false`、`schemaVersion` 不匹配、缺少合法 capability token / `subscriptionId`、工作区越权或 `instanceId` / `generation` 不一致时，立即失败关闭（`.denied` / `.stale`），零状态污染。
- **重复事件去重与 `Stop(fullyIdle=false)` 语义**：重复 Hook 事件不推进 `cursor`、不递增 `transitionCount`；`Stop(fullyIdle=false)` 归入 `backgroundActive`（`后台仍在运行`），仅 `fullyIdle == true` 且 `terminationReason == "NO_TOOL_CALL"` 才归入 `completed`。
- **断流标 `stale`**：Hook 流断开、心跳超时或实例换代时，桥返回 `status: "stale"`，客户端清空旧会话行并标记过期。

## 验证入口

```sh
scripts/verify-antigravity-installed-static --output .scratch/antigravity-status/installed-static.json
scripts/verify-antigravity-paired
```
