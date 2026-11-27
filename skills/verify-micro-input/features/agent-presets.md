# 桌面 Agent 预设与切换

四套独立 Agent Profile（Codex、Antigravity、DSH、Grok Bot）与「其他软件」通用预设。具体按键与能力差异以 README 与 docs/four-app-interfaces.md 为准。

1. `scripts/check` 验证迁移原子性、重复安装无变化、保留用户改动、目标应用绑定和输入路由。
2. `scripts/build` 后运行 `MI_SYNTHETIC_SCENARIO=host-execution scripts/run --verify --timeout=25`。
3. 检查 20 条 simulated 回执，覆盖 Antigravity、DSH、Grok Bot 与「其他软件」四个非受保护桌面 Profile；Grok Bot 的 AG10/AG11 组合只派发一次听写切换，AG06 派发语音聊天，AG12 派发发送。
4. 结束合成场景并清除 `MI_SYNTHETIC_SCENARIO` 后，用“试运行此动作”核对真实应用。必须另读原生 UI 的结果，不能仅凭 AXPress 成功或回执认定菜单/录音已生效。
5. 安装版单独核对权限、菜单栏显示和实际动作；QA 权限不代表安装版权限。

不能把 CLI 占位命令当作桌面 App 控制；Grok Bot 仅绑定三个原生 AX 控件（语音聊天、点击听写、听写后发送），其余槽位保持空置。
