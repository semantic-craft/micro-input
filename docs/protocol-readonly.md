# Creator Micro 2：USB 发现、身份与配置读取

状态：2026-11-02，Input 1.1.0 随包静态分析 + 本机只读 IOKit 枚举。可据此实现只读客户端；**尚未完成独立客户端与固件 0.6.3 的 RPC 收发验收**。本轮未打开 HID 句柄、未发送设备请求、未启动或操作 Input，也未读取用户宏载荷。

后续核验（2026-11-02）：独立 Swift / IOKit 临时探针已通过 USB 的 `sys.version` 读回 **0.6.3**，与官方 latest 稳定版一致，无需刷机。此次只证明单个版本查询成功；配置读取、多帧、异常恢复及应用端到端验收仍未完成。上段及 E01–E09 的“本轮”指此前静态调查。新增社区证据、差异和实机待验项见第 7 节。

## 适用范围与证据

- 目标：本机 `/Applications/input.app`，Info.plist 版本 1.1.0；随包 `@worklouder/wl-device-kit` 0.2.13。
- ASAR SHA-256：`a6c7185d5c3d8dda1262437a9c0740080cceaea5585dd289b8eef0668b8a4206`。名称和版本相同但摘要不同的包须重新核对。
- 原始分析、提取文件、哈希清单只留在 Git 忽略的 `.scratch/input-study/rea-1.1.0/`；不交给实现者，不提交原应用代码。
- `rea-evidence.json` 保存完整 REA Evidence。主进程/worker 的父 Evidence：`ev_8f627c64d7d6f37b51498c67f87e0fb2f7d10517c0b16f0f4132dba45f21d752`；device-kit：`ev_da2accd4ffc5965bb95ebf9f7c7aacf4b04dafd3dea628175a4e6cc8247bdc10`。
- REA 分析范围为上述入口和 kit，语义图受节点上限影响且有动态调用未知项；下文具体结论另经对应文件逐段阅读。不能把图覆盖当成全部调用已追通。
- REA 发现 27 个 unpacked 原生模块与 ASAR 声明哈希不一致，使用 `record-and-continue` 保留该事实；原因未确定。本文依据的提取 JS/JSON 文件均另行核对 SHA-256，与 ASAR 声明一致。这不等于证明整个应用可信；未执行或反编译原生模块。
- kit 标注 `UNLICENSED`。这里只交付用自然语言整理的接口行为与自定实现要求，不复制其实现。该调查不是严格 clean-room 隔离证明。

证据标记：**O** 本机只读枚举；**S** 当前包的静态代码事实（不代表设备已执行）；**D** 自研设计要求；**U** 尚未核实。下文源码定位均相对原 ASAR 根目录，行号对应原始提取文件、未格式化。

| 证据 | 文件及行号 | 支持内容 |
| --- | --- | --- |
| E01 | `dist-electron/workers/search_devices_worker.js:1–72` | 事件监听、合并扫描、HID/serial 分开回传 |
| E02 | `node_modules/@worklouder/wl-device-kit/dist/index.js:5368–5425,5505–5538,5655–5750` | VID/PID、usage page、USB 判定、Mac 监听包装 |
| E03 | 同上 `:5937–5967,6079–6100,6145–6230,6268–6309,6341–6418` | 非独占打开、分帧、换行、响应分派、队列与超时 |
| E04 | 同上 `:4237–4255,4330–4413` | JSON 请求、Unicode、ID 和响应校验 |
| E05 | 同上 `:4476–4488,4549–4605,4721–4755,4906–4945` | 状态、版本、文件列表与分块读取 |
| E06 | `dist-electron/main/index.js:5849–5880,6453–6605,6638–6649` | 每设备通信实例、连接后状态查询与身份键 |
| E07 | 同上 `:6849–7068,7178–7188` | 文件筛选、缓存比较、读顺序、超时与 SHA-1 |
| E08 | 同上 `:2897–2904,2948–2957,13164–13243,13287–13319,13513–13589` | 配置解析、token 转换、ID 补齐、迁移边界 |
| E09 | `.scratch/input-study/rea-1.1.0/manifest.json` | 本轮时间、原件摘要、提取文件校验、脱敏 IOKit 结果 |

