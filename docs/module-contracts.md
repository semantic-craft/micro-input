# 私有协议研究对独立实现的约束

本页保留从[Input 1.1.0 静态结果](input-architecture.md)推导的独立实现约束，并列出当前 Rust 核心、C-ABI 与 SwiftUI 的源码入口。源码可确认的能力、历史设计要求和设备验收分别记录；静态研究不证明当前固件支持。

| 现有模块 | 应承担的工作 | 明确不得承担的工作 | 依据 |
| --- | --- | --- | --- |
| `mi-transport` | 枚举、原生句柄、API 层报告读写、关闭 | 解释宏、直接执行 host 通知 | IA-02–04 |
| `mi-proto` | 帧与 JSON 编解码、回应/通知分类、方法及参数守卫 | 因 SDK 存在某方法就放开生产发送 | IA-04、07 |
| `mi-session` | 请求排队、关联、取消、断连、读取完整性与来源 | 把取消当回滚；把缓存当设备快照 | IA-04–07 |
| `mi-model` | 稳定引用、草稿、源字段保留、设备投影 | 用数组索引替换所有 ID；静默丢弃未知配置 | IA-12–14 |
| `mi-core` | 工作台命令、宿主路由、设备工作流与展示投影 | 用编辑选择代替运行目标；用未确认动作报告成功 | IA-01、09–14 |
| `mi-macos` | 原生宿主执行、AX 目标核验、窗口与被动面板 | 绕过原目标确认；把 AXPress 返回成功当作效果已确认 | IA-10、11 |
| `mi-ffi` | 命令和投影的 C-ABI 桥、句柄与返回字符串所有权 | 持有高层设备状态机；给 Swift 暴露任意 RPC 或研究脚本执行口 | IA-01 |
| SwiftUI `CoreStore` 与视图 | 解码投影、按 Equatable 域切片展示、转发命令和系统通知 | 重建 Rust 业务状态机；浏览 Profile 就隐式启动设备写入 | IA-13、14 |

## 核心源码模块索引

| 模块能力 | 核心源文件 | 说明 |
| --- | --- | --- |
| 输入流与按键解析 | [input.rs](../crates/mi-session/src/input.rs) | `CandidateInputDecoder` 与 `VerifiedInputDecoder`、`WideKeyMerger` 宽键合并 |
| 配置写入与分块传输 | [config_write.rs](../crates/mi-session/src/config_write.rs) | `ConfigWriter` 的计划绑定应用与恢复，合成构造受 feature 限制 |
| 灯光即时预览 | [lighting_preview.rs](../crates/mi-session/src/lighting_preview.rs) | 原生 `Preview::begin_native` 与合成 `Preview::begin` |
| 宿主动作与透传路由 | [host_router.rs](../crates/mi-model/src/host_router.rs) | `check_passthrough_health`、`resolve_host_action` 纯函数路由 |
| 偏好与关联存储 | [routing.rs](../crates/mi-model/src/storage/routing.rs) | 宿主自动跟随与前台关联持久化 |
| 变更候选生成 | [apply.rs](../crates/mi-model/src/apply.rs) | `Pending`、`Preview` 与一次消费的 `PreparedCandidate` |
| 持久操作与恢复记录 | [operation_record.rs](../crates/mi-model/src/operation_record.rs) | 失败安全与原子落盘操作记录 |
| 草稿与设备工作区 | [draft.rs](../crates/mi-model/src/draft.rs)、[storage.rs](../crates/mi-model/src/storage.rs) | 稳定身份、原子保存、宿主文档与设备草稿隔离 |
| 工作台命令与运行门禁 | [store.rs](../crates/mi-core/src/store.rs) | `WorkbenchStore` 持有工作流、`HeldDictation` 与宿主执行器 |
| 展示投影 | [projection.rs](../crates/mi-core/src/projection.rs) | 从当前事实生成投影，`projection_generation` 门禁由 Store 管理 |
| 宿主执行与目标确认 | [executor.rs](../crates/mi-macos/src/executor.rs)、[managed_controls.rs](../crates/mi-macos/src/managed_controls.rs) | `LiveHostActionExecutor` 与 `ManagedAxControls` 保留原窗口、分屏和录音目标 |
| C-ABI 桥 | [lib.rs](../crates/mi-ffi/src/lib.rs) | 解码 `WorkbenchCommand`、派发、投影、poll、释放句柄与字符串 |
| Swift 展示桥 | [CoreBridge.swift](../app/MicroInput/Sources/MicroInputUI/CoreBridge.swift)、[ProjectionModels.swift](../app/MicroInput/Sources/MicroInputUI/ProjectionModels.swift) | `CoreStore` 与 Equatable 域切片，系统通知转发到 Rust |

