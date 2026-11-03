# mi-model

## Overview

本地配置投影、结构化动作意图和 v1 草稿持久化库。Cargo 依赖 serde、serde_json、thiserror 与 url；没有设备会话依赖。

## Interface

- [源码契约](./src/EGM.md)：`lib.rs:DeviceConfig/parse_config/demo`、`draft.rs:Draft/Action`、`storage.rs:DocumentStore`。
- [Cargo.toml](./Cargo.toml)：`mi-model` 库，无本地 CLI target。

## Invariants

- `src/draft.rs:Draft` 使用 schema_version=1；绑定由来源 Profile/Layer ID 与 logical-N 定位。这里没有 v2 UUID 图或迁移 API。
- `src/storage.rs:DocumentStore::update` 先持久化克隆，再提交内存状态；动作校验由调用的 Draft 方法负责。

## Side Effects

- `src/lib.rs:parse_config` 读取系统时钟；`src/storage.rs:DocumentStore` 读写本地文档与锁。无设备、网络或宿主执行。

## Verification

仓库根目录：`cargo test --locked -p mi-model`；跨桥接检查：`scripts/check`。本次仅静态核对。