## 1. 设备发现与身份

**O/E09**：本轮系统注册表实际枚举到 Creator Micro 2：VID `12346 / 0x303A`，PID `33432 / 0x8298`，Transport `USB`，VersionNumber `0`。同一产品有三个 HID 接口：usage page `1`（键盘）、`12`（consumer）、`65280 / 0xFF00`（vendor）；vendor 接口 PrimaryUsage 为 `1`。未保存序列号和设备路径到公开文档。IOKit VersionNumber 不是固件版本；0.6.3 来自前轮 Input UI 观察。

**S/E01–E02**：Input 在 worker 中枚举 `node-hid.devices()`，并监听系统设备变化；macOS 包装层对变化做 175 ms debounce。worker 将扫描请求合并，避免并行扫描。native listener 内部没有分析。

HID 筛选顺序：

1. 优先保留 VID `0x303A` 且 manufacturer 包含 `Work Louder` 或 `Work_Louder` 的条目。
2. 若这批结果为空，退回 VID-only 候选集。
3. 要求 path、productId 存在，usagePage 为 `0xFF00`，PID 在注册表中。
4. `0x8297 / 33431` 和 `0x8298 / 33432` 都归为 `creator_micro_v2`，布局 `universal`；不能据此推定两者对应 Base/Pro。当前实机只确认 `0x8298`。
5. 原应用以 `(release & 3) == 0` 标识 USB，其他为非 USB。这是应用启发式，不是独立的 USB/BLE 规范；本机 USB 另有系统 Transport 证据。

**S/E06**：每个设备有自己的 transport、RPC API 和队列。连接去重键由 PID、trim 后序列号、connectionType、deviceType、layoutType、portPath 组成。PID 本身不能当物理设备唯一标识。

**D**：首版仅接受上述 VID、实机 PID `0x8298`、vendor usage page 和系统 USB transport；显示真实身份，不把普通键盘接口当 RPC 通道。`0x8297` 留作静态已识别、未验收型号。使用非独占打开；打开失败直接报告，不抢占设备、不关闭当前宿主。macOS 原生后端的运行循环寿命由长寿命对象持有。

## 2. HID 帧与 JSON 请求

**S/E03**：macOS 路径调用 `HIDAsync.open(path, {nonExclusive: true})`。下表是 **node-hid write 的 64 字节缓冲区布局**，不是已经验证的 IOKit 调用参数。

| 偏移 | 长度 | 内容 |
| --- | --- | --- |
| 0 | 1 | report ID = `0x06` |
| 1 | 1 | channel = `0x02`（RPC）；接收端另识别 `0x01` debug |
| 2 | 1 | 本片 payload 字节数，最大 61 |
| 3 | 0–61 | JSON UTF-8 字节的连续切片 |
| 片尾 | 剩余 | 补零至 64 字节；补零不属于 JSON |

长请求按字节连续切片并顺序发送，每片独立等待 write 完成。没有在此帧格式中看到分片序号或最终片标记。**JSON 请求不追加 LF 或 CRLF**；不要把 legacy serial 的 CRLF 混入 HID 请求。

**S/E04**：请求对象只有 `method`、`params`、`id`，没有 `jsonrpc: "2.0"`。默认 params 为 null；ID 生成范围为整数 0–998。该范围是当前客户端选择，注释称其受固件限制，但固件极值未实验核实。非 ASCII 字符经 Unicode 转义，补充平面字符使用代理对。

以下为合成请求，ID 仅为示例：

