# Input 1.1.0 私有协议与业务架构

本轮结论：Input 是 Electron 界面、主进程设备服务、Work Louder SDK 和 macOS 能力的组合。配置下行以文件为单位；宿主功能上行以通知为单位。前台应用检测又形成一条宿主到设备的消息链。独立管理器必须分别处理配置、会话和宿主动作，不能把全部键盘功能视为一种宏执行器。

范围：本机 Input 1.1.0，Creator Micro 2 相关代码。以下为当前安装包的静态事实或明确标记的推断，不是本轮实机协议验收。没有启动、重启、注入 Input，没有读取个人配置、发送设备请求或改变映射。正式方案见[分析设计](designs/input-protocol-reconstruction.md)，候选字段见[协议补充](protocol-candidates.md)，独立实现边界见[模块契约](module-contracts.md)。

## 证据身份与可复核位置

ASAR SHA-256：`a6c7185d5c3d8dda1262437a9c0740080cceaea5585dd289b8eef0668b8a4206`。

本轮直接从 `/Applications/input.app/Contents/Resources/app.asar` 提取所引用文件，逐一核对 ASAR 内声明的 SHA-256。原件、manifest、工具输出仅保存在本机 `.scratch/input-study/craft-anything/analysis/`。下文行号为未格式化的包内文件；压缩前端文件额外提供零基 UTF-8 字节偏移。路径缩写：

- `M`：`dist-electron/main/index.js`，SHA-256 `ab2eaf1fe6bb241fe9c8ae91bdc3a09bedcfd1b491af55179b0bc162403c1c16`。
- `K`：`node_modules/@worklouder/wl-device-kit/dist/index.js`，SHA-256 `a07dc5d90bade50f06cf94aeff79c60c305a491fceade7c4db987ffe55636876`。
- `P`：`dist-electron/preload/preload.mjs`。
- `R`：`dist/assets/device_config_data-D1iqtQNe.js`。
- `U`：`dist/assets/Main-CxBuDtba.js`。
- `W`：`dist-electron/workers/search_devices_worker.js`。

| 结论 ID | 包内定位 | 支持的事实 |
| --- | --- | --- |
| IA-01 | P:4–8；M:532、3430–3431、3485–3518 | 枚举值定义 IPC 通道；配置调用先返回取消标识，最终结果另发事件 |
| IA-02 | W:7–72；M:6463–6514；K:5368–5750 | worker 合并扫描、监听变化，管理器为发现的设备创建连接对象 |
| IA-03 | M:5850–5905、6515–6605；K:5937–5967 | 每设备持有通信与 RPC 服务；Creator Micro V2 标记 radial/focusedApp 特性；连接后查询状态 |
| IA-04 | K:6079–6100、6141–6230、6268–6418 | 队列、HID 帧、换行重组、响应/通知分派、超时及任务间隔 |
| IA-05 | M:6849–7068、7178–7225 | 文件列表、摘要比较、缓存与配置加载；缓存是单独的数据层 |
| IA-06 | M:13326–13511 | Smart Actions 按需先写、keymap 后写、list 摘要核验、保存本地配置 |
| IA-07 | K:4778–4864、4877–4898、5192–5220、5280–5310 | 分块写参数；取消队列；另有事务 API，但普通配置发送链未调用它 |
| IA-08 | M:5182–5255、14030–14044 | userData 下的 JSON 数据库与 devices 文件缓存 |
| IA-09 | M:5721–5804；K:5317–5324 | 前台应用检测、变化比较、host.focused_app；五次采样的应用选择辅助流程 |
| IA-10 | M:5806、5990–6035、14141–14210、5616–5644 | 宿主通知，命令同意开关，文本/应用/URL 执行边界 |
| IA-11 | M:4385–4458 | 径向菜单与 Cheat Sheet 使用设备通知及本地配置投影 |
| IA-12 | M:2509–2514、2650–2656、3035–3044、3148–3161、13472–13491 | 宏、Multi Action、keymap 的设备序列化字段 |
| IA-13 | R:50，字节 7118596–7124300；U，字节 25097 附近 | 更新调度、去重比较、倒计时、发送与本地失败回退 |
| IA-14 | R:50，字节 6782313 附近 | Profile 选择处理器同时改变选择值和使用值，并选择第 0 层 |
| IA-15 | M:13163–13236 | 解析 keymap 可触发迁移写回；已读分支仅为 KnobF1/NomadEV2 提供迁移配置 |
| IA-16 | K:5993–6063；M:6649–6657 | 断连拒绝队列与等待者、清理状态、关闭句柄；管理器移除订阅及设备对象 |
| IA-17 | M:2015–2083、2867–2891、13932–13934 | 两区灯光字段、Layer 持久化及即时预览调用边界 |

