# 私有协议研究对独立实现的约束

本页是从[当前静态结果](input-architecture.md)推导的自研设计要求，不是对 Input 内部实现的逐行移植。本轮未增加产品代码；现有 Rust + SwiftUI 骨架继续使用，接口是否完整仍由测试和设备验收决定。

| 现有模块 | 应承担的工作 | 明确不得承担的工作 | 依据 |
| --- | --- | --- | --- |
| `mi-transport` | 枚举、原生句柄、API 层报告读写、关闭 | 解释宏、直接执行 host 通知 | IA-02–04 |
| `mi-proto` | 帧与 JSON 编解码、回应/通知分类、方法及参数守卫 | 因 SDK 存在某方法就放开生产发送 | IA-04、07 |
| `mi-session` | 请求排队、关联、取消、断连、读取完整性与来源 | 把取消当回滚；把缓存当设备快照 | IA-04–07 |
| `mi-model` | 稳定引用、草稿、源字段保留、设备投影 | 用数组索引替换所有 ID；静默丢弃未知配置 | IA-12–14 |
| `mi-ffi` | 高层操作状态、完成结果、来源与错误 | 给 Swift 暴露任意 RPC 或研究脚本执行口 | IA-01 |
| SwiftUI Store/Workbench | 浏览、编辑、差异、明确提交状态 | 选择 Profile 就隐式启动设备写入 | IA-13、14 |

## 最小行为契约

