# 自动跟随与 HUD 验证

范围与切换契约见 [S5 设计](../designs/app-following-and-hud.md)。当前实现只控制宿主 Profile；没有径向菜单、全局快捷键或设备写入。最新已执行证据摘要见 [E-100](../spec/EVIDENCE.md)。

## 自动检查

运行 `scripts/check`，覆盖 Rust 模型/会话/FFI、合成与生产 Swift 测试、两种 ABI 构建及只读边界。不要把只通过文档或构建当作原生验收。

重点回归在 `HostRoutingTests`、`storage::routing::tests` 与 `mi-session::tests`：

- reader 已接收但 decoder 未消费的 north ≤ cutoff；切换后 east > cutoff 仍被阻断；实际 center 后 fresh east 才可触发。
- 已提交的 B 切换完成后，未关联 C 保留 B；手动选择优先于等待的自动观察。
- 同 Profile 手动选择也保持；重启保留暂停；显式缺失 ID 不退回默认。
- FIFO 最多 32 项；等待项过期/文档变化后丢弃；正在执行项保留原归属；设备、试运行和打开应用串行。
- 广播缺口后明确恢复输入；新订阅先观察释放/回中，再接受相应控制。

原生组件用例 `testNativeNoticeTimerAndSheetPreserveForeground` 创建真正的 AppKit 面板，核对 1.5 秒计时、不成为 key window、frontmost PID 保持，以及当前每块显示器的 visibleFrame 边界。它不证明完整的跨应用全屏 Space 叠放。

## 原生应用验收

沿用 [verify-micro-input](../../skills/verify-micro-input/SKILL.md) 的隔离启动器：先构建，再运行 `scripts/run --verify`；按它打印的 QA_APP_PATH 精确选择应用，回读 launcher.json，确认工作台目录与日常应用隔离。

1. 在“应用关联…”中用原生选择器关联 Antigravity；打开自动跟随并恢复跟随。保持按键控制暂停，用工作台“打开应用”真正激活 Antigravity，回读偏好确认匹配且暂停保留。仅从后台读取/点击某个应用的 AX 树不一定触发系统前台切换。
2. 打开未关联的应用，确认实际 Profile 保持且没有进入手动保持。手动选择 Herdr，再打开 Antigravity，确认不覆盖；恢复跟随后再次进入 Antigravity，确认重新匹配。
3. 实际截图检查菜单四个常用按钮和新增入口完整可见；打开速查，确认显示宿主配置而非正在浏览的配置、首层映射、一个合并宽键、不可用项及暂停状态。速查没有执行按钮，也不显示文本/宏内容。
4. 关闭主工作台，检查观察与速查仍存在；检查速查关闭按钮、关联编辑器完成按钮、提示计时、焦点保持。分别在多显示器与全屏 Space 检查可见区域和叠放，不用 flags 断言代替实际 Space 验收。
5. 重启、睡眠/唤醒及设备断开分别记录。硬件按压需要实际 HID 回执，不能用 UI 试运行或合成事件替代。

证据写入本地 QA evidence 或项目外产物目录，保留 launcher.json、脱敏 audit.jsonl 和按范围标记的 result.json；确认 QA PID、临时 app、文档和偏好均已清理。QA 身份不替代日常安装版的签名、辅助功能权限和用户验收。