IA 定位由逐段阅读确认。它们不是 Craft Anything 自动证明的全部调用关系。

## 工具覆盖与局限

Craft Anything 0.2.0 全包映射 Evidence：`ev_9ab0932d2539c2ed69b85c8f761af0c2766fee0843641bdac8a5b75d4303c6af`。报告统计为 19 个源码模块、5310 个节点、9742 条边；覆盖为 examined 1627 / total 2969，状态仍是 partial。19 个模块的摘要已完整取回，但不因此宣称整个安装包完整覆盖。

自动映射没有配对出 IPC 通道，并明确报告 198 处动态通道调用。包内使用 `enum.member` 而不是直接字符串参数，因此手工解析枚举、preload 和主进程 handler 才得到 IA-01。不能把零条自动配对解释成不存在 IPC。设备 SDK 单独分析 Evidence：`ev_a0d0738c0be065952289c2c32e9b1c81a38ddbfd4fad1fb28898c11bed5cb926`。

Electron fuse Evidence：`ev_d669f37f947bb84de491afa29564d7950bb72dafd22630c8f263b403b3564574` 显示 `EnableNodeCliInspectArguments` 启用；这只证明包允许调试参数。对正在运行的 Input 主进程检查未发现 TCP LISTEN 端口，未建立 Inspector 会话。本轮没有取得运行时调用轨迹，也没有证明独立 userData 目录可完全隔离原生副作用。

## 1. 组件架构

图中实线为包内代码可确认的连接，虚线为外部行为边界，不能作为本轮固件执行证据。

```mermaid
flowchart TB
  UI[Renderer：编辑状态与更新调度] -->|IA-01 / IA-13| PRE[Preload：contextBridge IPC]
  PRE --> CH[主进程 Channel handlers]
  CH --> KEY[KeymapService：序列化与发送]
  CH --> DM[DevicesCommManager：每设备对象]
  WORK[Discovery worker] -->|IA-02| DM
  KEY -->|IA-06| RPC[每设备 WLRPCApi]
  DM --> RPC
  RPC --> COMM[WLDeviceComm：队列与分帧]
  COMM -->|IA-04| HID[node-hid / 原生 HID]
  HID -. USB .-> FW[键盘固件]
  COMM -->|IA-10 通知| NOTIFY[DeviceNotifyService]
  NOTIFY --> NATIVE[NativeService / 系统执行接口]
  NOTIFY --> OVERLAY[WindowService：径向菜单与提示窗]
  FOCUS[FocusAppService] -->|IA-09| RPC
  NATIVE --> FOCUS
  KEY -->|IA-08| DB[本地配置数据库]
  FILE[DeviceFileService] --> RPC
  FILE --> CACHE[本地设备文件缓存]
  FILE --> DB
  OVERLAY --> DB
```

启动服务集中在 M:14397–14399。图保留与 Creator Micro 相关的边界；升级、图片转换和其他设备的媒体功能不表示 Creator Micro 全部支持。

## 2. 连接与读取

