# Antigravity 官方桌面扩展、既有会话目录、精确导航、agentapi 创建/发送与 L3 请求/模型控制

菜单栏的「Antigravity 状态连接」默认关闭。开启后，Micro Input 读取私有 `0600` 授权文件并通过私有 Unix 域套接字订阅 `integrations/antigravity/hooks-bridge.py` 归并的既有会话目录与官方 Lifecycle Hooks 元数据事件，支持按 `(conversationId, workspacePath)` 精确聚焦并读回确认 `selected`、通过 `antigravity-agentapi-v1` 契约执行绑定 `projectId` 的 `new-conversation` 创建与绑定 `conversationId` 的 `send-message` 发送，以及通过经只读 REA 核实的固定白名单契约（`antigravity-stop-v1`、`antigravity-interaction-v1`、`antigravity-model-config-v1`）执行当前选中会话停止、待处理交互审批/提问回答和模型/思考档位切换（绝不开放任意 RPC）。

## 隔离应用检查

```sh
MI_AUTO_HOST_READY=0 scripts/verify-offline
```

总入口包括 `antigravity-status`。单独重跑时先构建合成应用，再运行：

```sh
scripts/build --synthetic
MI_AUTO_HOST_READY=0 MI_SYNTHETIC_SCENARIO=antigravity-status scripts/run --verify --timeout=45
```

读取启动器打印的 `QA_RUN_DIR` 下 `evidence/launcher.json`、`evidence/audit.jsonl`、`antigravity-status.json`、`antigravity-navigation.json`、`antigravity-agentapi.json` 与 `antigravity-l3-control.json`。总入口核验 26 项状态、精确导航、`agentapi` 创建/发送与 L3 请求/模型控制断言（版本与能力分区记录、未启用零读取、两个既有测试会话目录身份与 UI 选中回读一致、同名跨工作区不混、错误选中回读标 `unconfirmed`、删除目标不重定向、缺 Schema/合法认证失败关闭、L3 Schema 或 CSRF 鉴权失效关闭、两会话隔离、重复事件去重、`Stop` `fullyIdle=false` 归入 `backgroundActive` 不报完成、`Stop` `fullyIdle=true` 归入 `completed`、创建绑定 `projectId` 并确认选中回读、两会话发送仅修改当前选中目标、运行中会话消息默认归入 `nextInvocation` 排队与显式 `steered` 单独验证、不存在/已删除目标零重定向零新建、超时/未知回执锁定 `requestId` 对账不重发、停止仅作用于当前选中会话且迟到轮次返回 `noop`、交互审批按类型拦截问题误批且过期返回 `noop`、模型/思考档位切换前置拦截不支持档位并回读确认、断流标 `stale`、换代标 `stale`、日志缺口标 `gap`、零 `transcriptPath` 读取、零 Token 读取、退出关闭订阅）及 `audit.jsonl` 中的 `antigravity_open`、`antigravity_create`、`antigravity_send`、`antigravity_query`、`antigravity_stop`、`antigravity_resolve`、`antigravity_switch_model` 回执（`text_byte_len == nil`），并确认 QA 进程与临时目录已清理。

## 安装版只读契约与真实 UDS 双端检查

```sh
scripts/verify-antigravity-installed-static --output .scratch/antigravity-status/installed-static.json
scripts/verify-antigravity-paired
```

`verify-antigravity-installed-static` 在只读模式下核对 `/Applications/Antigravity.app`（`2.29.0`，`language_server` `CL 1013060670`）、`agentapi`（`new-conversation`、`send-message`、`ANTIGRAVITY_PROJECT_ID`、`MessageDeliveryStrategy` 及 CLI 无显式 `--steer` 标志）、`sidecar_sdk`（`1.0.0`）及会话目录/工作区与 L3 停止（`CancelCascadeInvocationRequest`）、交互审批（`HandleCascadeUserInteractionRequest` / `CascadeUserInteraction`）、模型/思考档位（`GetCascadeModelConfigs` / `CascadeConfig` / `CascadePlannerConfig` / `ThinkingLevel` / `supported_thinking_levels`）protobuf 符号契约与 `x-codeium-csrf-token` 鉴权要求。`verify-antigravity-paired` 启动真实 `hooks-bridge.py` 守护进程、注入带 `0000` 权限金丝雀 `transcriptPath` 与 `canary-token.json` 的 `hooks-bridge.py catalog` / `emit` 子进程，并运行 `AntigravityPairedTests` 验证真实 UDS 传输下的目录身份、同名跨工作区隔离、精确聚焦回读确认、`projectId` 绑定创建、`conversationId` 绑定发送、运行中 `nextInvocation` vs `steered` 单独验证、超时不重发对账、两会话隔离停止与迟到轮次 `noop`、问题/权限类型门控与过期交互 `noop`、模型/思考档位 `supportedThinkingLevels` 门控与回读确认、任意 RPC 拒绝（`error.unsupportedOperation`）及升级/鉴权失效关闭。安装版桌面 GUI 与实体按键单列 `NOT_RUN`。
