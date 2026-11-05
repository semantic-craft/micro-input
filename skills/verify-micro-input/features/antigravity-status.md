# Antigravity 官方桌面扩展与状态事件

菜单栏的「Antigravity 状态连接」默认关闭。开启后，Micro Input 读取私有 `0600` 授权文件并通过私有 Unix 域套接字订阅 `integrations/antigravity/hooks-bridge.py` 归并的官方 Lifecycle Hooks 元数据事件。

## 隔离应用检查

```sh
MI_AUTO_HOST_READY=0 scripts/verify-offline
```

总入口包括 `antigravity-status`。单独重跑时先构建合成应用，再运行：

```sh
scripts/build --synthetic
MI_AUTO_HOST_READY=0 MI_SYNTHETIC_SCENARIO=antigravity-status scripts/run --verify --timeout=45
```

读取启动器打印的 `QA_RUN_DIR` 下 `evidence/launcher.json` 与 `antigravity-status.json`。总入口核验 11 项状态断言（版本与能力分区记录、未启用零读取、两会话隔离、重复事件去重、`Stop` `fullyIdle=false` 归入 `backgroundActive` 不报完成、`Stop` `fullyIdle=true` 归入 `completed`、断流标 `stale`、换代标 `stale`、日志缺口标 `gap`、零 `transcriptPath` 读取、退出关闭订阅），并确认 QA 进程与临时目录已清理。

## 安装版只读契约与真实 UDS 双端检查

```sh
scripts/verify-antigravity-installed-static
scripts/verify-antigravity-paired
```

`verify-antigravity-installed-static` 在只读模式下核对 `/Applications/Antigravity.app`（`2.29.0`，`language_server` `CL 1013060670`）、`agentapi` 与 `sidecar_sdk`（`1.0.0`）。`verify-antigravity-paired` 启动真实 `hooks-bridge.py` 守护进程、注入带 `0000` 权限金丝雀 `transcriptPath` 的 `hooks-bridge.py emit` 子进程，并运行 `AntigravityPairedTests` 验证真实 UDS 传输。安装版桌面 GUI 与实体按键单列 `NOT_RUN`。
