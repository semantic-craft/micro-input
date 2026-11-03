# Input 1.1.0：配置写入与宿主通知候选协议

本页记录安装包静态分析及明确标记的公开社区候选。字段和方法存在于客户端，不等于当前键盘固件支持。本轮没有发送这些操作。证据身份及 IA 定位见[架构说明](input-architecture.md)；已有只读请求见[只读协议](protocol-readonly.md)。下面没有任何方法因此获得生产写入授权。

## 通信外壳

请求为 `{method, params, id}`，不是带 `jsonrpc: "2.0"` 的标准声明。SDK 使用 0–998 请求 ID。HID 写入 API 缓冲共 64 字节：report ID `0x06`、channel `0x02`、本片长度、至多 61 字节 UTF-8 分片，余下补零。发出 JSON 不额外追加换行；接收以 LF/CRLF 分行再解析。物理总线和 IOKit 参数边界不得从 node-hid 缓冲直接推定（IA-04）。

取消 ID、RPC 数字 ID 和设备对象 ID 是三个不同命名空间。`writeFileChunkedFromStr` 给调用者 UUID 作为取消入口，内部每块另有 RPC ID。取消会拒绝等待中的任务或移除队列项，没有撤销已经生效的设备写入（IA-07）。

## 文件与写入

| 方法 | 当前 SDK 发送字段 | 当前证据能说明什么 |
| --- | --- | --- |
| `fs.write` | `{file, data}` | 小文件接口存在；普通配置发送路径使用下面的分块接口 |
| `fs.writebin` | `{file, data, append:true, completed, offset}` | data 为 Base64；原始字节块为 3072 字节，末块 completed=true；offset 是原始字节偏移 |
| `fs.writebin` 二进制辅助路径 | 上述字段加可选 `tx` | `writeFileChunked` 接受事务 ID；不同于常规配置的 FromStr 路径 |
| `fs.txbegin` | SDK 调用未指定 params | 包装器要求回应 `result.tx` 为 number；未证明值域或事务隔离 |
| `fs.txcommit` | `{tx}` | 提交方法存在；没有据此证明失败时自动回滚或存在 abort 方法 |
| `lights.preview` | 直接传入 config | SDK 注释称即时、不持久化；未由本轮设备读回证明 |
| `host.focused_app` | 直接传入前台应用对象 | 主机向设备发送上下文，不是本机直接切层的证据 |

普通配置链为：准备配置副本与期望摘要 → 按需写 `smart_actions.json` → 写 `keymap.json` → `fs.list` → 对比文件名与摘要 → 保存本地设备配置。所读 `sendConfigData` 未调用 txbegin/txcommit。SDK 的 `sendMultipleFiles` 是另一条辅助路径，不能替普通配置链补上事务语义（IA-06、07）。

比对的是设备文件列表中的摘要，不是本轮代码每次重新下载全文件的逐字节读回。验证后写本地文件缓存的 Promise 不等待，数据库配置保存结果则参与最终成功判定。缓存失败、本地配置保存失败和设备写入失败不应合并为同一种状态。

## 原生候选复核（2026-11-03，E-105）

本机安装包与 IA 表摘要一致；本轮未启动 Input 或打开设备。REA 定向 SDK 分析 parent 为 `ev_c76b45b73180338d6abc45f917e545bd5eeedb7c2b5bd0ad279ccbc66eadb57b`，独立 RGB 核对 parent 为 `ev_7438305d76a1239544a49fcb3af9fbd714bc1868aa3b6db247e779c9d154aa25`。图覆盖 partial、语义节点触顶；ASAR 中若干原生插件有完整性元数据矛盾，本次仅使用摘要匹配的 JavaScript 文件，不据此信任插件或执行原应用。以下结论以指定源码段复核为依据，不能从图中未命中推断固件没有其他方法。

