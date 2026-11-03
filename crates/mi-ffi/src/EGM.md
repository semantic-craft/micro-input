# UniFFI 高层桥接

## Overview

当前实现的代码契约；依据 [lib.rs](./lib.rs)。下列未带路径的符号均位于该文件。规划接口不视为已实现。

## Interface

- `MicroInput::{new,draft_state,action_catalog,open_document,assign_action,undo_draft}` 提供本地文档 JSON 接口；`BridgeError::Failed` 携带消息。
- `MicroInput::{list_devices,subscribe_device_events,connect,disconnect,read_identity,read_config,load_demo,input_monitoring_status}` 提供高层设备/演示操作。
- `DeviceInfo` 包含临时路径 id、product、vid/pid、usage_page、transport；`DeviceEvents::{changed,failed}` 为回调。
- [绑定生成器](./bin/EGM.md)：`uniffi-bindgen`。

## Invariants

- `MicroInput::connect` 通过 generation 与 connect_gate 串行打开；过期 open 返回的句柄关闭。生产会话始终注入 UnimplementedFramer 和 UnverifiedSchema，没有已验证的真实 RPC 路径。
- 文档操作只调用 DocumentStore，不访问 session；`read_config` 不替换本地草稿。
- `devices_changed/invalidate` 在设备消失时退役连接；`current` 拒绝未连接读取。
- `subscribe_device_events` 持 Weak 引用，每 250ms 检查对象存活；lag 后重新枚举。该订阅不是设备输入通知流。
- `failure` 把内部错误转为字符串；不能宣称存在结构化 Timeout/Disconnected 桥接错误变体。

## Side Effects

- 文档方法经 DocumentStore 读写文件；设备方法经 spawn_blocking 枚举/open/close 和权限预检。
- `subscribe_device_events` 启动 Tokio 任务并调用外部回调；`Drop for MicroInput` 断开剩余会话。

## Verification

仓库根目录运行 `cargo test --locked -p mi-ffi`；跨桥接入口为 `scripts/check`。本次独立只读 scout 已全文读取生产文件；未构建或运行设备验收。
