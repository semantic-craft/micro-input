# Swift 源码

## Overview

手写 SwiftUI/AppKit 应用与构建生成的 UniFFI bridge 分开；生成目录按 .gitignore 排除，不生成契约。

## Interface

- [MicroInput](./MicroInput/EGM.md)：子目录代码契约。
- 应用边界为 `MicroInputApp`、`Store/Backend`、`Workbench/ActionParameters` 和 JSON 模型；详见叶契约。

## Invariants

- `MicroInputApp.init` 创建单一 Store 供窗口与状态面板共享；`AppLifetime.applicationShouldTerminateAfterLastWindowClosed` 返回 false。
- `Store.selectProfile/selectLayer` 只改变本地浏览选择，不激活设备配置；当前工作台不提供设备连接入口。

## Side Effects

- AppKit 窗口/状态面板和本地文档桥接由叶模块负责；界面没有宿主动作执行器。

## Verification

仓库根目录 `scripts/check`；桥接生成后可用 `swift test --package-path app/MicroInput`。本次只核对源码与文档。
