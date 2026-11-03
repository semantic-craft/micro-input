# 三大 Coding Agent 预设与切换 (`agent-presets`)

一键在本地草稿中安装 **Antigravity**、**Herdr**、**DeepSeek Coder (DSH)** 三套 Coding Agent 预设，在每层 20 个槽位（`logical-0`–`logical-19`）上绑定 19 个有效动作槽：覆盖 12 个实体键帽（`logical-0`–`logical-10` 与 `logical-12`；`logical-11` 作为 2U 宽键副轴与 `logical-10` 对称回退）、左上旋钮 3 槽（`logical-13` 逆时针、`logical-14` 顺时针、`logical-15` `ENC_CLK` 按压）及右上摇杆 4 扇区（`logical-16`–`logical-19`）。

## Sub-features

- `AP-01`：`install_agent_starter_profiles` 以单次原子修订自动为每个预设层开启 `logical_sectors`（`active_sectors: 4`）并绑定 `AG00`–`AG18` / `ACT06`–`12` / `ENC_CC`/`CW`/`CLK` 对应的 19 个动作槽。
- `AP-02`：Antigravity 预设使用标准 `OpenApp`、`Text`、`Shortcut` 动作；Herdr 与 DSH 预设使用 `FocusSession`、`Text` 与 `AgentCommand` 动作。
- `AP-03`：菜单栏在三套预设间切换时，同一物理按键（如 `AG01` / `logical-0`、宽键 `AG10` / `ACT10` / `logical-10` 或旋钮按压 `ENC_CLK` / `logical-15`）立即路由到当前激活 Agent 的目标动作。

## How to get to it (user POV)

- 在菜单栏面板或工作台「预设与交换」菜单中点击「安装 Coding Agent 预设」，随后在菜单栏点击 `Antigravity`、`Herdr` 或 `DeepSeek Coder (DSH)`。

## Driving it with `verify-micro-input`

1. 运行 `swift test --package-path app/MicroInput -Xswiftc -DMI_SYNTHETIC_BACKEND --filter HostExecutionTests`。
2. 运行 `MI_SYNTHETIC_SCENARIO=host-execution scripts/run --verify --timeout=15`。
3. 检查 `$QA_RUN_DIR/evidence/audit.jsonl` 中依次出现三套 Profile ID 下的 **9 条** 动作派发记录（包含 `AG15` 摇杆扇区映射到的 `logical-16` 动作）。

## Gotchas

- `create_layer` 默认仅分配 `logical-0`..`logical-15` 共 16 个槽位；预设安装或自定义配置必须先调用 `set_control_settings`（或在工作台点击「启用摇杆四向扇区（J1–J4）」）将 `joystick_mode` 设为 `"logical_sectors"` 且 `active_sectors` 设为 `4`，否则 `AG15`–`AG18`（`logical-16`..`logical-19`）不会触发或绑定。
