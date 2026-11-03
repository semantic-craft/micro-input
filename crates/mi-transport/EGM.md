# mi-transport

## Overview

原生 HID 与内存替身。依赖与构建目标以 [Cargo.toml](./Cargo.toml) 为准。

## Interface

- [src 契约](./src/EGM.md)：公开符号、当前占位及生命周期。
- [build.rs](./build.rs): `main` 仅在 macOS 编译 discovery.c 并链接 IOKit/CoreFoundation/CoreGraphics；hidapi 的 macos-shared-device feature 见 manifest。构建会产生本地编译产物。

## Invariants

- 实际接口限制以 [源码不变量](./src/EGM.md#invariants) 为准；设计文件的未来接口不表示本 crate 已实现。

## Side Effects

- 运行时副作用见 [源码契约](./src/EGM.md#side-effects)。

## Verification

仓库根目录：`cargo test --locked -p mi-transport`；完整入口 `scripts/check`。本次仅静态核对。
