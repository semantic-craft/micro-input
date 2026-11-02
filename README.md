# Micro Input

面向 Work Louder Creator Micro 2 的独立 macOS 配置器与宿主服务。技术方向：**Rust 核心 + SwiftUI 界面**。项目名为工作名。

## 当前状态

2026-11-02：已建立正式项目及公开 GitHub 仓库，前期调研原件仅保存在本机；尚未初始化 Cargo/Swift 工程、实现设备通信或构建可运行应用。既有 input.app 和设备配置保持原状。

从 [开发起点](docs/development.md) 开始；操作设备或处理兼容行为前阅读 [项目教训](lessons.md)。Agent 入口见 [AGENTS.md](AGENTS.md)。

工单与规格统一放在 [GitHub Issues](https://github.com/semantic-craft/micro-input/issues)。

## 调研入口

以下为本机原始调查材料，保存在 Git 忽略的 `.scratch/input-study/`，不随源码提交：

- [官方来源、能力矩阵与公开协议线索](.scratch/input-study/out/research.md)
- [已连接 UI 清单](.scratch/input-study/out/connected-ui-inventory.md)
- [灯光、摇杆、层、AppSense 与参数实验](.scratch/input-study/out/layer-lighting-joystick-followup.md)
- [观察规格](.scratch/input-study/out/spec.json)和[证据记录](.scratch/input-study/out/evidence.jsonl)
- [原任务历史](.scratch/input-study/task-history.md)

旧入口 `~/agent-tasks/worklouder-input-study` 仅保留晋升记录和兼容链接。后续项目事实、代码和进度只在本项目维护。

## 下一步

首版范围已确定为 USB 只读配置查看器：发现一台 Creator Micro 2 → 读取身份与配置 → 浏览 Profile、Layer、按键及旋钮映射。范围、验收和依赖以 [MVP 总工单 #1](https://github.com/semantic-craft/micro-input/issues/1) 为准。下一步执行 [#2 合成数据查看器](https://github.com/semantic-craft/micro-input/issues/2)，先交付真实 Rust / SwiftUI 桥接的可启动应用，再逐步接入设备。

目前没有构建或启动命令；初始化实现时增加项目验证入口 `scripts/check`。本次只验证文档导航和调研迁移完整性，未做应用构建或真实设备协议验收。
