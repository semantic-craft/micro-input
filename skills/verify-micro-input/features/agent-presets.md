# 桌面 Agent 预设与切换

四套独立 Agent Profile（Antigravity、Herdr、Codex、DeepSeek Harness）与「其他软件」通用预设。具体按键与能力差异以 README「桌面 Agent 常用控制」和 docs/verification/desktop-agent-controls.md 为准。

1. `scripts/check` 验证迁移原子性、重复安装无变化、保留用户改动、目标应用绑定和输入路由。
2. `scripts/build --synthetic` 后运行 `MI_SYNTHETIC_SCENARIO=host-execution scripts/run --verify --timeout=25`。
3. 检查 20 条 simulated 回执，覆盖 Antigravity、DSH、Herdr 与「其他软件」四个非受保护桌面 Profile；Herdr 的 AG10/AG11 组合只派发一次，AG15 派发摇杆动作。
4. `scripts/build` 恢复生产后端，用“试运行此动作”核对真实应用。必须另读原生 UI 的结果，不能仅凭 AXPress 成功或回执认定菜单/录音已生效。
5. 安装版单独核对权限、菜单栏显示和实际动作；QA 权限不代表安装版权限。

不能把 Herdr TUI 或 DSH CLI 的占位命令当作桌面 App 控制。新建 Herdr Space/Tab/分屏的验收只在本次创建的独立 Space 中做，结束后恢复原 Space 并清理本次产物。