| 已核对内容 | 定位与结论 | 实现及未解锁边界 |
| --- | --- | --- |
| 通用成功包络 | K:4330–4388：对象必须恰有 `result/error` 之一；成功不限制 result 值。K:4787–4864、5175–5182 不再检验方法专用结果 | `validate_mutation_response_candidate` 检查本项目的 ID 归属及成功包络。`null/false/object` 不代表写入、灯光已生效或持久化；独立读回仍必须 |
| 分块构造 | K:4787–4864、5333–5346：UTF-8 原始字节每3072字节切分，Base64、原字节offset、最后块completed=true；常规字符串路径无tx | 自编向量覆盖整块、跨块中文/emoji及最后标记。空字符串在官方方法里零请求但返回客户端成功；本项目拒绝空候选，不把零请求当作清空文件 |
| 原配置校验 | M:13326–13412：写后按文件名和列表摘要比较；保存本地失败与设备写入是不同结果 | 本项目保留更严格的完整原字节读回。模拟“成功包络但未写/损坏”和“写后丢失/错误响应”不能标Verified，未知结果不自动重放 |
| RGB请求 | K:5175–5182；M:2015–2083、13932–13935：两区DTO直接作为params，只按设备对象转发 | `lighting_candidate` 只编码已知可选字段，不补默认、不增加目标或恢复字段。范围为客户端策略；后续原生接线遵循下节设备级语义 |
| 活动来源 | K:4565–4602 有可缺省的 profile_index/layer_index，SDK仅检查number | 会话已有严格非负整数解析。前后查询不能原子绑定预览，中间切层仍有竞态；文件activeProfileId不能代替运行态 |
| 关闭和恢复 | 前端 R 的灯光面板字节6825228–6826455：500ms debounce发送两区；hook字节6824496–6824710卸载仅取消待发，关闭入口约6830188只隐藏面板 | 该链未读运行态或发送恢复。SDK方法枚举K:4439–4466未列出lights.get/state/begin/end/restore；此范围内未找到，不断言固件不存在 |

上述 R 为未格式化提取物，SHA-256 `b3ee40ec883df9a7c8c8744433847abe585b5cd1d1d01c1063295abe2eb23329`；字节范围为零基 UTF-8 左闭右开，不能套用其他格式化副本的行号。

对摘要固定的 SDK 选定函数进行了一次受控离线重放：只加载上述函数体，在无模块导入的 VM 上提供内存 RPC 桩，未启动 Electron、加载原生插件或连接设备。输出验证了通用包络、3072字节切块、空字符串零请求和两区预览直传。SDK辅助结果/通信由桩提供，因此这是客户端函数行为证据，不是完整 SDK 或固件运行证据。脚本、源码位置、摘要和结果仅保存在 `.scratch/input-study/native-protocol-20261103/`。

文件覆盖/截断和最小配置恢复的后续实机结果见 E-108。即时预览的固件实际效果、外部客户端竞争与断连/超时的执行顺序仍需实机验证。当前合成 `synthetic.lights.read/set` 的原子比较与恢复能力不属于已发现的原生协议，不能通过改方法名移植。

### 设备级预览与缺失灯光（2026-11-04）

沿用 Input 1.1.0 同摘要原件，仅补查 IA-18；未启动原应用或连接设备。REA 定向 renderer 分析 parent 为 `ev_e16cfebcfc9d6ae911920b34f9904dfe348f4bbeb9a3b3e9a99c63564e5c7401`，静态图仍为 partial，以下事实依据指定原字节段复核，不依赖图的未命中结果。

- R 字节171717–172004：预览管理器以 `connectedDeviceId` 转发灯光 DTO；结合 M:13932–13935 和 K:5175–5182，传输参数没有 Profile/Layer 标识。客户端请求的是在当前连接设备显示编辑中的两区灯光，不是激活所编辑的层。此结论不证明固件内部应用范围或视觉效果。
- R 字节6828000–6828450：允许灯光的层缺少 `lights` 时，官方界面取型号默认值并更新本地层。R 字节7176500–7177100：Creator Micro V2 默认 Backlight 为 solid、brightness=1、speed=0.5、magic=1、color=0xFFFFFF；Underglow 为 rainbow、brightness=1、speed=0.55、magic=1、color=0xFFFFFF。这是客户端默认值，不是从设备读取的原运行态。
- 本项目使用显式“添加灯光设置”创建缺失字段，继续沿用统一候选、备份、写后读回和恢复流程；普通浏览不初始化字段。缺失与 `null`、未知或损坏对象分开处理，后者不被默认值覆盖。
- 原生即时预览停止只取消待发更新并等待在途请求完成，不补造恢复指令；完成状态为停止发送，不能称“原值已恢复”。只有配置应用/恢复链具有完整文件读回。预览成功包络只能证明协议回应，实体颜色和效果仍待视觉验收。

## 配置格式

`keymap.json` 输出字段：`version:1`、`activeProfileId`、`language`、`profiles`、`multiActions`、`macros`、`macrosGroups`、`multiActionsGroups`、`deviceSpecificConfig`、`linkedApps`（IA-12）。不要由字段名断定 activeProfileId 是稳定 Profile ID：前端更新链将所用 Profile 索引传入它，UI 中还会以数组长度验证其范围。独立模型应保存源值并单独验证语义。

