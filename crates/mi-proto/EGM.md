# mi-proto

## Overview

RPC 类型与占位分帧。依赖与构建目标以 [Cargo.toml](./Cargo.toml) 为准。

## Interface

- [src 契约](./src/EGM.md)：公开符号、当前占位及生命周期。

## Invariants

- 实际接口限制以 [源码不变量](./src/EGM.md#invariants) 为准；设计文件的未来接口不表示本 crate 已实现。

## Side Effects

- 运行时副作用见 [源码契约](./src/EGM.md#side-effects)。

## Verification

仓库根目录：`cargo test --locked -p mi-proto`；完整入口 `scripts/check`。本次仅静态核对。
