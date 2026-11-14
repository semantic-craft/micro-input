# Creator Micro 2 `0.7.0-rc.16` 进阶硬件能力契约（FW3）

本文档记录 `Creator Micro 2`（USB `0x303A:0x8298`）在官方预发布固件 `0.7.0-rc.16` 上的进阶 RPC 契约、电源管理事件边界与实机验证证据，作为 `Slice FW3` 在 `mi-model`、`mi-proto`、`mi-session`、`mi-core` 与 SwiftUI「通用」设置页实现与验收的唯一协议契约。

> [!IMPORTANT]
> 所有写操作 RPC 均仅在固件版本满足 `supports_v070_hardware_capabilities(version)`（严格 semver `>= 0.7.0-rc.16`）、且传输层持有 macOS 独占 USB Vendor 接口（`0xFF00 / 1`，`MacHidapi267Numbered` + `is_exclusive_vendor() == true`）时放行。生产只读退出门禁（`mi_proto::Method` 与 `validate_read_only`）保持严格关闭。

## 静态分析与实机验证依据

- **固件镜像**：`0.7.0-rc.16` ESP32-S3 merged flash（`1,806,784` 字节，`sha256:9f429866986b2be640b4668d527e84df7e517e0f30559f287ba911dc2a67dab7`），APP 段映射 `0x42000020..0x42108c1c` 与 `.rodata` `0x3c110020..0x3c18b4f4`。
- **受控实机抓取**：`fw3-raw-probe` 直连独占 Vendor HID Report `0x06`（`[6, 2, len, ...]`），全程校验切换前/后 `/fs/keymap.json` `sha1` 始终保持 `c4a98a61c1a211bd37b6a3e6b00454b0cf6a2a20` 未改写（脱敏结果保存在受忽略目录 `.scratch/input-study/fw-v0.7.0-rc.16/fw3-rpc-probe.json`）。
- **关联架构证据（`docs/agents/research-context.md`）**：`IA-04`、`IA-07`、`IA-12`、`IA-13`、`IA-14`、`IA-16`。

---

## 1. 鼠标速度与摇杆死区（`mouse.config`、`joystick.deadzone`）

| 方法 | 固件位置 | 持久化 | 实机验证结论 |
| --- | --- | --- | --- |
| `mouse.config` | `0x420742ac` | NVS `wl_mouse` | 读取（`params: null`）与等价回写均返回 13 个整型字段 |
| `joystick.deadzone` | `0x4200c5bc` | NVS `joystick` | 读取（`params: null`）与等价回写返回 `{"v": 0.15}` |

### `mouse.config` 字段定义（实机默认值）

```json
{
  "delay_ms": 100,
  "interval_ms": 20,
  "move_delta": 8,
  "max_speed": 10,
  "time_to_max": 30,
  "wheel_delay_ms": 100,
  "wheel_interval_ms": 80,
  "wheel_delta": 1,
  "wheel_max_speed": 8,
  "wheel_time_to_max": 40,
  "stick_speed": 2,
  "stick_regulator": 20,
  "stick_scroll": 40
}
```

### `joystick.deadzone` 字段定义

- 请求（读）：`{"id": N, "method": "joystick.deadzone", "params": null}`
- 请求（写）：`{"id": N, "method": "joystick.deadzone", "params": {"v": <f64>}}`
- 响应：`{"id": N, "method": "joystick.deadzone", "result": {"v": 0.15}}`
- `.flash.text` 常量区边界与步进线索：`0.0025`、`0.01`、`0.03`、`0.1`、`100.0`（UI 范围收敛在 `0.02..0.45`，默认 `0.15`）。

---

## 2. 无写 Flash 实时切层（`kb.pl.set` 与出站事件 `kb.pl`）

| 标识符 | 固件位置 | 方向 | 持久化 | 实机验证结论 |
| --- | --- | --- | --- | --- |
| `kb.pl.set` | `0x4205ddbc` | Host → Device RPC | 不写 Flash（`keymap.json` SHA-1 不变） | `{"p": 3, "l": 1}` 返回 `{}`，随后 `device.status` 立即反映 `layer_index: 1` |
| `kb.pl` | `0x4205c8b6` | Device → Host Notify | — | 入站请求 `kb.pl` 返回 `404 Method not found`；切层实际触发出站事件 |

### 协议契约

- 请求：`{"id": N, "method": "kb.pl.set", "params": {"p": <u8>, "l": <u8>}}`（至少提供 `p` 或 `l` 之一；均缺返回 `400 "missing p and l"`；越界或未加载返回 `"invalid p"` / `"invalid l"` / `"profile not loaded"`）。
- 响应：`{"id": N, "method": "kb.pl.set", "result": {}}`
- 重要实机发现：当前机载六层键位（开启全局基础层，`profile_index = 3`）下，层 `0` 为全局保留基底层，可选应用层自 `l = 1` 开始（实测 `{"p": 3, "l": 0}` 返回 `400 "invalid l"`，`{"p": 3, "l": 1}` 与 `{"p": 3, "l": 2}` 均切换成功且 `/fs/keymap.json` SHA-1 保持 `c4a98a61c1a211bd37b6a3e6b00454b0cf6a2a20` 不变）。
- 出站通知格式：`{"m": "kb.pl", "p": {"p": 3, "l": 1}}`（经 `WORKLOUDER_KEYMAPPER_EVENTS` 广播，同一 `(p, l)` 幂等切换不重复广播）。

