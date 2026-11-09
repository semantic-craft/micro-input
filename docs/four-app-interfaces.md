# 桌面应用接口

支持范围见[桌面应用控制](designs/four-app-control.md)。各入口有不同的能力和验收边界。

| 应用 | 入口和证据 |
| --- | --- |
| Antigravity | [状态桥](../integrations/antigravity/README.md)、[Rust 桥](../crates/mi-bridge/src/antigravity.rs)和原生执行器。状态流只消费目标会话元数据。 |
| Herdr | [桌面控制验收](verification/desktop-agent-controls.md)记录本机桌面版的快捷键入口。预设只对 `dev.herdr.app` 派发。 |
| Codex | [原生执行器](../crates/mi-macos/src/executor.rs)和[受保护层测试](../crates/mi-model/tests/protected_oai_layer.rs)。设备层识别与应用快捷键分开。 |
| Cindy | [适配说明](../integrations/cindy/README.md)、[会话桥](../crates/mi-bridge/src/cindy.rs)和[所有权握手](../crates/mi-bridge/src/ownership.rs)。 |
| DeepSeek Harness | [同 Host 桥](../integrations/dsh/README.md)和 [Rust 实现](../crates/mi-bridge/src/dsh.rs)。目录、订阅和命令受授权范围与目标身份限制。 |

[预设生成](../crates/mi-model/src/preset.rs)定义内置按键动作与「其他软件」。[宿主路由](../crates/mi-model/src/host_router.rs)解析物理输入；[工作台状态仓](../crates/mi-core/src/store.rs)连接设备会话、宿主执行和界面投影。

设备输入映射沿用 Input IA-06/10/13/14 的已核对范围。修改应用适配不解锁新协议或设备写入。
