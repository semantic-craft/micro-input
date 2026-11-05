# Antigravity 官方桌面扩展契约与状态事件桥

本目录记录安装版 `/Applications/Antigravity.app`（`com.google.antigravity` `2.29.0`，`language_server` `Built at CL: 1013060670`）只读静态核对结果，并提供在隔离测试工作区接收官方 Lifecycle Hooks 元数据事件的 Unix 域套接字桥 `hooks-bridge.py`。

## 安装版官方接口与能力分区

`scripts/verify-antigravity-installed-static` 在只读模式下核对安装版包体、ASAR 头部文件完整性哈希、`language_server --stamp`、`agentapi --help` 与 `~/.gemini/antigravity/lib/node_modules/sidecar_sdk`：

1. **包体与核心二进制**：
   - Bundle ID：`com.google.antigravity`，版本 `2.29.0`
   - `Contents/Resources/app.asar` SHA-256：`290e1b7a1caa430a0c5367c7b433e8d284e2ca5d0b305e91e5ce8ec926f43283`
   - `Contents/Resources/bin/language_server` SHA-256：`fb6e0fc3a1e091e773a3e91ce3e5a4dc93a5f0d30bf305a1ddd70008a2a26a74`（`Built at CL: 1013060670`）
2. **`agentapi` CLI**：
   - 仅暴露 3 个官方子命令：`get-conversation-metadata <conversation_id>`、`new-conversation [--model=...] [--title=...] [--profile=...] <prompt>`、`send-message [--title=...] <recipient_id> <content>`。
   - **缺失能力**：不提供会话全量列表（`supportsSessionCatalog = false`）、精确窗口/标签页聚焦（`supportsTargetFocus = false`）、停止当前轮次（`supportsStopTurn = false`）或审批放行（`supportsApprovalGate = false`）。
3. **Official Sidecars (`sidecar_sdk` `1.0.0`)**：
   - 仅绑定 `127.0.0.1`，要求宿主注入 `ANTIGRAVITY_LS_ADDRESS`、`ANTIGRAVITY_CSRF_TOKEN`、`ANTIGRAVITY_SIDECAR_WEB_PORT` 及 `X-Sidecar-Token`（`ANTIGRAVITY_SIDECAR_UI_TOKEN`），暴露 `/_sidecar/get-conversation-metadata`、`/_sidecar/new-conversation`、`/_sidecar/send-message`。
4. **Official Lifecycle Hooks (`hooks.json`)**：
   - 支持六个事件：`SessionStart`、`PreInvocation`、`PostInvocation`、`PreToolUse`、`PostToolUse`、`Stop`。
   - 标准输入 JSON 含 `conversationId`、`workspacePaths`、`transcriptPath`、`artifactDirectoryPath`、`modelName`、`lastUserInput` 及事件字段（`invocationNum`、`stepIdx`、`toolCall`、`executionNum`、`terminationReason`、`error`、`fullyIdle`）。

## 状态桥不变量与隐私边界

- **两阶段边界剥离（不读 `transcriptPath`）**：`hooks-bridge.py emit` 从 `stdin` 读取原始 Hook JSON 后，立即在内存中丢弃 `transcriptPath`、`artifactDirectoryPath`、`lastUserInput` 与 `toolCall.args`，绝不 `open` 或 `stat` `transcriptPath`，也不将这些字段写入套接字。守护进程与 Swift 客户端同时对出现禁用键名的报文执行拒绝（`transcriptReads == 0`）。
- **两会话隔离**：会话主键固定为 `(conversationId, workspacePath)`，并校验 `workspacePath` 属于 `allowedWorkspaces`。
- **重复事件去重**：按标准键 `conversationId:workspacePath:eventType:invocationNum:stepIdx:executionNum`（或显式 `eventId`）去重；重复事件不推进 `cursor`、不追加 `changes`、不递增 `transitionCount`。
- **`Stop` `fullyIdle=false` 不报完成**：当 `Stop` 事件的 `fullyIdle == false` 时，表示仍有后台任务或子代理运行，状态归入 `backgroundActive`（中文状态：`后台仍在运行`），绝不报告为 `completed`。仅当 `fullyIdle == true` 且 `terminationReason == "NO_TOOL_CALL"` 时才归入 `completed`。
- **断流标 `stale`**：Hook 流断开、心跳超时或实例换代时，桥返回 `status: "stale"`，客户端清空旧会话行并标记过期。

## 验证入口

```sh
scripts/verify-antigravity-installed-static
scripts/verify-antigravity-paired
```