---

## 3. 休眠跟随、电源地板与电量状态（`pm.config`、`pm.state`、`device.status`、`usb.host`）

### `usb.host` 的真实身份边界

静态 `.flash.text`（`0x42040014`、`0x4204039c`，`wl_host_follow.cpp`）与实机抓取共同证实：**`usb.host` 不是可调用的入站 JSON-RPC 方法**，而是 TinyUSB suspend/wake 驱动在电源管理模块 `wl_pm` 中持有的地板锁 owner 字符串，并在 `pm.config.host_sleep` 控制下自动释放或抬升功耗地板。

### `pm.config`（`0x4204e014`，NVS `wl_pm`）

- 读：`params: null` → 实机返回：
  ```json
  {
    "auto_sleep": true,
    "host_sleep": true,
    "sleep_after_min": 15
  }
  ```
- 写：提交 `{auto_sleep, host_sleep, sleep_after_min}` 并原样返回当前保存值（已实机验证幂等回写）。

### `pm.state`（`0x4204d504`，只读）

```json
{
  "auto_sleep": true,
  "base_floor": "standby",
  "credit_s": 0,
  "floor": "turbo",
  "holds": [
    {
      "age_ms": 1786077,
      "floor": "turbo",
      "owner": "usb.host"
    }
  ],
  "idle_s": 15,
  "line_powered": true,
  "sleep_after_s": 900,
  "standby_s": 45,
  "state": "turbo"
}
```

### `device.status` 电量扩展与 `sys.charger_diagnostic_summary`

- `device.status`（`0x4207d434`）新增 `battery`（`0..100`）与 `is_charging`（`bool`），实机返回：`{"battery": 100, "is_charging": true, "layer_index": 2, "name": "Creator Micro 2", "profile_index": 3, "version": "0.7.0-rc.16"}`。
- `sys.charger_diagnostic_summary` 在 `params: null` 时额外返回 `{"category": "power.max77972.summary", "status": "ok", "payload": "..."}`（仅作只读诊断证据，不纳入常规退出链）。

---

## 4. 设备命名与动态 Smart Action 灯光（`device.name`、`kb.sa.rgb`）

### `device.name`（`0x42042d34`，NVS `wl_identity`）

- 读（`params: null`）：`{"factory_name": "Creator Micro 2", "max_length": 15, "name": "Creator Micro 2"}`
- 写（`{"name": "<string>"}`）：写入最大 15 字符的设备显示名（已在实机以原名校验回写）。

### `kb.sa.rgb`（`0x4206fd78`，`api_lighting.cpp`）

结合 `0x4206fd78`、`0x4206f90c` 与 `.rodata`（`0x3c178a8c`）反汇编及实机边界验证，精准协议契约如下：

- 请求参数：
  - `sa`（必填，非空 `Vec<u32>`，元素 `>= 1` 且 `<= 0xFFFFFF`）：目标 Smart Action 编号数组；传标量、空数组或 `null` 返回 `400 "invalid sa"`。
  - `c`（可选 `u32`，`0..=0xFFFFFF`）：24 位 RGB 颜色值（超界返回 `400 "invalid c"`）。
  - `b`（可选 `f32`，**单位浮点 `0.0..=1.0`**）：亮度比例，由辅助函数 `0x4206f90c`（常量 `0x3f800000 = 1.0f`）校验；实机传递 `100`、`128` 或 `1.1` 均返回 `400 "invalid b"`，`0.5` 返回 `{}`。
  - `s`（可选 `f32`，**单位浮点 `0.0..=1.0`**）：速度比例；实机传递 `50`、`101` 返回 `400 "invalid s"`，`0.5` 返回 `{}`。
  - `e`（可选 `u8`）：由位掩码 `0x5b = 0b0101_1011` 校验，仅允许 `{0, 1, 3, 4, 6}`（对应 `0=off/reset`、`1=solid`、`3=rainbow`、`4=breath`、`6=shallow_breath`）；`2=snake` / `5=gradient` 返回 `400 "unsupported effect"`。
- **重置语义**：`e = 0` 即触发指定 `sa` 列表的动态 RGB 重置，此时不得附带 `c`/`b`/`s`（实机传递 `{"sa": [1], "e": 0, "c": 255}` 返回 `400 "reset takes no other value"`，传递 `{"sa": [1], "e": 0}` 返回 `result: {}`）。若 `c`/`b`/`s`/`e` 全部缺省则返回 `400 "empty update"`。
