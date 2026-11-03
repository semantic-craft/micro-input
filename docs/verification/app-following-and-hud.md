# S5：自动跟随、切换提示与按键速查验收

2026-11-03；范围依据 [修订设计](../designs/app-following-and-hud.md)。实现包含前台 bundle ID 关联、手动保持、非激活切换提示和只读速查；没有径向菜单、全局热键、新设备 RPC 或 Flash 写入。

## 实现与检查

- Rust 在原工作台偏好所有者中原子保存选择和 override，旧版偏好默认关闭自动跟随。显式缺失 ID 不替换成默认 Profile；启动保留按键暂停。
- 文档编辑、载入、导入、恢复、预设安装与宿主切换共用串行门禁；自动观察合并，手动请求优先。输入持续读取，动作队列最多 32 项、单执行任务、有效期 1 秒。设备输入、试运行和“打开应用”复用此任务。
- reader 在广播前分配 ingress sequence；切换后水位阻断旧输入。摇杆旧通知仍更新物理状态，真正回中后才解除隔离。广播缺口停止订阅，明确“恢复输入”后重新订阅，不自动重连健康设备。
- `scripts/check` 通过：Python 启动器、Rust 工作区、68 项合成 Swift、40 项生产 Swift、两种 ABI 构建与只读/不发送边界。`git diff --check` 通过。
- 关键回归：未读 north ≤ cutoff → east > cutoff 不触发 → 实际 center → fresh east 触发；B 已提交后到达未关联 C 保留 B；手动选择压住自动观察；同 Profile 手动选择持久保持；缺失 ID 无隐式替换；积压丢弃、超时、文档变化失效、执行器串行与原归属；gap 后先松开再触发。
- 原有 `DelayedBackend` 测试夹具存在跨 executor 发布 continuation 的数据竞争，复跑曾在 `resume` 崩溃。用锁同步发布后全量重跑通过；产品任务不采用这一测试用的等待机制。

## 原生证据

使用项目 `verify-micro-input` 流程，所有运行目录均在该工作树的 `.scratch/qa-runs/`，bundle ID 为 `com.semanticcraft.microinput.qa`。日常工作台、已安装应用和真实设备映射未更改。

| 场景 | 实际结果 | 本地证据目录 |
| --- | --- | --- |
| 合成 HID 宿主闭环、应用选择器 | 16 条 simulated 回执；原生选择 Antigravity 并保存 bundle ID 关联 | `1793711832703-98375/evidence/` |
| 生产构建前台跟随、速查 | 暂停按键控制时，经“打开应用”真正激活 Antigravity，自动切换到关联 Profile；暂停保留。显示速查，关闭工作台后速查继续存在，关闭按钮移除面板 | `s5-production-native/evidence/` |
| 生产构建未关联、手动与恢复 | 真正打开未关联 Cindy 保留 Antigravity 且不进入手动保持；手动选 Cindy 后打开 Antigravity 仍保持 Cindy；恢复跟随后再次打开 Antigravity 自动匹配 | `s5-native-final/evidence/` |
| 菜单和映射视图 | 实际截图检查四个常用按钮和新增入口完整可见；速查显示宿主配置、暂停状态、首层映射、一个合并宽键与不可用项；无动作执行按钮 | 本轮原生 UI 工具记录 |
| 原生 AppKit 组件 | 提示 1.5 秒后消失；notice/sheet 不成为 key window，frontmost PID 保持；在本机 Studio Display 与 DELL U2723QE 的 visibleFrame 内实际定位并断言边界 | `HostRoutingTests.testNativeNoticeTimerAndSheetPreserveForeground` |

各运行保留 `launcher.json`、`audit.jsonl`（有动作时）及 `result.json`；退出后确认 QA PID 不存在，临时 app、文档和偏好已清理。一次构建尚未切换完成时启动的 QA 在操作前退出，标为 NOT_RUN，不计入通过证据。

## 验收边界

- 全屏 Space 的实际跨应用叠放仍未逐项视觉验收；现有覆盖是原生非激活行为、两显示器定位和 full-screen auxiliary 配置，不能代替全部 Space 场景。
- 本轮没有真人按压/拨动硬件。摇杆缓冲边界由 reader 合成回归与跨 UniFFI/Swift 边界测试覆盖，不声明新的真实固件协议证据。
- QA 身份不代表日常安装版签名、辅助功能权限或用户验收。没有安装或发布新的日常应用包。
- IA-09/11 仅作为宿主匹配与只读 HUD 的静态研究背景；不实现厂商 AppSense 设备上报或设备 Profile 激活。原 E-098 的 gap 自动重订阅、E-099 的启动自动开启执行由本设计明确替代。
