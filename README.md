# Micro Input

面向 Work Louder Creator Micro 2 的独立 macOS 配置器与宿主服务。技术方向：**Rust 核心 + SwiftUI 界面**。项目名为工作名。

## 当前状态

2026-11-02：已建立正式项目和本地 Git 仓库，迁入前期调研原件；尚未初始化 Cargo/Swift 工程、实现设备通信或构建可运行应用。既有 input.app 和设备配置保持原状。

从 [开发起点](docs/development.md) 开始；操作设备或处理兼容行为前阅读 [项目教训](lessons.md)。Agent 入口见 [AGENTS.md](AGENTS.md)。

## 调研入口

以下为本机原始调查材料，保存在 Git 忽略的 `.scratch/input-study/`，不随源码提交：

- [官方来源、能力矩阵与公开协议线索](.scratch/input-study/out/research.md)
- [已连接 UI 清单](.scratch/input-study/out/connected-ui-inventory.md)
- [灯光、摇杆、层、AppSense 与参数实验](.scratch/input-study/out/layer-lighting-joystick-followup.md)
- [观察规格](.scratch/input-study/out/spec.json)和[证据记录](.scratch/input-study/out/evidence.jsonl)
- [原任务历史](.scratch/input-study/task-history.md)

旧入口 `~/agent-tasks/worklouder-input-study` 仅保留晋升记录和兼容链接。后续项目事实、代码和进度只在本项目维护。

## 下一步

先确定最小可运行范围并初始化 Rust / SwiftUI 工程。建议第一条端到端路径为：发现设备 → 读取身份/配置 → 在 UI 展示，暂不写设备。随后建立可回读、可恢复的配置写入流程。

目前没有构建或启动命令；初始化实现时增加项目验证入口 `scripts/check`。本次只验证文档导航和调研迁移完整性，未做应用构建或真实设备协议验收。