```mermaid
sequenceDiagram
  participant W as Discovery worker
  participant D as DevicesCommManager
  participant C as Device / Comm / RPC
  participant H as HID设备
  participant F as DeviceFileService
  W->>D: HID及serial候选发生变化
  D->>D: 比对身份，移除消失对象，创建新对象
  D->>C: connect
  C->>H: 非独占打开HID路径
  C-->>D: CONNECTED事件
  D->>C: device.status
  Note over D,C: 延迟后查询；状态失败时的版本回退见只读协议
  C->>H: JSON经HID分帧发送
  H-->>C: 回应重组、按id分派
  C-->>D: 身份/状态
  Note over D,F: 文件服务受连接事件触发；不声称与身份查询全局串行
  F->>C: fs.list及按需fs.readbin
  F->>F: 校验摘要、解析、更新缓存和本地配置
```

IA-02–05。Input 会自动连接发现的设备；不能把打开 Input 当作不接触硬件的纯浏览操作。身份键含设备 PID、序列号规范化值、连接类型、型号、布局和端口路径；不能直接把其临时对象 ID 当作跨重连身份。

通信层逐请求排队；单请求超时为 10 秒，任务结束后等待 50 ms 才处理下一项（IA-04）。这些是该客户端策略，不是已证明的固件强制参数。

断连生命周期可进一步恢复如下（IA-02、04、16）。注意原 SDK 的 `cleanOnDisconnect` 清除队列、resolver、rpcResponse 和句柄，但本段没有清除 `buffers[channel]`；不能从“清理”这个函数名推断所有分帧残留均清空。

```mermaid
stateDiagram-v2
  [*] --> Discovered
  Discovered --> Connected: 管理器创建对象并打开
  Connected --> AwaitingResponse: 发送队首请求
  AwaitingResponse --> Connected: 收到匹配回应
  AwaitingResponse --> Connected: 超时拒绝本次请求并继续队列
  Connected --> Removing: 设备消失或明确移除
  AwaitingResponse --> Removing: 设备消失或明确移除
  Removing --> Removed: 拒绝等待者 / 清理 / close / 解除订阅
  Removed --> Discovered: 再次发现时创建新对象
```

该图不证明睡眠恢复、资源释放时序或旧回调竞争已在实机通过。

## 3. 编辑到设备配置的生命周期

```mermaid
stateDiagram-v2
  [*] --> Editing
  Editing --> Compared: stageConfig
  Compared --> Unchanged: 与本地保存配置相同
  Compared --> Countdown: 存在差异
  Countdown --> Countdown: 新修改取消旧任务并重新计时
  Countdown --> Writing: 计时结束
  Writing --> Checking: SmartActions按需写 + keymap写
  Checking --> Stored: fs.list摘要一致且本地保存成功
  Writing --> Failed: 发送失败或取消
  Checking --> Failed: 摘要/保存失败
  Failed --> Editing: 本地配置回退路径
  Stored --> Editing
  Unchanged --> Editing
```

IA-06、07、13、14。前端 `lV=3`，tick 将倒计时递减到 0 后在下一次 tick 执行发送；不能把常量 3 直接作为“精确 3 秒端到端更新延迟”。主界面 effect 在配置完成/layout 阶段调用 `stageConfig`，首次 effect 被跳过。

Profile 下拉选择处理器同时调用所选和所用 Profile 的 setter，并把层选择设为 0。因此已有“切换 Profile 名称也会触发更新”的观察有代码线索支持；这不是一次新的设备行为验收。

`changeDeviceConfig` 的 IPC Promise 首先返回 keymap 写入的取消 ID；发送结束另外发布 `connDeviceOnChangeDeviceConfig`。独立客户端必须区分“请求已受理”和“提交已验证”。

失败分支尝试回退的是前端本地配置记录。所读调用路径没有在这里重新把完整旧配置写回设备；不能称作硬件回滚。Smart Actions 写成功而 keymap 失败时，可能处在跨文件部分更新状态，这是从顺序推得的风险，未以实机制造故障验证。

## 4. 宿主事件与上下文