配套 [protocol-vectors.json](protocol-vectors.json) 给出完整十六进制帧、分片边界及合成 Base64/SHA-1 样本，可用于实现时编解码测试。它们由本文规格独立生成，未执行原应用模块，不是设备抓包或固件认可的有效配置。

```json
{"method":"device.status","params":null,"id":1}
{"method":"sys.version","params":null,"id":2}
{"method":"fs.list","params":{"checksum":true},"id":3}
{"method":"fs.readbin","params":{"file":"keymap.json","offset":0,"len":3072},"id":4}
```

**S/E03–E04**：接收端按 buffer[1] 的通道分别累积 buffer[3..3+length]。以 LF 或 CRLF 分行，完整行进入 JSON 解析；不能假定一次 HID 回调就是完整 RPC 回应。response 的 `id`（或紧凑 `i`）匹配请求；无 ID、有 `method`（或 `m`）的消息作为通知，参数接受 `params` 或 `p`。RPC 包装层要求 `result` 与 `error` 恰有一个；error 至少有数字 code、字符串 message。

**D**：实现接收时先验证长度、report ID 和通道，按字节重组后再 UTF-8 解码；避免跨帧 Unicode 被逐片解码损坏。未知通知不执行宿主动作，不阻塞请求。不同请求的迟到响应不得误配；超时/断连后清空所有通道、取消待处理请求。不要照搬原客户端宽松校验或缓冲区残留行为。

**U**：直接 IOKit 与 Rust HID 后端是否将 report ID 单独传入、回调是否保留 report ID，需分别实测。本文的 64 字节 API buffer 不能机械转换为“IOKit 一定发送 64 字节”或“线上固定 63 字节”。BLE 不在首版验收范围。

## 3. 状态与版本查询

**S/E06**：Input 连接后等待约 1000 ms，然后查询 `device.status`；成功即采用返回状态，失败才对 HID 调用 `sys.version`。不把两者误写为必定依次调用的握手。

| 方法 | params | result 字段及当前客户端接收类型 |
| --- | --- | --- |
| `device.status` | null | 对象；`version?: string`、`profile_index?: number`、`layer_index?: number`、`battery?: number`、`is_charging?: boolean` |
| `sys.version` | null | 对象；必须有 `version: string` |

**S/E05**：状态字段均可缺失，甚至空对象也能通过该包装层；应用随后可能拿不到版本。字段数值范围、索引基数和缺省意义未由此次静态分析确认。

**D**：缺失字段显示“未知”，不能默认为固件 0、层 0、电量 0 或未充电。固件身份必需而状态缺少 version 时，自研客户端可再查 `sys.version`，这是明确的设计补充。保留 profile/layer 索引和配置对象 ID 两套值，未验证前不相互替代。

## 4. 从设备读取配置文件

**S/E05–E07**：Input 的 file service 在连接事件后调用 `fs.list`。该服务与连接后状态查询是事件驱动关系；不能仅由源码排列推断二者在运行时有固定完整顺序。自研客户端采用下文串行流程。

`fs.list` 默认发送 `{"checksum":true}`；可选的 `rec`、`path` 未指定时不会进入 JSON。result 是数组，每项包含 `name`、`size`、`checksum`。包装层只验证数组，不严格验证字段。主应用过滤数值 0 大小、名字 `null` 和以 `tmp` 开头的条目；字符串 `"0"` 不符合这段严格等值过滤。

原应用要求列表包含 `keymap.json`，按本地缓存的文件名/校验和判断是否重读；顺序优先 `smart_actions.json`，再 `keymap.json`。这意味着原应用 UI 可展示复用缓存，不能作为每次都完成设备读回的证据。首版只读取这两个配置文件，不下载无关壁纸或目录。

实际配置读取使用 **`fs.readbin`，不是直接 `fs.read`**：