- **读取**：完整性失败不发布新快照；保留旧快照但标识过期。当前只读出口继续保持。
- **输入**：现有 `InputDecoder` 接口可承接将来证实的原始事件；公开社区 AG 输入可作[不可执行候选](protocol-candidates.md#公开社区输入候选dispatch-062)，独立保留绑定身份，不赋造物理映射或动作角色。其他宿主通知字典继续使用不可执行的未知投影。
- **写入**：准备、执行、验证、本地保存是不同状态；终态至少区分“未发送”“设备状态未确认”“设备已验证但本地保存失败”“全部完成”。具体公开 API 待写能力获证后扩展，当前不新增方法枚举。
- **取消**：终止尚未发送的请求并使旧会话结果失效；若已经有设备副作用，另行读取和判断，不自动宣称已恢复。
- **宿主执行**：另设显式能力边界。命令同意、目标应用、路径和焦点约束不属于底层 transport。当前不新增执行器或后台服务。
- **前台上下文**：变化检测、锁屏抑制、发送节流与停止生命周期独立于配置编辑；何时切层由证据决定。
- **浮窗**：显示当前已核验活动配置或明确标识过期来源；不从当前编辑草稿推断键盘实际状态。

## 骨架当前结论

现有接口的输入输出与验证落点如下。错误名为当前 Rust API，约束为后续改动必须保留的验收要求，不表示本轮重新执行过全部测试。

| 接口 | 输入 → 输出 | 状态、错误与取消 | 验收落点 |
| --- | --- | --- | --- |
| HidTransport.enumerate/open | 枚举 → HidCandidate；临时 id → HidLink | 后端与设备身份属于 transport；Unavailable/Unsupported/Disconnected 不转成成功 | 无候选、多候选不误连；关闭后资源释放 |
| HidLink.read/write/close | API 报告字节 → 字节或结果 | 句柄生命周期属于 link；关闭必须终止旧 I/O | 合成与原生后端分别验证，不能用 buffer 测试代替实机 |
| RpcCodec.encode/decode | RpcRequest → 字节；行 → Incoming | 不拥有会话；Invalid 与 Unverified 保持可区分 | 分帧后重组、响应短字段、畸形字段、外来 ID |
| Framer.encode/feed/clear | JSON 字节 → Reports；输入报告 → Lines | 持有分帧缓冲，clear 清除残留 | 跨帧、跨行、未知通道、断连后不拼接旧内容 |
| DeviceSchema | 请求构造及 Value → FileMeta/ReadChunk/DeviceConfig | 版本和数据约束属于 schema；协议不支持返回 Unverified | 未验证组合零发送；大小/摘要/分块变化拒绝发布 |
| DeviceSession | 高层读取及连接操作 → 身份/快照/通知 | pending、reader、generation、snapshot 属于 session；Timeout/Disconnected/Config/Protocol/Transport 分开；取消不得污染新会话 | 迟到响应、取消、断连、同设备重开、旧快照保留 |
| InputDecoder / InputMonitor | HostNotification → 可选逻辑事件；订阅 → InputEvent | 当前生产 decoder 仍不解释物理按键；流缺口清理按住状态并停止 | 未知通知不执行；物理映射验证前 action_eligible 不升级 |
| MicroInput / Swift Store | 高层命令 → DTO 与界面状态 | 桥接隐藏通用 RPC；草稿、设备快照和当前活动状态分别维护 | Navigation、SessionLifecycle、InputMonitor 的真实桥接测试 |

新通知的静态字段结构可以进入研究 contract；尚未确认来源与物理语义时，不进入 InputDecoder 的可执行输出。写入与宿主执行尚无本轮授权新增的产品接口，不能填造 API 签名来掩盖未决行为。

既有 `HidLink/HidTransport`、`Framer/RpcCodec`、`DeviceSession/DeviceSchema`、`InputDecoder` 和 `MicroInput` 已提供上述主要接入边界。本轮没有足够的新设备证据支持开放写入或宿主动作，因此不另造一套模拟成功的骨架。后续实现应在这些接口上补证据支持的行为与回归，保持 [现有实施设计](implementation-plan.md) 的能力门和恢复约束。

## 下一轮可执行的补证

1. 为独立 userData 与后台自动连接行为建立隔离证据；在此之前不重启正在使用的 Input 来插桩。
2. 以一个明确、合成、非破坏性场景验证 Inspector 或原生观察点确实命中，记录 API 层与物理层边界。
3. 将单个协议候选与实际固件回应对照，再决定对应能力是否可进入模型与会话。
4. 对写入阶段单独验证设备基线、读回和恢复；对物理输入单独验证键帽/开关/旋钮对应关系。

## S3 首段离线扩展

`mi-model::lighting` 保存Layer目标并复用mutate/undo；`apply`在精确基线下生成单键或灯光候选，副本成功后才允许取得一次PreparedCandidate。此对象不携带生产发送权限。`mi-proto::mutation`仅编码限定keymap块；原Method/公开request的只读守卫不变。

`mi-session::config_write::SyntheticWriter`只在synthetic-backend编译，构造时拒绝原生/未知backend，复用私有会话队列和唯一reader。显式合成文件按实际字节生成摘要，合成ACK不冒充固件事实；写后逐字节比较并保全smart_actions，失败/取消后不自动重试。它是离线验证工具，已接入持久操作记录和失败恢复；尚无宿主排空、FFI 或产品恢复 UI，不能作为生产协调器。只读快照复用抽出的原始文件校验；解析失败仍不发布快照。


## S3 离线持久记录与恢复

`mi-model::operation_record` 复用原副本和候选文件，仅新增冻结的前后本地文档、材料关联及校验状态文件；目录 0700、文件 0600，状态通过临时文件、fsync、rename、目录同步更新。创建操作目录即持久占用计划 ID，重开只读记录，不重发。状态文件不含配置载荷；含原文档的私有恢复材料不进入诊断日志。

`SyntheticWriter` 首次可能发送前保存 Writing，逐块确认和完整读回分别落盘。读回后本地失败返回 VerifiedLocalSaveFailed；只有持久核验结果完整时，重开后才可离线修复。若核验结果未落盘，保留未知状态，必须显式独立读回；当前进程仍保留已核验事实，可在材料完好时重试落盘。恢复新计划绑定当前完整原字节、明确合成身份和固件，旧计划过期后零变更；现状不可解析不阻断原始读取，缺失/摘要错误/读取失败则拒绝恢复。

恢复只写 keymap，保全现状 smart_actions，并保存恢复链关联。显式读回可区分 original/candidate/other；OriginalVerified 表示本次检查等于原配置，不声称从未发送。未决记录冻结对应文档的编辑和新普通应用；成功恢复或本地修复后重开文档清空旧撤销历史。retry_local_save 不依赖会话，仅核对冻结文档身份、修订和原字节；磁盘已经等于目标时只补记录，其他内容一律拒绝覆盖。记录损坏时保持关闭，不自动清理材料。合成身份为模拟器固定标识，不冒充硬件序列号验证。
