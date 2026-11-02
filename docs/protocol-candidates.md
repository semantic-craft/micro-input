# Input 1.1.0：配置写入与宿主通知候选协议

本页为安装包静态分析结果。字段和方法存在于客户端，不等于当前键盘固件支持。本轮没有发送这些操作。证据身份及 IA 定位见[架构说明](input-architecture.md)；已有只读请求见[只读协议](protocol-readonly.md)。下面没有任何方法因此获得生产写入授权。

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

## 配置格式

`keymap.json` 输出字段：`version:1`、`activeProfileId`、`language`、`profiles`、`multiActions`、`macros`、`macrosGroups`、`multiActionsGroups`、`deviceSpecificConfig`、`linkedApps`（IA-12）。不要由字段名断定 activeProfileId 是稳定 Profile ID：前端更新链将所用 Profile 索引传入它，UI 中还会以数组长度验证其范围。独立模型应保存源值并单独验证语义。

- Profile 包含 `id/name/layers`；Layer 包含 `id/name/color/layout/os/lights/linkedAppId`。
- 普通宏的步骤编码为 `{kc, delay, act}`；对象还有 `id/name/color/icon`，步骤列表名为 `actions`。
- Multi Action 编码四个字段 `kcOnTap/kcOnHold/kcOnDoubleTap/kcOnTapHold`，阈值字段为 `tt`。构造默认 250 是应用默认值，不能当作固件范围上限或下限。
- `smart_actions.json` 为 `{version:1, smartActions:{SA_<id>:...}, smartActionGroups:[...]}`。每个 action 保存 `name/icon/color/type/payload`；ID 来自对象键。解析器接受 `SA_0`，不能依据旁边陈旧的正整数注释排除 0。
- 输出 JSON 进行非 ASCII Unicode 转义；摘要应针对实际输出字节。不要在算摘要后再格式化文件。

上述为主要序列化字段，不是包含全部 layout、灯效值域和型号分支的完整 schema。

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