1. 初始 offset=0、len=3072。result 必须含 `total_size`（非负 number）和 `data`（Base64 string）。
2. 解码 data，追加原始字节；offset 增加实际解码字节数，不能按 Base64 字符数递增。
3. 剩余字节少于 3072 时缩短下一次 len；offset 达到 total_size 后结束。
4. 未到 EOF 却收到空数据，原客户端报无效响应。空文件在 offset=0、total_size=0 时可结束，但原 file service 的列表过滤另有限制。
5. 原客户端每轮读取等待 50 ms；transport 队列每请求结束另有 50 ms cooldown，两者可能重叠，不能简单相加为固定 100 ms。
6. transport 单请求超时 10 秒；主应用对两份配置的整文件读取另设 20 秒超时并清队列。这些是应用策略，不是固件保证。

**S/E07**：主应用用 SHA-1 对原始文件字节计算摘要，并与文件列表 checksum 比较（迁移后读回和配置写入核验路径可见）。普通初次读取路径未显式逐文件重新计算并比较，而是保存收到的字节和列表给出的 checksum。

**D**：自研读取始终以设备字节为准：先 list → readbin → 原始字节 SHA-1 校验 → 再 list。若前后 checksum/size 变化，标记快照不稳定、丢弃本次结果；不得用本地旧缓存伪装成功。不稳定或校验不符不触发自动修复/写回。此策略不能保证跨文件原子快照，只能检测所检查的变化。

**D**：为文件和累计响应设合理可配置内存上限；检查 total_size 在各块间不变、偏移/长度为整数、Base64 合法、解码长度不超过请求 len 和剩余大小。对 size 的字符串/数字兼容必须显式解析并拒绝歧义值。未知或缺失 checksum 应报告无法校验，而非按成功处理。

## 5. 只读配置模型的实现注意点

**S/E08**：`keymap.json` 与 UI 数据模型不完全相同。首版保留原 JSON 和未知字段，另建显示投影，不做正规化后写回。

- 读取 `profiles[]`，每个 Profile 有 id、name、layers；Layer 包含 id、name、color、layout，以及可选 os、lights、linkedAppId。
- Layer 的设备 color 是数值，原 UI 转为六位十六进制颜色字符串；不能把 UI 字符串格式直接当设备格式。
- layout 的 keymap 为二维 token 数组；encoders、buttons 可选；joystick.sectors 的键引用字段为 `k`。物理行/旋钮槽位次序仍需后续实机动作核对。
- 原 UI 投影把设备 token 前缀 `KA_A` 改为 `KA_`、`KA_M` 改为 `KM_`、`KC_FUNC` 改为 `KI_FP`；原始 token 必须另存，未知 token 原样显示。此规则不是完整键码表。
- 文件中的 `macros` 对应 UI actions，`multiActions` 对应 multiactions；组字段兼容 `multiActionsGroups` 和旧名 `multiActionGroups`。不推定数组位置就是对象 ID。
- `smart_actions.json` 的 `smartActions` 是以 `SA_<非负十进制整数>` 为键的对象，原 UI 解析后按数字 ID 排序转换为数组。它可以不存在；缺失与存在但为空应分别记录。
- 原应用会在内存补缺失 Profile/Layer ID、删未引用 linkedApps。自研只读客户端不改原始快照，最多在显示层给出临时标识及异常提示。
- 原应用的通用 parseKeymapFile 路径含迁移后写回分支，但本包 getKeymapMigration 只列 Knob F1、Nomad V2，**没有 Creator Micro 2 分支**。不能宣称这台 Micro 会因该迁移而自动写入；也不能复用整条通用加载流程当纯读取 API。

## 6. 首版执行契约与待验项

**D：允许设备命令仅为** `device.status`、`sys.version`、`fs.list`、`fs.readbin`。其中 readbin 只允许上述两份配置文件。枚举和接收通知均不能触发配置同步、前台应用上报或 Smart Action 执行。HID Output report 用于发送读请求，不等于修改设备配置；验收以实际 method/params 白名单约束。

