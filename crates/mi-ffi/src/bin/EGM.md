# UniFFI CLI 入口

## Overview

绑定生成器薄入口，无本地协议实现。

## Interface

- [uniffi-bindgen.rs](./uniffi-bindgen.rs): `main` 调用依赖的 `uniffi::uniffi_bindgen_main`。

## Invariants

None。此入口没有额外本地校验；CLI 契约来自 UniFFI 依赖。

## Side Effects

- `main` 委托外部生成器处理参数并读写相应文件；仓库 scripts/build 用于生成 Swift bridge。

## Verification

仓库根目录：`cargo check --locked -p mi-ffi --bin uniffi-bindgen`。本次未执行构建。
