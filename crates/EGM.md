# Rust 工作区契约

## Overview

Cargo workspace 将模型、协议、会话、传输和 UniFFI 桥接拆为五个 crate；依赖由根 Cargo.toml 与各 crate manifest 定义。

## Interface

- [mi-model](./mi-model/EGM.md)：本地配置、草稿及持久化。
- [mi-proto](./mi-proto/EGM.md)：RPC 编解码、类型化方法与占位分帧。
- [mi-session](./mi-session/EGM.md)：串行请求、快照及请求期间的通知广播。
- [mi-transport](./mi-transport/EGM.md)：USB HID 发现和原生句柄。
- [mi-ffi](./mi-ffi/EGM.md)：面向 Swift 的高层应用接口与 DTO。

## Invariants

- `mi-ffi::MicroInput` 暴露高层操作；生产通过未验证 Framer/schema 拒绝真实通信，不能从 Swift 发任意 RPC。
- `mi-model::DocumentStore` 的本地持久化与 `mi-session::DeviceSession` 的设备状态各自拥有生命周期，保存草稿不等于设备写入。

## Side Effects

- 文档存储访问本地文件；传输与可注入会话提供 HID I/O，生产桥接的未验证分帧/schema 拒绝真实 RPC。各 crate 契约分别记录具体入口与限制。

## Verification

仓库根目录：`cargo test --locked --workspace`；跨 Swift 桥接验证使用 `scripts/check`。本次仅核对文档与源码。
