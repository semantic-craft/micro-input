# 官方 Input 1.1.0 受保护宿主层（Codex / ChatGPT）与多 Profile 切换机制及复刻计划

本文记录从本机官方 `/Applications/input.app`（`Input 1.1.0`，固件 `0.6.3`）安装包提取并核实的**受保护宿主层（Protected Vendor Layer）与多 Profile 切换机制**，结合 `Cindy.app`（`v0.1.126`）与 `Codex.app` 的宿主接管行为，给出 `Micro Input` 1:1 复刻该官方机制的设计与验收计划（对应 `docs/pstack-relay.md` 的 **Slice O2**）。

---

## 1. 证据身份与可复核位置

- 官方安装包：`/Applications/input.app/Contents/Resources/app.asar`
  - ASAR SHA-256：`a6c7185d5c3d8dda1262437a9c0740080cceaea5585dd289b8eef0668b8a4206`
- 关键源码模块（零基 UTF-8 字节偏移）：
  - `S`：`dist/assets/smart_action-CXGxoOrO.js`（ASAR 内偏移 `39594180`）
  - `R`：`dist/assets/device_config_data-D1iqtQNe.js`（SHA-256 `b3ee40ec883df9a7c8c8744433847abe585b5cd1d1d01c1063295abe2eb23329`）
  - `M`：`dist-electron/main/index.js`（SHA-256 `ab2eaf1fe9c8ae91bdc3a09bedcfd1b491af55179b0bc162403c1c16`）
  - `K`：`node_modules/@worklouder/wl-device-kit/dist/index.js`（SHA-256 `a07dc5d90bade50f06cf94aeff79c60c305a491fceade7c4db987ffe55636876`）