```mermaid
sequenceDiagram
  participant OS as macOS
  participant F as FocusAppService
  participant K as 键盘
  participant C as Comm通知路由
  participant N as DeviceNotifyService
  participant X as Native/Window服务
  F->>OS: 每轮获取前台应用
  OS-->>F: 应用信息
  F->>K: 发生变化且未锁屏时 host.focused_app
  K-->>C: kb.sa.* / kb.radial / kb.cs.*
  C->>N: deviceId + method + params
  alt kb.sa.exec
    N->>N: 检查smartActionCmdEnabled及cmd字段
  end
  N->>X: 插入文本 / 打开应用或URL / 显示浮窗
  Note over K,C: 通知字典来自包内处理器，非本轮真实按键抓包
```

IA-09–11。前台应用检测循环在每次采集后延时约 1000 ms；变化按 `appName/process/path` 比较。与 UI 中的“切换应用后采集五次”是两个流程，不能把五秒当成后台自动切换阈值。主机上报上下文，固件如何依据 `linkedApps` 选择层仍需固件或实机证据。

普通宏序列和 Multi Action 四分支被编码进 keymap（IA-12）；宿主的 Smart Action 执行器只处理相应通知。由此可以区分“发送给设备的动作描述”与“由宿主实际执行的系统操作”，但不能仅凭代码证明所有普通宏都能脱离 Input 完整运行。

## 5. 业务覆盖与尚缺的连接

| 场景 | 本轮恢复的路径 | 缺口及下一项证据 |
| --- | --- | --- |
| Profile | 前端选择/使用状态 → stageConfig → keymap → 摘要验证 | 设备实际激活时刻、失败恢复；需受控读回 |
| Layer | Layer 对象 → layout/lights/linkedAppId → Profile 序列化 → keymap | UI 选择和固件活动层并非同一状态；切层规则需固件或实机 |
| 普通键 | layout.keymap 中的绑定 → keycode 转换 → 文件发送 | 矩阵扫描、去抖和 OS 输入报告在固件侧；需物理输入与报告对照 |
| 旋钮 | layout encoders 绑定 → 文件发送 | 顺逆方向、步进和按压对应关系需逐控件实测 |
| 普通宏 | actions 步骤 → kc/delay/act → macros → keymap | 设备实际计时和执行次序需验证，不能只读序列化器 |
| Multi Action | 四分支 keycode 与 tt → multiActions → keymap | 固件手势判定及边界时间需测试 |
| 持久灯光 | backlight/underglow → Layer.lights → keymap | effect/brightness/speed/magic/color 的值域及生效时刻未知 |
| 灯光预览 | RPC Channel → sendLightingPreview → lights.preview | 即时效果与非持久性目前仅 SDK 意图；恢复需实测 |
| AppSense | 原生前台采集 → 变化比较 → host.focused_app | 固件根据 linkedAppId 的匹配和切层行为未恢复 |
| 径向菜单 | kb.radial → 本地配置索引 → 浮窗显示/更新 | 最终动作触发边、字段值域及物理手势待补 |
| Cheat Sheet | kb.cs.* → 本地配置索引 → 浮窗 | 是否与当前设备配置一致需读回对照 |
| 宿主 Smart Action | kb.sa.* → 参数检查/命令许可 → 系统接口 | 实际通知产生条件、焦点和系统权限需运行时验证 |

普通键和旋钮的虚线边界是明确未恢复的固件行为，而不是省略的已知实现。两区灯光字段来自 IA-17；未从 UI 默认值推断固件参数范围。

## 尚未签收的部分

- 没有运行时 hook 或物理总线包，本轮无法确认任何新发现的写方法在固件 0.6.3 的行为。
- 没有解析固件内部状态机；BLE、原生模块内部、物理开关与键帽位置仍不在本轮证据覆盖内。
- 真正的按下/释放原始通知未由上述 `kb.sa.*` 字典证明，不能把宿主命令通知转换成物理键事件。
- 多宿主同 ID 冲突、取消时已写入的部分数据、断连后的设备恢复仍需独立实验。
- 全包映射覆盖为 partial；本文手工追通的是表中关键业务边界，不是所有程序路径。
