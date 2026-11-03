# MicroInput 包

## Overview

Swift tools 5.9，macOS 14+；executable MicroInput 依赖生成的 MicroInputCore 与 systemLibrary MicroInputCoreFFI。

## Interface

- [Sources](./Sources/EGM.md)：子目录代码契约。
- 应用边界为 `MicroInputApp`、`Store/Backend`、`Workbench/ActionParameters` 和 JSON 模型；详见叶契约。

- [Package.swift](./Package.swift): `package` 定义 targets，按 checkout 位置链接 target/debug/libmi_ffi.a 与 IOKit/CoreFoundation/CoreGraphics/Security/c++。
- [Info.plist](./Info.plist): bundle ID craft.semantic.micro-input.dev，版本 0.1.0/build 1。

## Invariants

- `MicroInputApp.init` 创建单一 Store 供窗口与状态面板共享；`AppLifetime.applicationShouldTerminateAfterLastWindowClosed` 返回 false。
- `Store.selectProfile/selectLayer` 只改变本地浏览选择，不激活设备配置；当前工作台不提供设备连接入口。

## Side Effects

- AppKit 窗口/状态面板和本地文档桥接由叶模块负责；界面没有宿主动作执行器。

## Verification

仓库根目录 `scripts/check`；桥接生成后可用 `swift test --package-path app/MicroInput`。本次只核对源码与文档。
