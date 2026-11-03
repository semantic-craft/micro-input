# 原生 HID 与内存替身

## Overview

当前实现的代码契约；依据 [lib.rs](./lib.rs)。下列未带路径的符号均位于该文件。规划接口不视为已实现。

## Interface

- `TransportError::{Disconnected,Unavailable,Unsupported}`、`TransportHint::{Usb,Bluetooth,Unknown}`、`HidCandidate`、`is_candidate`。
- `HidLink::{write,read,close}`、`HidTransport::{enumerate,subscribe,open}`；MacTransport 与 MockTransport。
- `MockLink::new` 返回可注入链接和 outgoing/时间戳写入记录；`MockTransport::{new,set_devices}`。
- `input_monitoring_allowed` 调用 macOS 预检，其他平台返回 false。
- [discovery.c](./discovery.c)：`mi_changes`、`mi_permission`、`mi_watch` C ABI。

## Invariants

- `is_candidate` 检查 VID303a、PID8297/8298、usage_page ff00；不检查完整 usage pairs。`MacTransport::open` 重枚举并要求当前 path 和 USB，两种 PID 均可打开。
- `MacTransport::enumerate` 从 hidapi bus_type 取得 transport，不用 release 位猜测。
- `MacTransport::start_watching` 每实例只启动一次；每 25ms 检查变化后再等待 175ms 合并。Drop 仅设停止位，不 join 线程。
- `MacLink` 用 mutex 保护 Option 句柄；关闭后拒绝 I/O。read 使用 256 字节缓冲与 20ms read_timeout 直到非空。
- `MockTransport::open` 忽略 id 并返回配置的 mock link；files 字段在本实现中未使用。不能把 mock 当物理协议。
- `discovery.c:mi_watch` 注册 VID303A 匹配/移除回调并运行 run loop，不打开设备或读取报告；`mi_permission` 仅预检，不弹授权。

## Side Effects

- MacTransport 枚举 OS 设备、打开原生句柄、创建 watcher 与事件轮询线程；MacLink 经 spawn_blocking 执行 HID I/O。
- MockLink/MockTransport 改内存通道、写入记录和候选列表。
- `discovery.c:changed/mi_watch` 更新进程级原子计数并创建/释放 IOHIDManager。

## Verification

仓库根目录运行 `cargo test --locked -p mi-transport`；跨桥接入口为 `scripts/check`。本次独立只读 scout 已全文读取生产文件；未构建或运行设备验收。
