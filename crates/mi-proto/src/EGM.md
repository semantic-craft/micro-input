# RPC 类型与占位分帧

## Overview

当前实现的代码契约；依据 [lib.rs](./lib.rs)。下列未带路径的符号均位于该文件。规划接口不视为已实现。

## Interface

- `ProtoError::{Unverified,Invalid}`；`Method::{SysVersion,DeviceStatus,FsList,FsRead,FsReadbin}` 与 name。
- `RpcRequest::{new,id}`、`RpcResponse`、`Notification`、`Incoming`；`RpcCodec::{encode,decode}` 由 JsonCodec 实现。
- `Report/Line` 为 Vec<u8>；`Framer::{encode,feed,clear}` 与 UnimplementedFramer。
- `Identity::Unknown`、`HostNotification` 和 `HostAction::FocusedAppUnknown` 保留未知数据与通知分类。

## Invariants

- `RpcRequest::new` 仅接受 id<999；params 仍是任意 JSON，不存在文件名/参数生产白名单校验。
- `JsonCodec::encode` 写 method/params/id，非 ASCII 转 UTF-16 转义，不追加换行。
- `JsonCodec::decode` 要求对象；存在 id 时按响应处理且检查无符号 id<999，否则取 method/m。不识别 compact i，不校验 result/error 恰一或 error 字段结构。
- `UnimplementedFramer::encode/feed` 返回 UNKNOWN-01 Unverified，clear 无操作。Method 没有写入变体，但 FsRead 仍存在。
- `HostNotification::from` 只分类 kb.sa/kb.radial/kb.cs 等通知，不执行动作。

## Side Effects

None。该模块仅进行内存转换；无磁盘、设备或宿主执行。

## Verification

仓库根目录运行 `cargo test --locked -p mi-proto`；跨桥接入口为 `scripts/check`。本次独立只读 scout 已全文读取生产文件；未构建或运行设备验收。