- Profile 包含 `id/name/layers`；Layer 包含 `id/name/color/layout/os/lights/linkedAppId`。
- 普通宏的步骤编码为 `{kc, delay, act}`；对象还有 `id/name/color/icon`，步骤列表名为 `actions`。
- Multi Action 编码四个字段 `kcOnTap/kcOnHold/kcOnDoubleTap/kcOnTapHold`，阈值字段为 `tt`。构造默认 250 是应用默认值，不能当作固件范围上限或下限。
- `smart_actions.json` 为 `{version:1, smartActions:{SA_<id>:...}, smartActionGroups:[...]}`。每个 action 保存 `name/icon/color/type/payload`；ID 来自对象键。解析器接受 `SA_0`，不能依据旁边陈旧的正整数注释排除 0。
- 输出 JSON 进行非 ASCII Unicode 转义；摘要应针对实际输出字节。不要在算摘要后再格式化文件。

上述为主要序列化字段，不是包含全部 layout、灯效值域和型号分支的完整 schema。

## RGB 静态参数规则

Input 1.1.0 的两区为 backlight/underglow；effect 为 off、solid、snake、rainbow、breath、gradient。brightness/speed/magic 在 UI 为0–100，协议表示为0–1；color 为24位RGB整数（例如 #123456 → 1193046）。对应 IA-17（M:2015–2083、2867–2891），效果枚举 K:5358–5366；本机补充研究为 `.scratch/input-study/rea-s3-rgb-20261103/implementation-plan.md`。

| 效果 | 客户端禁用参数 |
| --- | --- |
| Off | 全部参数 |
| Solid | Speed、Magic |
| Gradient | Speed |
| Snake | Magic |
| Rainbow | Color |

这是客户端静态规则，不是固件范围或视觉验收；未知效果和字段保留，禁用参数不自动清零。原生预览采用上节设备级、停止发送语义，不声称关闭后硬件恢复。

离线实现使用自编可变文件模拟器及明确的 `synthetic_written` ACK；它不表示固件真实回应。普通 Method/request 仍只读，候选分块编码器不能自行获得发送权限。

## 设备到主机通知

SDK 区分有 ID 的回应与无 ID 的 method 通知，并兼容部分紧凑字段。下表来自 Input 主进程实际注册和处理代码；IA-10、11。

| method | 处理器消费字段 | 主机行为与约束 |
| --- | --- | --- |
| `kb.sa.inserttext` | `text` 非空字符串 | 保存若干剪贴板格式，写入文本，调用随包粘贴 AppleScript，再尝试恢复剪贴板；不是固件逐键打字 |
| `kb.sa.exec` | `cmd` 非空字符串 | `smartActionCmdEnabled` 为真才调用 child_process.exec；默认设置为 false |
| `kb.sa.openapp` | `path` 非空字符串 | macOS 使用 `execFile("open", [path])` |
| `kb.sa.openurl` | `url` 非空字符串 | 转交 applicationService.openExternalTab；不推定任意 scheme 都可接受 |
| `kb.radial` | `a/d` 有限数字；`p/l` 有限非负数字经截断转为索引；`o` 数字；可选 `s` | `o=0` 隐藏；其他值走显示/更新，`o>1` 时可消费 sector；不推定连续的固件操作码定义 |
| `kb.cs.show` | `profile/layer` 有限非负数字经截断转为索引 | 从本地配置解析要显示的层；这是处理器容忍行为，不证明固件发送小数 |
| `kb.cs.hide` | 隐藏分支不要求配置索引 | 关闭提示窗 |
| `kb.cs.toggle` | 需要新建窗口时消费 profile/layer | 已存在窗口时关闭，否则打开 |

径向菜单初始数据取自本地配置 `profiles[p].layers[l].layout.joystick.sectors`，至少要求两个 sector，同时传入宏、Multi Action 与 Smart Actions 展示数据。因此本地缓存过期会影响浮窗内容。此代码不证明点击/松手时最终动作由谁触发，也不构成对原始按键事件协议的证明。

## 独立实现前必须解决

1. 对目标固件验证各写方法、tx 支持、失败语义、零字节文件和跨文件恢复；不从通用 SDK 外推型号支持。
2. 证明独立的写后读回与恢复机制；不能复用前端“失败时恢复本地配置”的逻辑充当设备回滚。
3. 为宿主命令、文本和路径定义自有许可与目标约束；不把收到通知等同于可执行权限。
4. 分开存储稳定对象 ID、源索引和当前活动索引；径向菜单字段与 keymap 字段名字相似不证明可互换。
5. 为真实输入建立物理事件证据；不要把 `kb.sa.exec` 等已编译语义通知伪装为通用键盘扫描码。