当前验证入口包括 [runtime_consistency_tests.rs](../crates/mi-core/src/runtime_consistency_tests.rs)、[workbench_store.rs](../crates/mi-core/tests/workbench_store.rs)、[managed_controls.rs](../crates/mi-macos/tests/managed_controls.rs) 和 C-ABI 的 `ffi_roundtrip_projects_and_dispatches_commands`。它们分别覆盖 core 接线、工作台状态、AX 确认与桥接所有权。

## 最小行为契约

- **读取**：完整性失败不发布新快照；保留旧快照但标识过期。当前只读出口继续保持。
- **输入**：现有 `InputDecoder` 接口可承接将来证实的原始事件；公开社区 AG 输入可作[不可执行候选](protocol-candidates.md#公开社区输入候选dispatch-062)，独立保留绑定身份，不赋造物理映射或动作角色。其他宿主通知字典继续使用不可执行的未知投影。
- **写入**：准备、执行、验证、本地保存是不同状态；终态至少区分“未发送”“设备状态未确认”“设备已验证但本地保存失败”“全部完成”。具体公开 API 待写能力获证后扩展，当前不新增方法枚举。
- **取消**：终止尚未发送的请求并使旧会话结果失效；若已经有设备副作用，另行读取和判断，不自动宣称已恢复。
- **宿主执行**：另设显式能力边界。命令同意、目标应用、路径和焦点约束不属于底层 transport。当前不新增执行器或后台服务。
- **前台上下文**：变化检测、锁屏抑制、发送节流与停止生命周期独立于配置编辑；何时切层由证据决定。
- **浮窗**：显示当前已核验活动配置或明确标识过期来源；不从当前编辑草稿推断键盘实际状态。

## 当前接口与验证落点

现有接口的输入输出与验证落点如下。错误名为当前 Rust API，约束为后续改动必须保留的验收要求，不表示本轮重新执行过全部测试。

| 接口 | 输入 → 输出 | 状态、错误与取消 | 验收落点 |
| --- | --- | --- | --- |
| HidTransport.enumerate/open | 枚举 → HidCandidate；临时 id → HidLink | 后端与设备身份属于 transport；Unavailable/Unsupported/Disconnected 不转成成功 | 无候选、多候选不误连；关闭后资源释放 |
| HidLink.read/write/close | API 报告字节 → 字节或结果 | 句柄生命周期属于 link；关闭必须终止旧 I/O | 合成与原生后端分别验证，不能用 buffer 测试代替实机 |
| RpcCodec.encode/decode | RpcRequest → 字节；行 → Incoming | 不拥有会话；Invalid 与 Unverified 保持可区分 | 分帧后重组、响应短字段、畸形字段、外来 ID |
| Framer.encode/feed/clear | JSON 字节 → Reports；输入报告 → Lines | 持有分帧缓冲，clear 清除残留 | 跨帧、跨行、未知通道、断连后不拼接旧内容 |
| DeviceSchema | 请求构造及 Value → FileMeta/ReadChunk/DeviceConfig | 版本和数据约束属于 schema；协议不支持返回 Unverified | 未验证组合零发送；大小/摘要/分块变化拒绝发布 |
| DeviceSession | 高层读取及连接操作 → 身份/快照/通知 | pending、reader、generation、snapshot 属于 session；Timeout/Disconnected/Config/Protocol/Transport 分开；取消不得污染新会话 | 迟到响应、取消、断连、同设备重开、旧快照保留 |
| InputDecoder / InputMonitor（[input.rs](../crates/mi-session/src/input.rs)） | HostNotification → 可选逻辑事件；订阅 → InputEvent | 候选 decoder 不授予执行资格；`VerifiedInputDecoder` 只解释已有核验映射；流缺口清理按住状态并停止 | 未知通知不执行；物理映射验证前 action_eligible 不升级 |
| `WorkbenchStore` / C-ABI / `CoreStore` | `WorkbenchCommand` → JSON 投影与域切片 | 草稿、设备快照和当前运行状态分别维护；无变化派发和 poll 不重新序列化 | `ffi_roundtrip_projects_and_dispatches_commands`、[MicroInputTests.swift](../app/MicroInput/Tests/MicroInputTests/MicroInputTests.swift)、[WorkspaceLifecycleTests.swift](../app/MicroInput/Tests/MicroInputTests/WorkspaceLifecycleTests.swift) |

新通知的静态字段结构可以进入研究 contract；尚未确认来源与物理语义时，不进入 InputDecoder 的可执行输出。写入与宿主执行尚无本轮授权新增的产品接口，不能填造 API 签名来掩盖未决行为。

既有 `HidLink/HidTransport`、`Framer/RpcCodec`、`DeviceSession/DeviceSchema`、`InputDecoder` 与 `WorkbenchStore` 提供上述接入边界。本轮没有足够的新设备证据支持开放写入或宿主动作，因此不另造一套模拟成功的骨架。后续实现应在这些接口上补证据支持的行为与回归，保持 [现有实施设计](implementation-plan.md) 的能力门和恢复约束。

## 下一轮可执行的补证

1. 为独立 userData 与后台自动连接行为建立隔离证据；在此之前不重启正在使用的 Input 来插桩。
2. 以一个明确、合成、非破坏性场景验证 Inspector 或原生观察点确实命中，记录 API 层与物理层边界。
3. 将单个协议候选与实际固件回应对照，再决定对应能力是否可进入模型与会话。
4. 对写入阶段单独验证设备基线、读回和恢复；对物理输入单独验证键帽/开关/旋钮对应关系。

## S3 候选与会话写入边界

`mi-model::lighting` 保存Layer目标并复用mutate/undo；`apply`在精确基线下生成单键或灯光候选，副本成功后才允许取得一次PreparedCandidate。此对象不携带生产发送权限。`mi-proto::mutation`仅编码限定keymap块；原Method/公开request的只读守卫不变。

[ConfigWriter](../crates/mi-session/src/config_write.rs) 复用私有会话队列和唯一 reader。`ConfigWriter::new` 只在 `synthetic-backend` 编译，并拒绝非合成连接；`ConfigWriter::native` 要求原生 numbered 后端和非空接口身份。两条路径都绑定准备计划、核验基线、逐块写入并独立读回，保全 `smart_actions`，失败或取消后不自动重试。普通 RPC 入口的只读守卫不变。当前 `WorkbenchStore` 的配置编辑入口提供候选预览、备份和一次消费，不据此宣称工作台已接通会话 writer 的全部应用与恢复能力。


## S3 离线持久记录与恢复

`mi-model::operation_record` 复用原副本和候选文件，仅新增冻结的前后本地文档、材料关联及校验状态文件；目录 0700、文件 0600，状态通过临时文件、fsync、rename、目录同步更新。创建操作目录即持久占用计划 ID，重开只读记录，不重发。状态文件不含配置载荷；含原文档的私有恢复材料不进入诊断日志。

`ConfigWriter` 首次可能发送前保存 Writing，逐块确认和完整读回分别落盘。读回后本地失败返回 VerifiedLocalSaveFailed；只有持久核验结果完整时，重开后才可离线修复。若核验结果未落盘，保留未知状态，必须显式独立读回；当前进程仍保留已核验事实，可在材料完好时重试落盘。恢复新计划绑定当前完整原字节、明确合成身份和固件，旧计划过期后零变更；现状不可解析不阻断原始读取，缺失/摘要错误/读取失败则拒绝恢复。

恢复只写 keymap，保全现状 smart_actions，并保存恢复链关联。显式读回可区分 original/candidate/other；OriginalVerified 表示本次检查等于原配置，不声称从未发送。未决记录冻结对应文档的编辑和新普通应用；成功恢复或本地修复后重开文档清空旧撤销历史。retry_local_save 不依赖会话，仅核对冻结文档身份、修订和原字节；磁盘已经等于目标时只补记录，其他内容一律拒绝覆盖。记录损坏时保持关闭，不自动清理材料。合成身份为模拟器固定标识，不冒充硬件序列号验证。


## S3 历史 FFI 与宿主排空设计

旧 `mi-ffi::device_operations`、UniFFI token 接口、Swift 执行 lease 与 `DeviceOperationsTests` 属于历史实现，当前源码已退役。它们不能作为当前工作台完成应用、恢复或排空的证据。

原设计要求保留为待接线契约。准备计划绑定文档身份、修订与原字节，重连使旧计划失效。检查、恢复准备、本地修复与写入互斥。取消输入订阅不等于在途动作完成，排空未确认时不得开始写入。旧代回执不能更新新会话，未知结果不得自动重放。后续接线仍须验证原目标清理、一次消费、跨会话拒绝、写后重验与离线修复。

当前软件准备落点是 [DocumentStore](../crates/mi-model/src/storage.rs) 与 [WorkbenchStore::dispatch_editor_action](../crates/mi-core/src/store.rs)。当前会话应用与恢复行为由 [config_write.rs 测试](../crates/mi-session/tests/config_write.rs) 覆盖；候选和备份门禁由 [apply_preparation.rs](../crates/mi-model/tests/apply_preparation.rs) 与 [inspector_and_editors.rs](../crates/mi-core/tests/inspector_and_editors.rs) 覆盖。这些分层测试不能替代尚未证实的完整工作台接线。


## RGB 编辑与合成预览

RGB 编辑由 [Draft](../crates/mi-model/src/draft.rs) 与 [mi-model::lighting](../crates/mi-model/src/lighting.rs) 保存，`WorkbenchStore` 转发编辑并生成参数投影。旧 `mi-ffi::lighting` 是已退役的历史模块。当前 `LightingBeginPreview` 只更新工作台预览状态与秒数，未调用会话 `Preview::begin_native`；这是工程接线缺口，不能只标为实体效果未验收。下面的合成和原生预览契约描述会话层能力。

`mi-session::lighting_preview::Preview::begin`（[lighting_preview.rs](../crates/mi-session/src/lighting_preview.rs)）只在 synthetic-backend 编译并拒绝非合成连接；使用明确标记的 `synthetic.lights.read/set` 测试协议，而非臆造官方预览 ACK/目标绑定规则。可变合成文件另存运行态和活动目标；目标及最后一次已确认运行态同时匹配的原子 set 才改变运行态，文件保持原字节。原生 `begin_native` 按文末契约单独发送，不复用这些合成消息。

一个有界任务持有同一会话队列和唯一 reader，watch 槽只保留最新待发状态，80 ms 合并更新；不会取消已在途交换。队列持有期内其他读取不消耗 ID，更新始终为结束恢复和独立运行态读回保留两个 ID，30 秒自动结束。结束先停更新，再恢复、读回；切层或同层外部改色均拒绝覆盖更新和恢复，失败/断连为 Unknown 且断开旧会话，不自动重连/重试。配置操作与预览互斥，未知恢复结果保持关闭。合成运行态成功不证明真实灯光视觉效果。
## S5 宿主跟随与输入边界

- `storage::routing` 在工作台偏好文件中原子保存选择、手动保持、自动开关和 bundle ID 关联；严格解析保留，失败不发布新 revision。
- `mi-session` 唯一 reader 在广播前分配 ingress sequence，消费者的 sequence 只作诊断。摇杆回中/离中状态即使没有动作边沿也向上交付。
- [host_router.rs](../crates/mi-model/src/host_router.rs) 负责纯函数路由；[store.rs](../crates/mi-core/src/store.rs) 与 [six_layer_store.rs](../crates/mi-core/src/six_layer_store.rs) 持有运行选择和动作来源，设备会话与文档身份互不替代。
- [runtime_inbox.rs](../crates/mi-core/src/runtime_inbox.rs) 接收硬件事件，`WorkbenchStore::poll_hardware_events` 消费事件并核对代次与门禁。`InputStreamGap` 停止原目标动作并封锁旧输入会话，重新连接后才接收新输入。旧 Swift `HostRouting` 的 FIFO 与 lease 数字是历史设计，不能套用为当前实现参数。
- [projection.rs](../crates/mi-core/src/projection.rs) 生成实际运行配置投影；[passive_panel.rs](../crates/mi-macos/src/passive_panel.rs) 显示被动面板。Swift `CoreBridge.swift` 的 `WorkspaceLifecycle` 转发睡眠和唤醒，Rust [workspace_lifecycle.rs](../crates/mi-core/src/workspace_lifecycle.rs) 退役并等待旧任务。旧 `HostOverlay` 和 `WorkspaceSleepObserver` 是历史 owner。

采用 IA-09/11 作为静态研究背景，不实现厂商 AppSense 通知或设备自动激活；本轮验证须分别记录自动化输入夹具、原生窗口与真实硬件覆盖。

## 原生候选字节与发送权限

`mi-proto::lighting_candidate` 的两区类型仅生成固定 `lights.preview` 候选字节，保留可选字段缺省，校验客户端已知范围；它不是配置往返模型；仅会话私有 `exchange_lighting` 为显式预览提供发送入口。`mutation::validate_mutation_response_candidate` 只确认匹配ID的成功包络，不提升为写入或恢复结果。原生通用回应的 `result` 不限类型，不能继续依赖自造的 `synthetic_written`。

`DeviceSession::exchange_encoded` 在分帧前重新校验非合成连接的实际方法、参数与队列ID，再从受限对象重新编码，原始重复键等歧义字节不进入传输，候选字节即使被内部调用误标为只读也零发送、零消耗ID；合成后端继续承载受控写入/预览实验。原生通用 `Method` 未扩展，高层 capability 由明确的配置/预览入口拥有。`check-boundaries.py` 同时运行候选字节绕过回归，硬件探针仍不执行。


## 原生配置计划入口（E-108）

`ConfigWriter` 共用持久记录和独立读回，原生构造要求受支持后端与接口身份，执行时重新核验固件和计划基线。旧 FFI 的唯一 USB 型号检查属于历史接线，不能替当前 C-ABI 作证。一般RPC仍只读；只有私有 `exchange_keymap` 为审阅计划构造固定keymap写块。MacLink按整条RPC持锁发送所有报告，唯一reader不会插入阻塞读取。原生基线包含随机connection_nonce，同连接原值恢复可用，跨连接旧记录拒绝。RGB 可进入 keymap 候选；会话层原生即时预览遵循文末契约，工作台接线缺口见上节；文件读回不代表物理/视觉验收。

## 设备编辑与宿主文档隔离

`DocumentStore` 的设备工作区保留宿主 Store，宿主路由从 `host_store()` 取文档和偏好，设备修改只影响独立设备草稿。`begin_device_editing` 从当前核验快照创建新草稿，`resume_device_editing` 只打开已保存草稿，`return_to_host` 恢复宿主编辑状态；入口与设备操作互斥。设备草稿选择记录不改写宿主的 active 文件。

`apply::Pending` 将多个普通键和已有 RGB 字段编译为一个候选；Preview.changes 给出逐项来源及前后值。会话层应用和恢复沿用 E-108 的计划约束。工作台当前可准备并消费候选，完整应用、恢复与写后逐绑定重验的接线仍须单独核实。

## 原生 RGB 即时预览（2026-11-04）

原生预览为显式设备级操作：将所选草稿灯光的两区已知参数发送到当前核验连接，不切换 Profile/Layer、不写配置文件。结束、关闭或 30 秒到期停止待发更新并等待在途请求，返回 Stopped；不声称自动恢复或运行态读回。失败停止且不重发，明确断连退役旧预览。合成恢复模拟器的 Restored/原子比较不作为原生开放前提。

会话层 `Preview::begin_native` 接收两区值，并经当前连接重新核对 0.6.3。当前工作台尚未调用此入口，旧 FFI 的唯一 USB 型号检查不代表当前生产调用。每次持有现有队列后重新取最新watch值，停止/所有者取消/到期后不发送旧pending；已经发送的交换完成后再结束，失败断开旧会话、不重发。`exchange_lighting` 只编码固定两区DTO，通用字节出口仍拒绝灯光方法。

历史预览采纳设计要求绑定进入时的文档身份与修订，通过 DocumentStore 原子保存并作为一次 undo；当前实时预览接线尚未证实。`add_lighting` 只对原始来源完全不存在 lights 成员开放，新增成员编译保持所有非目标原字节；未知字段和已有null不补默认值。
