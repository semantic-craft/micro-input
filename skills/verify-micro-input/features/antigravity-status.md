# Antigravity 官方桌面扩展、既有会话目录、精确导航、agentapi 创建/发送与 L3 请求/模型控制

`antigravity-status` 验证 `mi-bridge` 的合成状态、导航及命令协议。旧 Python live bridge 已退役，当前工作台直接通过 Rust 读取 hub 目录并执行原生 AX 导航，见 [当前接口](../../../integrations/antigravity/README.md)。本页合成回执不能证明真实应用选中、发送、停止或模型变更。

真实导航另运行 `mi-core` 的 `native_session_navigation_verifies_catalog_and_selected_url` 忽略测试，设置 `MI_VERIFY_SESSION_ID` 为已存在的测试会话，使用 `--ignored --nocapture`。该测试会实际切换会话；分别覆盖两个目标并保留命令输出。完整验收还须在生产应用中操作工作台和实体键，回读原窗口的会话 URL，核对 `session_navigation` 审计回执。软件注入不能证明实体键路径。

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
