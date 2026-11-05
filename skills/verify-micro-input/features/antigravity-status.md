# Antigravity 官方桌面扩展、既有会话目录、精确导航与 agentapi 创建/发送

菜单栏的「Antigravity 状态连接」默认关闭。开启后，Micro Input 读取私有 `0600` 授权文件并通过私有 Unix 域套接字订阅 `integrations/antigravity/hooks-bridge.py` 归并的既有会话目录与官方 Lifecycle Hooks 元数据事件，支持按 `(conversationId, workspacePath)` 精确聚焦并读回确认 `selected`，以及通过 `antigravity-agentapi-v1` 契约执行绑定 `projectId` 的 `new-conversation` 创建、绑定 `conversationId` 的 `send-message` 发送和 `requestId` 超时对账查询。

## 隔离应用检查

```sh
MI_AUTO_HOST_READY=0 scripts/verify-offline
```

总入口包括 `antigravity-status`。单独重跑时先构建合成应用，再运行：

```sh
scripts/build --synthetic
MI_AUTO_HOST_READY=0 MI_SYNTHETIC_SCENARIO=antigravity-status scripts/run --verify --timeout=45
```

读取启动器打印的 `QA_RUN_DIR` 下 `evidence/launcher.json`、`evidence/audit.jsonl`、`antigravity-status.json`、`antigravity-navigation.json` 与 `antigravity-agentapi.json`。总入口核验 22 项状态、精确导航与 `agentapi` 创建/发送断言（版本与能力分区记录、未启用零读取、两个既有测试会话目录身份与 UI 选中回读一致、同名跨工作区不混、错误选中回读标 `unconfirmed`、删除目标不重定向、缺 Schema/合法认证失败关闭、两会话隔离、重复事件去重、`Stop` `fullyIdle=false` 归入 `backgroundActive` 不报完成、`Stop` `fullyIdle=true` 归入 `completed`、创建绑定 `projectId` 并确认选中回读、两会话发送仅修改当前选中目标、运行中会话消息默认归入 `nextInvocation` 排队与显式 `steered` 单独验证、不存在/已删除目标零重定向零新建、超时/未知回执锁定 `requestId` 对账不重发、断流标 `stale`、换代标 `stale`、日志缺口标 `gap`、零 `transcriptPath` 读取、零 Token 读取、退出关闭订阅）及 `audit.jsonl` 中的 `antigravity_open`、`antigravity_create`、`antigravity_send`、`antigravity_query` 回执（`text_byte_len == nil`），并确认 QA 进程与临时目录已清理。

## 安装版只读契约与真实 UDS 双端检查

```sh
scripts/verify-antigravity-installed-static --output .scratch/antigravity-status/installed-static.json
scripts/verify-antigravity-paired
```

`verify-antigravity-installed-static` 在只读模式下核对 `/Applications/Antigravity.app`（`2.29.0`，`language_server` `CL 1013060670`）、`agentapi`（`new-conversation`、`send-message`、`ANTIGRAVITY_PROJECT_ID`、`MessageDeliveryStrategy` 及 CLI 无显式 `--steer` 标志）、`sidecar_sdk`（`1.0.0`）及会话目录/工作区 protobuf 符号契约。`verify-antigravity-paired` 启动真实 `hooks-bridge.py` 守护进程、注入带 `0000` 权限金丝雀 `transcriptPath` 与 `canary-token.json` 的 `hooks-bridge.py catalog` / `emit` 子进程，并运行 `AntigravityPairedTests` 验证真实 UDS 传输下的目录身份、同名跨工作区隔离、精确聚焦回读确认、`projectId` 绑定创建、`conversationId` 绑定发送、运行中 `nextInvocation` vs `steered` 单独验证、超时不重发对账与失败关闭。安装版桌面 GUI 与实体按键单列 `NOT_RUN`。