建议流程：枚举 USB vendor 接口 → 非独占打开 → 状态/必要时版本 → 文件列表 → 逐文件读与校验 → 重查列表 → 展示快照 → 关闭会话。连接被占用或没有回包时记录失败，不能用 reset、bootloader、切 Profile、重写配置解决。

| 验收 | 预期 | 本轮状态 |
| --- | --- | --- |
| 当前设备枚举 | VID/PID/USB/vendor 接口吻合 | 已完成只读 IOKit 枚举 |
| 64 字节帧切分 | 61/62/122/123 字节边界、尾部补零、UTF-8 字节计数 | 合成向量已生成并离线自检；不是原模块差分测试 |
| 回应重组 | LF/CRLF、跨帧、多个消息、通知插入、未知 ID | 实现后验证 |
| 请求限制 | 无换行、只读方法、文件名白名单、单请求在途 | 实现后验证，需捕获真实出口 |
| 状态读取 | 固件值、可选字段、错误回退、迟到响应 | 待实机 RPC |
| 文件读取 | 多块、空块、非法 Base64、大小漂移、checksum 不匹配、缺少文件 | 实现后离线及实机验证 |
| 连接生命周期 | 非独占、断连取消、重新连接、Input/Codex 共存 | 待实机；本轮未改变宿主运行状态 |
| 不改变配置 | 所有出口仅白名单读命令；读取前后校验和可比 | 待客户端验收；本轮未发送任何 RPC |

## 7. Dispatch 补充证据与当前固件核验

