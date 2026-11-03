# Micro Input Blueprint

## System Overview

Micro Input 是 Rust 核心与 SwiftUI 界面组成的 macOS 配置器。此文件提供代码契约索引；手写 [README](./README.md) 保留产品说明和验收边界。

## Interface

- [Rust workspace](./crates/EGM.md)：配置模型、协议、会话、传输与高层桥接。
- [原生应用](./app/EGM.md)：SwiftUI 工作台与应用生命周期。
- [开发脚本](./scripts/EGM.md)：构建、启动和验证入口。

## Invariants

- 本地草稿、设备快照和活动状态分离；浏览与本地保存不构成设备配置写入。
- 生产会话注入 UnimplementedFramer / UnverifiedSchema，真实协议仍拒绝；未来能力守卫不能当作现有实现。
- 生成桥接目录、测试专用目录、fixtures、隐藏及忽略目录不生成契约；现有 README 不覆盖。

## Side Effects

- 本地文档落盘、原生应用生命周期与受控 HID I/O 分别由对应子模块拥有。宿主操作的具体能力以本分支的 app 契约为准。

## Verification

仓库根目录使用 `scripts/check`，包含 Rust、桥接、Swift 与边界检查；本次文档任务未运行构建、应用或设备操作。EGM bootstrap 对发现的 23 个子目录按预授权执行全树筛选；纯文档/纯测试目录不纳入代码契约。
