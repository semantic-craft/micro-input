# InputKit 基础层与单设备写入预算

本文档记录基于开源仓库 [worklouder/input-kit](https://github.com/worklouder/input-kit) `v0.3.0`（Apache License 2.0）静态分析落地的一期基础能力，以及二期按需启用的接线路径。

## 1. 上游 `worklouder/input-kit` v0.3.0 协议与限流基线

`worklouder/input-kit` v0.3.0（Apache-2.0）定义了客户端通过本机回环 WebSocket（`ws://127.0.0.1:19147/input`、`ws://[::1]:19147/input`、`ws://localhost:19147/input`）与主机侧 JSON-RPC 2.0 通信的公开契约，包含以下关键工程基线：

| 维度 | 公开契约参数 | 一期处理 |
| --- | --- | --- |
| 单设备写入预算（USB） | 10 秒滚动窗口内最多 60 次写入 | 立即落地并接入 `DeviceSession` 与 `VendorBridge` |
| 单设备写入预算（BLE） | 10 秒滚动窗口内最多 40 次写入，且相邻写入间隔 ≥ 250 ms | 立即沉淀为 `WriteBudgetPolicy::ble()`，按 `TransportHint` 自动派生 |
| 单连接请求节流 | 10 秒滚动窗口内最多 65 次请求，相邻间隔 ≥ 160 ms | 沉淀为 `ConnectionRequestBudget`，供二期本地 WS 连接层复用 |
| 帧上限与关闭码 | 单帧最大 512 KiB；二进制帧关闭码 `4003`；连接限流关闭码 `1008` | 沉淀为 `mi-proto::input_kit` 常量与解码门禁 |
| 单设备写入限流错误码 | JSON-RPC 错误码 `-32017`（拒绝写入但保持连接不断开） | 映射为 `SessionError::DeviceRateLimited` 与 `InputKitErrorCode::DeviceRateLimited` |
| JSON-RPC 2.0 方法与通知 | 7 个方法（`requestAuthorization`、`authenticate`、`getDevices`、`getDeviceInfo`、`setActiveProfileLayer`、`setKeyLighting`、`addLayer`）与 3 个通知（`devicesUpdated`、`deviceInfoChanged`、`keyTriggered`） | 沉淀为零新依赖的强类型编解码与边界校验模块 `mi-proto::input_kit` |

一期保持**零新增 Cargo 依赖**、不开放本地 `19147` 监听端口、不改动只读出口 `mi_proto::validate_read_only` 与 `mi-bridge` 特性隔离边界。

## 2. 一期已交付能力

### 2.1 `mi-transport::budget`：跨会话共享的单设备写入预算器

位于 [`crates/mi-transport/src/budget.rs`](../crates/mi-transport/src/budget.rs)：

- **`DeviceBudgetKey`**：按物理设备身份（优先序列号 `Serial`，其次非零 USB 位置 `UsbLocation`，最后 `VendorPath`）稳定索引同一把键盘。
- **`WriteBudgetPolicy`**：
  - `WriteBudgetPolicy::usb()`：10 秒滚动窗口、最多 60 次写入、`min_spacing = 0ms`（保留实时切层与单帧亮灯突发响应速度）。
  - `WriteBudgetPolicy::ble()`：10 秒滚动窗口、最多 40 次写入、`min_spacing = 250ms`。
  - `WriteBudgetPolicy::unbounded()`：关闭限流，供 `SessionConfig::interval == 0` 的离线单元测试使用。
  - `WriteBudgetPolicy::for_transport(TransportHint)`：`Usb => usb()`，`Bluetooth | Unknown => ble()`。
- **`WritePriority`**：`BackgroundSync`、`Normal`、`UrgentClear`。`UrgentClear`（例如退出/切离受保护层时的清灯）允许跳过软间隔 `min_spacing`，但仍严格遵守 10 秒滚动窗口硬上限。
- **`DeviceWriteBudget` 与 `DeviceBudgetRegistry`**：采用最小临界区 `Mutex<RollingWindow>`（不跨越 `.await` 持锁），提供 `next_ready_at`、`check_batch_capacity`、`try_acquire`、`update_policy` 与 `snapshot`。`DeviceBudgetRegistry::budget_for` 为同一设备复用同一个 `Arc<DeviceWriteBudget>`，并在 USB/BLE 切换时原地更新策略而不丢失历史窗口。
- **`ConnectionRequestBudget`**：提供 65 次/10 秒、160 ms 节流的纯状态机，供未来本地 WebSocket 连接层直接复用。

### 2.2 `mi-proto::input_kit`：零新依赖协议契约与边界校验

位于 [`crates/mi-proto/src/input_kit.rs`](../crates/mi-proto/src/input_kit.rs)：

- 完整导出端口、路径、512 KiB 帧上限、错误码（含 `-32017`）、关闭码（`1008` / `4003`）与各类超时常量。
- `LoopbackHost` 仅放行 `127.0.0.1`、`::1` 与 `localhost`，并生成规范 `ws://` URL。
- `InputKitRequest::decode_frame` 强制执行 512 KiB 帧上限、`jsonrpc == "2.0"`、有效请求 ID 与完整参数边界校验（`deviceId` 正安全整数、`profileIndex >= 0`、`layerIndex >= 1`、`applicationName` 去空白后 1..=80 可打印字符且拒绝 `Cc/Cf/Zl/Zp`、`identifier` 精确模式 UTF-16 长度 1..=128 且禁止控制符与行段分隔符、正则模式 UTF-16 长度 1..=256）。
- `SetKeyLightingParams` 直接复用 `crate::v070_capabilities::{SaRgbEffect, SaRgbUpdate}` 作为单一真源；`to_sa_rgb_updates(&[u32], chunk_size)` 将 `SaRgbEffect::Off` 显式规范化为 `SaRgbUpdate::reset(chunk)`，避免触发固件 `reset takes no other value` 拒绝。
- `InputKitResponse` 与 `InputKitNotification` 覆盖全部 7 类响应（含 `-32017` 限流错误）与 3 类服务端通知。

### 2.3 `mi-session` 与 `mi-core` 运行时接入

- **统一流量分类（[`crates/mi-session/src/lib.rs`](../crates/mi-session/src/lib.rs)）**：`RpcTrafficClass::{ReadOnly, ConfigChunk, ControlWrite}` 通过 `classify_rpc_call` / `classify_rpc_method` 单一入口映射全部固件 RPC：
  - `ReadOnly`（`sys.version`、`device.status`、`fs.list`、`fs.read`、`fs.readbin`、`pm.state` 及无写载荷的设置读取）：零预算消耗，即使写入窗口耗尽仍可立即执行。
  - `ConfigChunk`（`fs.txbegin`、`fs.writebin`、`fs.txcommit`）：不按分片重复扣减预算；整次逻辑配置写入在事务开始前校验并计入 1 次写入。
  - `ControlWrite`（`kb.pl.set`、`kb.sa.rgb`、`v.oai.thstatus`、`v.oai.rgbcfg`、`lights.preview`、`mouse.config` 写、`joystick.deadzone` 写、`pm.config` 写、`device.name` 写、`sys.bootloader`）：在获取 `DeviceSession::queue` 锁**之前**完成预算等待或快速拒绝，杜绝队头阻塞；窗口耗尽时返回 `SessionError::DeviceRateLimited { method, retry_after }` 且保持会话存活。
- **跨轮换与重连共享预算（`crates/mi-core`）**：`WorkbenchStore` 持有 `Arc<DeviceBudgetRegistry>`，并贯穿 `device_sync`、`six_layer_store`、`firmware_store`、`six_layer_runtime::lifecycle` 与 `VendorBridgeSpawnConfig`，确保 `VendorBridge` 每 900 次请求轮换会话时沿用同一把设备的 `DeviceWriteBudget` 与滚动窗口计数。
- **动态灯效协同退避（[`crates/mi-core/src/v070_session_sa_rgb/coordinator.rs`](../crates/mi-core/src/v070_session_sa_rgb/coordinator.rs)）**：`SaRgbRuntimeCoordinator` 在调度与批量刷新时参考 `session.write_budget().next_ready_at(now, priority)` 自动退避，后台同步使用 `WritePriority::BackgroundSync`，`teardown_blocking` 与紧急清灯使用 `WritePriority::UrgentClear`。

## 3. 二期启用指南

当以下场景进入开发范围时，可直接在一期底座上按需挂接，无需返工底层预算与协议类型：

1. **高频动态灯效**：调用方在派发多块 `SaRgbUpdate` 前使用 `session.write_budget().check_batch_capacity(now, chunks.len(), priority)` 预检余量；不足时直接按 `next_ready_at` 合并或延后，不再与 `VendorBridge` 互相挤占 10 秒硬件窗口。
2. **BLE 配置与灯效通道**：当 `mi-transport` 放行蓝牙 Vendor 写入链路时，`DeviceBudgetRegistry::budget_for` 会依据 `TransportHint::Bluetooth` 自动将同一设备的预算切换为 `WriteBudgetPolicy::ble()`（40 次/10 秒、≥ 250 ms 间隔）并保留既有窗口历史。
3. **可选本机 `19147` 插件兼容服务（如 `claude-plugin` / `cursor-plugin`）**：
   - 在独立特性门禁下引入回环 WebSocket 服务端，仅绑定 `LoopbackHost` 允许的地址与端口 `INPUT_KIT_DEFAULT_PORT`（`19147`），路径固定 `/input`。
   - 每条连接持有一个 `ConnectionRequestBudget::for_input_kit_connection()`（超限关闭码 `INPUT_KIT_WS_RATE_LIMIT_CLOSE_CODE = 1008`，二进制帧关闭码 `INPUT_KIT_WS_BINARY_CLOSE_CODE = 4003`）。
   - 收到文本帧后经 `InputKitRequest::decode_frame` 解析并按方法路由到现有 `DeviceSession`；若目标设备写入预算耗尽，直接返回 `InputKitResponse::device_rate_limited(id, ...)`（错误码 `-32017`）并保持 WebSocket 连接不断开。
   - 若支持 `setKeyLighting({ isRegex: true })`，在应用层结合 `regex` 引擎将匹配到的 Smart Action 标识映射为 `&[u32]`，再交由 `SetKeyLightingParams::to_sa_rgb_updates(&sa_ids, 16)` 生成合法分块并下发。

## 4. 版权与开源归属

本模块设计的协议常量、方法名称与边界校验规则参考自 [worklouder/input-kit](https://github.com/worklouder/input-kit) `v0.3.0`，其原始代码依据 Apache License 2.0 发布。Micro Input 侧全部 Rust 实现均为独立编写的零新依赖实现。