- 落地实现与源码验证：
  - 受保护 OAI 层门禁与模型测试：[protected_oai_layer.rs](../../crates/mi-model/tests/protected_oai_layer.rs)
  - 受保护层判定与灯光守卫：[lighting.rs](../../crates/mi-model/src/lighting.rs) 与 FFI 接口 [lighting.rs](../../crates/mi-ffi/src/lighting.rs)
  - 原生所有权与通道让渡控制器：[NativeOwnership.swift](../../app/MicroInput/Sources/MicroInput/NativeOwnership.swift)
  - Cindy 原生接管补丁与上游问题单：[native-ownership.patch](../../integrations/cindy/native-ownership.patch) 与上游工单 [makecindy/cindy#9058](https://github.com/makecindy/cindy/issues/9058)

---

## 2. 官方 `Input 1.1.0` 是如何处理 `Codex` 与多 Profile 的

### 2.1 受保护宿主层的唯一判定标准：`Mg1(layer)`（别名 `rn`）

在 `S`（`smart_action-CXGxoOrO.js` 偏移 `39594180`）中，官方定义了判定某个 Layer 是否归外部 Agent 宿主（`ChatGPT` / `Codex`）专管的纯函数 `Mg1`（在 `R` 中以 `i as rn` 导入）：

```js
function Mg1(e) {
  var n, t, i;
  const c = "KV_OAI_";
  return !!(
    e.layout.base.some(s => s.some(o => {
      var f;
      return (f = o.keycode) == null ? void 0 : f.startsWith(c);
    })) ||
    ((n = e.layout.encoders) == null ? void 0 : n.some(s => s.some(o => {
      var f;
      return (f = o.keycode) == null ? void 0 : f.startsWith(c);
    }))) ||
    ((i = (t = e.layout.joystick) == null ? void 0 : t.sectors) == null ? void 0 : i.some(s => {
      var o;
      return (o = s.k) == null ? void 0 : o.startsWith(c);
    }))
  );
}
```

**核心事实**：
- 官方并不依赖 Profile 名称是否叫 `"Codex"`，也不依赖固定 Layer ID，而是**只要该层的矩阵按键（`base`）、旋钮（`encoders`）或摇杆扇区（`joystick.sectors`）中含有任何一个 `"KV_OAI_"` 前缀的键码，整层即判定为受保护宿主层**。
- 官方内置的 `ChatGPT` Preset（`R` 偏移 `188436`、`6732905`）结构固定为：
  - `encoders`: `[["KV_OAI_ENC_CC", "KV_OAI_ENC_CW", "KV_OAI_ENC_CLK"]]`
  - `joystick`: `{ type: "VENDOR", sectors: [] }`
  - `base`: 第 1 行 `["KV_OAI_AG00", "KV_OAI_AG01"]`，第 2 行 `["KV_OAI_AG02", "KV_OAI_AG03", "KV_OAI_AG04", "KV_OAI_AG05"]`，第 3 行 `["KV_OAI_ACT06", "KV_OAI_ACT07", "KV_OAI_ACT08", "KV_OAI_ACT09"]`，第 4 行 `["KV_OAI_ACT10", "KV_OAI_ACT11", "KV_OAI_ACT12"]`。

### 2.2 处于 `Codex` 受保护层时的四项硬隔离（为什么“用 Codex 时其他设置都不给用”）

当 `W = rn(layer)`（或 `M = rn(Q)`、`i = rn(a)`）为 `true` 时，官方 `Input 1.1.0` 在 UI 与配置层执行了四项强制锁定：

| 隔离项 | 包内定位（`R` 字节偏移） | 官方具体行为 |
| :--- | :--- | :--- |
| **1. 键盘画布锁定遮罩** | `80061`、`80658`–`80780` | 渲染 `Pe.layer_lock_overlay` 遮罩，显示 `To edit this layer please use ChatGPT app`；向键盘布局组件传入 `disabled: W`，禁止点击任何键帽、旋钮或进入摇杆编辑页 |
| **2. 自动清空选中并收起改键面板** | `6903933`、`6904980` | `useEffect(() => { i && (setKeySelected({ row: -1, index: -1, keyType: undefined }), setSidePanelVisible(false)) }, [i])`：切入受保护层时立即取消键选中并收起右侧 Key Mapping 抽屉 |
| **3. 禁用静态灯光与层操作菜单** | `6827908`、`6831450`–`6831800` | 计算 `K = deviceHasLighting(deviceType) && !M`：当 `M` 为真时隐藏 `Backlight settings` 按钮并自动关闭已打开的灯光弹窗（因为受保护层的键帽灯和底灯由宿主通过 `v.oai.rgbcfg` / `v.oai.thstatus` 实时驱动）；同时将 `showOsSelector: !M`、`showLayerColor: !M`、`canDuplicate: !M`、`canExport: !M` 全部设为 `false` |
| **4. 跨层合并键（Merge keys）跳过保护层** | `77139` | 执行跨层按键合并时，`vA.layers.forEach(oA => { rn(oA) || ... })` 显式跳过受保护层，防止将 `KV_OAI_AG10/11` 等宿主专用槽位改写成 `KC_NONE` |

### 2.3 官方 `Input 1.1.0` 的多 Profile 切换与硬件下发流程

1. **选择即切换活动 Profile**：在 `R` 偏移 `6782313` 中，点击 Profile 下拉菜单项执行：
   ```js
   _ = AA => {
     setProfileSelected(AA);
     setProfileUsed(AA);
     setLayerSelected(0);
   };
   ```
2. **倒计时自动同步硬件**：`profileUsed` 改变后触发主界面 `UpdateConfigService.stageConfig`（`R` 偏移 `7119393`）。若与本地已存设备快照不同，启动 `lV = 3` 秒倒计时（界面显示 `Updating device in 3`），倒计时结束后调用 `KeymapService.sendConfigData`（`M:13326–13511`），以 `fs.writebin`（每块 `3072` 字节）将新的 `keymap.json` 写入硬件并核对 `fs.list` 摘要。
3. **控制权移交本质**：
   - 当硬件活动层是 `Codex`（`KV_OAI_*`）时，固件不会发出普通键盘 HID 键码或 `kb.sa.*` 通知，而是发出 `v.oai.hid`（`AG00..12`、`ENC_*`）和 `v.oai.rad` 通知，完全由 `Codex.app` 或 `Cindy.app` 消费。
   - 当切换到其他 Profile（如 `DeepSeek Harness`、`ZCode`）并写入硬件后，活动层变为普通键/宏/工作台绑定，按键不再触发 `Codex` 宿主动作。

---

## 3. 官方机制在多宿主并存时的痛点与我们的根因发现

在把官方机制推广到 `Codex` + `Cindy` + `Micro Input`（管理 `Antigravity`、`ZCode`、`DeepSeek Harness`）时，存在两个必须由 `Micro Input` 在复刻时一并解决的底层约束：

1. **USB Vendor HID 单通道互斥**：
   - 硬件的 `0xFF00`（Report ID `0x06`, Channel `0x02`）同一时刻只能由一个宿主进程安全读写。
   - `Cindy.app` 开启设备开关后，其 `workLouderCodexHostProcess` 会长连 HID 并在启动时调用 `bindCreatorAgentKeys` 将当前活动层改写为 `KV_OAI_*`。如果此时 `Micro Input` 也在后台打开同一个 HID 句柄发 `device.status` 或 `lights.preview`，双方就会争抢响应帧导致超时或重连。
2. **固件 `0.6.3` 单包 `fs.read` / `fs.write` 的 `4 KB` JSON 池上限**：
   - 官方 `Input 1.1.0` 把所有 Profile 和全部 `30` 个宏（共 `7,421` 字节）塞进同一个板载 `keymap.json`。
   - 未打补丁的正式版 `/Applications/Cindy.app`（`v0.1.126`）在连接 Creator Micro 2 时调用了单包 `fsApi.readFile("keymap.json")`（`fs.read`），当板载 `keymap.json` 超过约 `4 KB` 时直接触发固件 `json pool overflow`，导致 Cindy 反复闪现“已连接 → 连接异常”（已提报上游 [makecindy/cindy#9058](https://github.com/makecindy/cindy/issues/9058)）。

---

## 4. `Micro Input` 1:1 复刻并升级官方做法的工程方案（Slice O2）

我们完全遵循官方 `Input 1.1.0` 的产品心智：**当选用 `Cindy` 或 `Codex` Profile 时，该层作为受保护的原生宿主专管层（Protected Host Layer），由 `Cindy.app` 或 `Codex.app` 独占控制，工作台锁死其他冲突设置；当切换到 `Antigravity`、`ZCode`、`DeepSeek Harness` 等 Profile 时，收回控制权并切换为对应工作台配置。**

具体分为以下 4 个可验证模块：

### 4.1 `mi-model`：复刻官方 `Mg1` 受保护宿主层判定与精简宿主载荷生成

1. **受保护宿主层判定（`is_protected_oai_layer`）**：
   - 在 `crates/mi-model` 中实现与官方 `Mg1` 完全一致的判定：检查层的 `keys`、`encoders`、`joystick.sectors` 是否包含 `"KV_OAI_"` 前缀 token。
   - 在文档视图与路由状态中暴露 `protected_host_layer: bool` 及宿主归属提示（如 `Cindy / Codex (ChatGPT)`）。
2. **编辑期硬隔离（复刻官方四项锁）**：
   - 当目标层为受保护宿主层时，`Draft` 拒绝单键改绑（`edit_key`）、拒绝静态灯光修改（`edit_lighting` / `add_lighting`）、拒绝复制/导出该受保护层，并在跨层合并键时自动跳过受保护层。
3. **板载精简宿主 `keymap.json` 生成器（解决固件 `4 KB` 溢出）**：
   - 本地 `LocalDocumentV2` 完整保留用户的全部 Profile（`Codex`、`Cindy`、`Antigravity`、`ZCode`、`DeepSeek Harness` 等）与全部 30 个宏，绝不丢失任何本地配置。
   - 当激活 `Codex` 或 `Cindy` 原生宿主 Profile 下发到硬件时，编译生成的板载候选 `keymap.json` 仅包含该单层的标准 `KV_OAI_*` + `VENDOR` 摇杆结构（约 `520` 字节，`< 1 KB`），既 100% 满足官方 `ChatGPT` 预设规格，又让未改动的正式版 `/Applications/Cindy.app` 单包 `fs.read` 秒连零溢出。

### 4.2 SwiftUI 工作台：复刻官方 `layer_lock_overlay` 与切换体验

1. **键盘画布受保护遮罩**：
   - 当选中的 Profile / Layer 为 `Cindy` 或 `Codex` 原生受保护层时，在键盘画布上方展示与官方 `Pe.layer_lock_overlay` 对齐的锁定覆盖层：`当前为 Cindy / Codex 原生托管层，按键与任务灯由对应客户端直接控制`，并禁用键帽点击、右侧改键抽屉与静态灯光面板。
2. **显式 Profile 切换与硬件/所有权同步**：
   - 从其他 Profile 切换到 `Cindy` / `Codex` Profile 时：
     1. 自动停止 `Micro Input` 的灯光预览、PTT 与宿主动作队列（`drainForNativeOwnership`）；
     2. 若板载活动层尚未处于精简 `KV_OAI_*` 宿主层，按受控写入计划下发 `520B` 官方 `KV_OAI_*` 布局并读回校验；
     3. 主动释放/挂起 `Micro Input` 的 USB HID 会话，并将 `NativeOwnershipController` 切回 `desiredOwner = "native"`，让 `/Applications/Cindy.app` 或 `Codex.app` 独占硬件。
   - 从 `Cindy` / `Codex` Profile 切换到 `Antigravity` / `ZCode` / `DeepSeek Harness` 时：
     1. 将 `NativeOwnershipController` 设为 `desiredOwner = "micro-input"`（若原生桥在运行则等待 `released` 回执；若未打补丁的正式版 `Cindy.app` 正在占用 HID，则在状态栏明确提示先关闭 Cindy 中的设备开关或完成让渡）；
     2. 重新连接并激活 `Micro Input` 的目标 Agent 路由与 HUD。

### 4.3 可证伪验收条件（Done Predicate）

1. **模型与边界门禁（Rust）**：
   - `cargo test --locked -p mi-model` 覆盖：
     - `is_protected_oai_layer` 对 `base`、`encoders`、`joystick.sectors` 含 `KV_OAI_*` 的精确识别；
     - 受保护层拒绝改键、拒绝静态灯光修改、跨层合并键自动跳过；
     - 切换到 `Codex` / `Cindy` 宿主模式时生成的精简板载 `keymap.json` 字节数 `< 1024` 字节且完整保留本地多 Profile / 30 宏归档；切回普通 Profile 时完整恢复原多 Profile 映射。
2. **宿主所有权与 UI 锁定门禁（Swift + 隔离 QA）**：
   - `swift test --package-path app/MicroInput` 与隔离 QA 场景验证：
     - 选中 `Cindy` / `Codex` 受保护层时，改键、静态灯光预览和宿主重叠动作全部锁定/暂停；
     - Profile 在 `Cindy/Codex` 与 `Antigravity/ZCode/DSH` 之间切换时，HID 会话让渡与接管状态转换无双发、无死锁；
   - 全量 `MI_AUTO_HOST_READY=0 scripts/verify-offline` 生成 `PASS` 回执。
