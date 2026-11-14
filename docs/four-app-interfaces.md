# 桌面应用接口

支持范围见[桌面应用控制](designs/four-app-control.md)。各入口有不同的能力和验收边界。

| 应用 | 入口和证据 |
| --- | --- |
| Antigravity | [会话目录与原生导航](../integrations/antigravity/README.md)。工作台直接读取 hub 元数据并通过原窗口 AX 回读确认；[Rust 桥](../crates/mi-bridge/src/antigravity.rs)保留独立协议回归。 |
| Herdr | [桌面控制验收](verification/desktop-agent-controls.md)记录本机桌面版的快捷键入口。预设只对 `dev.herdr.app` 派发。 |
| Codex | [原生执行器](../crates/mi-macos/src/executor.rs)和[受保护层测试](../crates/mi-model/tests/protected_oai_layer.rs)。设备层识别与应用快捷键分开。 |
| DeepSeek Harness | [同 Host 桥](../integrations/dsh/README.md)和 [Rust 实现](../crates/mi-bridge/src/dsh.rs)。目录、订阅和命令受授权范围与目标身份限制。 |

[预设生成](../crates/mi-model/src/preset.rs)定义内置按键动作与「其他软件」。[宿主路由](../crates/mi-model/src/host_router.rs)解析物理输入；[工作台状态仓](../crates/mi-core/src/store.rs)连接设备会话、宿主执行和界面投影。

[六层编译器](../crates/mi-model/src/six_layer_board.rs)保存已应用的固定路由；[Karabiner 规则](../crates/mi-core/src/karabiner.rs)和[用户命令接收器](../crates/mi-transport/src/user_command.rs)处理设备专属输入。点击听写、输入聚焦和定向滚动由[宿主控件状态机](../crates/mi-macos/src/managed_controls.rs)通过[原生辅助功能适配](../crates/mi-macos/src/native_ax.rs)执行。

设备输入映射沿用 Input IA-06/10/13/14 的已核对范围。修改应用适配不解锁新协议或设备写入。

## Codex 本机方案

Micro Input 独占键盘的 USB 厂商接口，原生 Codex 键盘插件对同一接口的共享连接会被系统拒绝。全部二十个控制使用现有「Codex · Micro Input」分配，包括前六会话键、下排七键、旋钮和摇杆。选择或锁定原生设备方案也使用这些本机定义；设备原件保护仅限制改写，不保留插件控制权。

「Codex · Micro Input」底部七键依次为实时语音、聚焦输入、新建标签页、内置浏览器 / 布局、新建聊天、点击听写和提交。实时语音、新建标签页（Cmd+T）、内置浏览器 / 布局（Cmd+Shift+B）与新建聊天（Cmd+N）采用[官方快捷键](https://learn.chatgpt.com/docs/reference/commands)。Cmd+N 在当前项目中直接新建对话，Cmd+T 与 Cmd+Shift+B 分别触发新建标签页与内置浏览器 / 工作区布局切换；语音是否可用取决于应用。

聚焦、听写和提交复用宿主控件状态机。听写使用目标窗口内的控件和停止读回；转写尚未完成时不发送。控件文案依据本机桌面包静态资源，合成测试不代表实际 Codex 无障碍树已经验收。

前六键使用 Micro Input 自己的 Codex 会话元数据观察和原窗口选中确认路径，不依赖原生键盘插件的 USB 连接。公开 app-server 的线程状态属于连接的运行实例，不能代替桌面当前会话目录。逐颗实体按键的目标效果仍须单独验收。