## 公开社区输入候选：Dispatch 0.6.2

本项目的输入监视器可识别以下**不可执行候选**，来源为 MIT 许可的 Dispatch 固定提交 `6c6770a67f640f0eefbad03aff3c43e254b52239` 的[协议记录](https://github.com/thefotes/Dispatch/blob/6c6770a67f640f0eefbad03aff3c43e254b52239/docs/protocol/creator-micro-2.md#vendor-methods)、[绑定说明](https://github.com/thefotes/Dispatch/blob/6c6770a67f640f0eefbad03aff3c43e254b52239/Sources/DispatchCreatorMicro/Keymap.swift#L24)及[解码边界](https://github.com/thefotes/Dispatch/blob/6c6770a67f640f0eefbad03aff3c43e254b52239/Sources/DispatchCreatorMicro/VendorDecoder.swift#L20)。它是作者对固件 **0.6.2** 的观察；本机身份曾读回 **0.6.3**（E-078），本轮没有验证当前固件输入或物理映射。下面是协议事实的自撰契约，不移植第三方实现或厂商材料。

- 只分类无 `id/i` 的 `v.oai.hid` 通知。对象参数中的 `k` 必须精确为 `AG00`–`AG18`，`act` 必须为整数 `1`（候选按下）或 `0`（候选释放）。兼容现有包络的 `method/m`、`params/p` 别名；冲突别名、非对象、缺字段、字符串/布尔/小数或其他 act 一律 unknown，不将 `2` 转为释放。未来附加字段保留在内部原通知，不解释、不跨桥接或写日志。
- `k` 是活动层绑定的 vendor keycode 名称。相应通知依赖该控件已绑定 `KV_OAI_AGnn`；连接或订阅不修改绑定。重复绑定可使多个实体发送相同 k，不能按数字推定扫描位置。
- 该来源布局将 AG00–12 分配给 13 个矩阵开关，将 AG10/11 作为同一宽键帽的两个开关，将 AG13/14 分配给旋钮两方向、AG15–18 分配给摇杆四个 cardinal sectors。这些只是该布局的候选对应，本项目保留每条绑定按/放，不合并 AG10/11、不计算 detent、不生成方向或动作角色；旋钮按压没有已证明的 AG ID，AG19 继续 unknown。
- [cm2-probe 的历史基线](https://github.com/panghy/cm2-probe/blob/1cb03f970bc86061be903fdd49f534e3fdaf0862/README.md#vendor-key-events-codex-layer)是固件 0.6.0 / Input 0.18.0-rc.8，包含 ACT/ENC 名称与旋转 `act=2`；不与本候选合并。`kb.radial`、`v.oai.rad` 连续摇杆和 `kb.sa.*` 执行语义通知均不投影为本候选按键事件。

Rust→UniFFI→Swift 仅公开 candidate kind、`binding_id`、本地会话/序号/时间和候选来源状态；`logical_switch`、物理控件、键帽组和动作角色为空，`action_eligible=false`。本地序号不是固件序号，接收时间不是设备时间。unknown 不公开原始字段。流溢出终止订阅，断连与旧会话回调保护继续生效；重建订阅不补猜旧释放。

进一步解锁仍须独立核实当前活动配置与绑定是否唯一、逐实体按住/释放、宽键两开关、旋钮方向与 detent、摇杆回中，以及丢包/重连和其他消费者副作用；不能为取得事件自动 provision、切 Profile/Layer 或写设备。E-089 仅记录候选实现与合成验证，#14 / UNKNOWN-06/07/10/12/17 的实机缺口保持开放。

## E-108：当前设备的限定实测补充

2026-11-03，用户授权后，USB Creator Micro 2 / 0.6.3 的 H0 完整备份、H1 五块共 6220 字节的长→短→长临时内容及删除通过；未完成块可先不出现在目录，`completed:true` 后要求完整读回。实际 `keymap.json` 最小已有 RGB 字段修改和原字节恢复通过高层 FFI 完成，`smart_actions.json` 未变，活动 Profile/Layer 未切换。普通键候选共用此应用路径，未做实体按键验收。

原生发送现一次取得 HID 句柄锁并连续发送一个 RPC 的所有报告，避免 reader 的 20ms read_timeout 插入分包。此前出现缺少合法 ID / error 400；修复后恢复通过，不据此推定所有竞争原因已消除。保持响应严格校验、失败停止、计划一次性消耗；不重试不确定的写入。该次 E-108 未执行原生 `lights.preview`，跨连接自动恢复仍未开放；后续预览采用上文独立语义。
