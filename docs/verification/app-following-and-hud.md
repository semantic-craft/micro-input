# 手动路由、方案锁定与 HUD 验证

范围与切换契约见 [S5 设计](../designs/app-following-and-hud.md)。方案路由由手动选择、锁定方案或六层板载物理层决定，已退役的前台应用自动切换与 `app_profile_links` 偏好不再触发或持久化；旧版偏好字段在加载时静默丢弃。最新已执行证据摘要见 [E-100](../spec/EVIDENCE.md)。

## 自动检查

运行 `scripts/check`，覆盖 Rust 模型/会话/FFI、合成与生产 Swift 测试、两种 ABI 构建及只读/退役字段边界（含 `R-C11`）。不要把只通过文档或构建当作原生验收。

重点回归在 `storage::routing::tests`、`six_layer_store::tests`、`six_layer_runtime::tests`、`v070_hardware_store::tests` 与 `mi-session::tests`：

- 旧版 `workbench.preferences.json` 含 `auto_follow_enabled`、`manual_override`、`app_profile_links` 时可正常加载并静默丢弃，再次保存后不再写回；新建与正常偏好仍受 `#[serde(deny_unknown_fields)]` 严格校验。
- 六层模式启动及连接就绪后默认处于「六层板载路由」（`BoardLayers`），手动启用具体方案切至「手动选择方案」（`ManualPinned`）并抑制 V070 `kb.pl.set`，开启「锁定当前方案」（`Locked`）固定响应锁定方案；解除锁定或收到非预期硬件层变更后可回到 `BoardLayers`。
- reader 已接收但 decoder 未消费的 north ≤ cutoff；切换后 east > cutoff 仍被阻断；实际 center 后 fresh east 才可触发。
- FIFO 最多 32 项；等待项过期/文档变化后丢弃；正在执行项保留原归属；设备、试运行和打开应用串行；层路由通过 `Activation.layer_override` 逐事件绑定，过期第 1 层激活不会错路由后续第 2 层事件。
- 广播缺口后明确恢复输入；新订阅先观察释放/回中，再接受相应控制。

原生组件用例 `testNativeNoticeTimerAndSheetPreserveForeground` 创建真正的 AppKit 面板，核对 1.5 秒计时、不成为 key window、frontmost PID 保持，以及当前每块显示器的 visibleFrame 边界。它不证明完整的跨应用全屏 Space 叠放。

## 原生应用验收

沿用 [verify-micro-input](../../skills/verify-micro-input/SKILL.md) 的隔离启动器：先构建，再运行 `scripts/run --verify`；按它打印的 QA_APP_PATH 精确选择应用，回读 launcher.json，确认工作台目录与日常应用隔离。

1. 在工作台或菜单栏手动选择方案，并用“打开应用”或系统切换打开不同前台应用，确认宿主方案保持手动选择，不会因前台切换而自动改写；开启“锁定当前方案”后再次切换前台应用并验证暂停/解锁与 `ManualPinned` / `BoardLayers` 恢复一致。
2. 在状态栏与侧栏核对路线摘要（「六层板载路由」/「手动选择方案」/「已锁定 · 忽略前台切换与外部干扰」）；打开“按键速查”，确认显示实际宿主配置而非正在浏览的配置、首层映射、一个合并宽键、不可用项及暂停状态。速查没有执行按钮，也不显示文本/宏内容。
3. 实际截图检查菜单四个常用按钮完整可见；关闭主工作台，检查速查、提示计时、焦点保持，以及多显示器与全屏 Space 的可见区域和叠放，不用 flags 断言代替实际 Space 验收。
4. 重启、睡眠/唤醒及设备断开分别记录。硬件按压需要实际 HID 回执，不能用 UI 试运行或合成事件替代。

证据写入本地 QA evidence 或项目外产物目录，保留 launcher.json、脱敏 audit.jsonl 和按范围标记的 result.json；确认 QA PID、临时 app、文档和偏好均已清理。QA 身份不替代日常安装版的签名、辅助功能权限和用户验收。
