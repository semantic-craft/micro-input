# 2U 宽键合并防抖与透传层健康检查 (`wide-key-and-passthrough`)

修正第 0 行硬件反序（`AG01` 在左对应 `logical-0`，`AG00` 在右对应 `logical-1`），合并底部 2U 宽键双微动（`AG10` + `AG11` 及出厂 Codex 层 `ACT10` + `ACT11`）避免单次按压触发两次动作，支持 `v.oai.rad` 摇杆相邻扇区直接滑动切换与 `< 0.35` 归中释放，并在硬件层偏离 `KV_OAI_AG*` / `KV_OAI_ACT*` 透传绑定时给出告警。

## Sub-features

- `WP-01`：`AG01` 映射到 `logical-0`（`row0-col0-left`），`AG00` 映射到 `logical-1`（`row0-col1-right`）。
- `WP-02`：`WideKeyMerger` 在 `AG10` 与 `AG11`（或 `ACT10` 与 `ACT11`）重叠按下时仅发出 1 次 `Pressed`（`binding_id: "AG10"`, `logical_switch: "logical-10"`），全部释放后才发出 1 次 `Released`；`resolve_host_action` 对 `logical-10` 与 `logical-11` 提供双向对称回退；遇到 `InputStreamGap` 时清空按住集合。
- `WP-03`：`check_passthrough_health` 检查当前活动层（在真实 `0.6.3` 固件上按 1-based `layer_index` 映射 `layers[active_layer - 1]`）是否包含全部 13 个 `"KV_OAI_AG00"`..`"KV_OAI_AG12"` 或 `"KV_OAI_ACT00"`..`"KV_OAI_ACT12"` 透传键位绑定，不足时返回 `LayerDriftWarning`。

## How to get to it (user POV)

- 连接键盘并读取身份与配置快照后，在菜单栏面板顶部查看透传层健康状态指示（若处于偏移或未读快照状态，可点击「重新检查身份/层状态」）；按下键盘底部 2U 宽键触发单次审批/执行动作。

## Driving it with `verify-micro-input`

1. 运行 `cargo test --locked -p mi-session -p mi-model` 验证 `WideKeyMerger`、出厂 Codex `ACT06`–`12` / `ENC_*` / `v.oai.rad` 扇区切换状态机与 `check_passthrough_health` 单元测试。
2. 运行 `MI_SYNTHETIC_SCENARIO=host-execution scripts/run --verify --timeout=25`，该场景会连续注入 `AG10` 按下 → `AG11` 按下 → `AG10` 释放 → `AG11` 释放，检查 `$QA_RUN_DIR/evidence/audit.jsonl` 确认宽键动作仅被执行 1 次。

## Gotchas

- `crates/mi-ffi/tests/input_observation.rs` 中的 `CandidateBindings` 测试专门验证未核实候选模式（`subscribe_input` 使用 `CandidateInputDecoder`，不合并 `AG10`/`AG11`，产出 8 条事件）；开启宿主动作执行时使用 `subscribe_verified_input`（内置 `WideKeyMerger` 与 `VerifiedInputDecoder`），两条路径按 `hostExecutionEnabled` 区分。