**C（社区观察/实现）**：Dispatch 主版本固定在提交 `6c6770a67f640f0eefbad03aff3c43e254b52239`。它的[互操作文档](https://github.com/thefotes/Dispatch/blob/6c6770a67f640f0eefbad03aff3c43e254b52239/docs/protocol/creator-micro-2.md)主要基于作者的固件 0.6.2 实机，BLE 观察日期为 2026-10-20。以下不是官方保证，也不是本机验收。代码复用受其 [MIT 许可证](https://github.com/thefotes/Dispatch/blob/6c6770a67f640f0eefbad03aff3c43e254b52239/LICENSE)约束，保留版权与许可；本次只整理自然语言结论，没有移植代码。

| 补充证据 | 对本项目的设计含义 | 本机仍需验证 |
| --- | --- | --- |
| C：vendor collection 位于 device usage pairs，primary usage 却是键盘；使用 DeviceUsagePage/DeviceUsage 匹配 | 枚举检查完整 usage pairs，不能只依赖 PrimaryUsage；仍保留 VID/PID/USB 约束 | 与 E09 的三接口观察核对，区分 OS 枚举方式和固件差异，不把一个接口形态写死 |
| C：非独占打开；IOHIDManager 须存活到连接结束 | 会话持有 manager、回调缓冲区及监听生命周期 | 断连、退出及重新打开后的回调和资源释放 |
| C：缺 Input Monitoring 或 Secure Input 均可能返回 0xE00002E2；权限与签名关联 | 区分权限、被保护输入阻塞和其他 I/O 错误；不自动退出应用或改变系统权限 | 我们交付签名下的真实错误路径；枚举阶段仍不请求权限 |
| C：64 字节含 report ID 0x06、channel、length、最多 61 字节；channel 2 为 RPC | 与当前静态帧定义相互印证 | 各后端回调是否含 report ID；多帧及跨帧 UTF-8；本次仅验了短版本请求 |
| C：按完整顶层 JSON 对象重组；源码发送 m/p 紧凑字段 | 与 Input 的 LF/CRLF 行解析、method/params 长字段分别验证，不能直接互相替换 | 连续 JSON、行结束、通知插入、畸形对象的实际返回边界 |
| C：fs.read 的参数是 file；作者在 0.6.2 读到编码 JSON 字符串 | 这是另一条社区已观察路径，不取代本项目 fs.readbin 分块与校验方案 | 0.6.3 的 readbin、文件大小/校验和、分块一致性；fs.read 不自动加入 MVP 白名单 |
| C：忽略未持有 ID 的响应，ID 小于 1000 是 Dispatch 自定约定 | 自研应处理外来和迟到响应；小 ID 范围不是固件规格，也不能保证跨进程不碰撞 | 与 Input/Codex 共存、同 ID 外来响应、超时后迟到响应和新会话隔离 |
| C：13 个开关的行宽为 [2,4,4,3]，宽键跨开关 10/11，物理键为 12 个 | 查看器区分物理控件与开关；后续事件层合并宽键状态 | 本机物理位置、事件先后、去重及释放行为 |
| C：AG13/14 为旋钮方向，AG15–18 为摇杆下/左/上/右；动作含按下与释放 | 后续宿主动作不能把释放误当第二次动作 | 当前固件与配置下的方向、按压及事件；本次不发送绑定写入 |
| C：只有对应 KV_OAI_AGnn 活动绑定才能收到 v.oai.hid 并驱动逐键灯；Codex 也接收同类事件 | 专用绑定与普通按键分开显示；多宿主执行可能重复 | 不同活动层与 Codex 共存下的通知和灯光所有权 |
| C：逐键灯与 ambient/keys 区域参数、效果表现不完全相同；RPC 返回 ok 不代表灯实际变化 | 普通层灯效与 v.oai.* 灯光 API 分开建模，不借用 UI 序号 | 效果、速度、亮度、Magic 和键 12 灯的实际表现；留到授权灯光阶段 |
| C：BLE 与 USB 共用 channel 2 RPC，三槽分别配对；插拔 USB 后可切换传输 | 后续 BLE 可复用编解码，但传输身份与重连单独验收 | USB/BLE 切换、双候选去重和重连耗时；不扩入 USB MVP |

源码定位：[Transport.swift](https://github.com/thefotes/Dispatch/blob/6c6770a67f640f0eefbad03aff3c43e254b52239/Sources/DispatchCreatorMicro/Transport.swift)、[HIDFraming.swift](https://github.com/thefotes/Dispatch/blob/6c6770a67f640f0eefbad03aff3c43e254b52239/Sources/DispatchCreatorMicro/HIDFraming.swift)、[RPC.swift](https://github.com/thefotes/Dispatch/blob/6c6770a67f640f0eefbad03aff3c43e254b52239/Sources/DispatchCreatorMicro/RPC.swift)。`fs.write`、逐键灯光、配置 provisioning 与恢复仅作为后续设计参考，不属于当前读命令白名单。

**O（本机新增核验）**：2026-11-02，macOS IOKit，VID `0x303A`、PID `0x8298`、Transport `USB`，按 device usage pair `0xFF00/1` 匹配到一台设备，非独占打开。仅发送一次 `sys.version`（长字段 method/params、独立 request ID），收到匹配 ID 的 `version: 0.6.3`。原始输出及一次性探针留在 `.scratch/firmware-check/`；没有读取配置、刷机、切 Profile 或发送灯光命令。不能将此扩写为全部 RPC 或不变配置读回已验收。

**F（官方发布事实）**：同日查询[官方 latest](https://github.com/worklouder/cm-v2-fw-releases/releases/latest)为 [v0.6.3](https://github.com/worklouder/cm-v2-fw-releases/releases/tag/v0.6.3)，`prerelease=false`；[v0.7.0-rc.16](https://github.com/worklouder/cm-v2-fw-releases/releases/tag/v0.7.0-rc.16)为预发布，不是升级目标。遵循用户选择：保持最新稳定版，未来操作前重新查询发布状态并保留既有映射。发布说明可提示功能差异，但不能代替协议规格。

实现交接仅提供本文及合成向量说明；不得导入原应用模块充当自研核心。下一步可据本文实现编解码器与只读 transport，再在明确可控的设备会话中验证。尚未核实 native HID 适配细节、完整键码/容量、固件实际错误码以及跨文件一致性，不把这些缺口补成假定规格。

## 8. 当前独立后端与身份应用验收

E-075核对Cargo.lock锁定hidapi crate 2.6.7的macOS后端：编号报告0x06保留在64字节buffer首部，IOKit另传相同report ID；入站callback buffer原样保留，不再拼ID。`macos-shared-device`开启非独占；该版使用run loop并在close中join原生read thread，不适用dispatch cancel-handler实现。生产adapter只为303A:8298/USB/macOS启用，其他组合仍Unverified/no-send。

E-077受控本机观察补充：固件0.6.3回应包含`id/method/result`，method echo本次只读查询。自研RPC响应必须有合法id/i、result/error恰一项；可选method/m只允许四项只读查询名并与pending method匹配，不能带params/p。仅含method+id而无result/error的设备请求不满足pending，只读阶段不执行、不回复。RPC错误不伪装为成功。

E-078已在原生应用路径查询到0.6.3（device.status），可选活动索引与本地浏览选中分开显示。连接、身份和配置读取独立；当前配置schema仍Unverified，不会导入演示解析器。任意长响应/UTF-8、完整配置、拔插/睡眠、权限错误、同ID外来回包与共享channel共存尚待对应实机验收。

## 8. 输入监视的当前边界

`DeviceSession` 的唯一 reader 把无 ID 通知分流到 256 项有界队列。`subscribe_input()` 及 UniFFI `InputSubscription.next_event()` 复用它，不发送轮询请求。SwiftUI 输入监视只显示本地会话、订阅序号、时间、通知数量和缺口数量，不返回、显示或记录通知载荷。

当前生产 `UnverifiedInputDecoder` 不识别实体按下/释放、旋钮方向或摇杆。所有通知都返回 `UnknownNotification`、空逻辑引用和 `input-schema-unverified`，禁止动作执行；宿主动作名称也不能充当物理输入证据。物理控件、键帽覆盖和动作角色保留三个独立的空引用，不从 13 个逻辑位置推导实体键数或宽键去重。`InputDecoder` 仅提供未来有证据解码的 Rust 边界；合成测试里的逻辑开关不属于设备协议。

订阅队列溢出返回 `InputStreamGap` 和丢失数，清除其按住状态，停止该订阅全部后续输入。用户必须明确重建监听；新订阅丢弃旧积压并从空状态开始，生产解码仍保持未验证，不能借重建开放动作。关闭订阅会唤醒等待，断连终止等待并拒绝旧会话缓冲事件；窗口停止 UI 订阅不关闭应用设备会话。

这只交付监视基础。#14 的真实按下/释放、旋转方向、按压、摇杆回中、全部实体映射与宽键去重仍待当前配置和固件的独立证据及实机核对；本实现没有改变 keymap、准备最小绑定或写入设备。

## 9. 当前配置读回与响应归属

E-079/E-081：当前0.6.3/USB8298应用已通过显式身份→fs.list→smart/keymap分块→原始字节SHA-1→再次list，并在SwiftUI只读浏览实际Profile/Layer及逻辑开关。设备原字节仅保留在内存快照，和本地草稿分开；合成响应来源为Imported，不冒充设备。未知动作/字段/物理方向仍保留Unknown/raw，不从设备能返回JSON推定全部语义已验证。

读取限额为本地策略：每文件4MiB、总量8MiB、每块最多3072字节、单请求10秒、整次60秒、无自动重试。失败/取消保留旧字节和时间并标stale；smart缺失、零字节、合法空对象分别处理。前后list一致不证明跨文件原子性或无ABA变化。

接收循环先对完整JSON提取合法响应ID；非pending回应丢弃，不以其未知echo终结当前会话。只有当前请求的回应继续接受第8节严格echo/result/error验证；带method/id但无result/error的消息仍为未执行通知。这没有解决同ID跨进程冲突或共享channel写流交错，UNKNOWN-17保留。
