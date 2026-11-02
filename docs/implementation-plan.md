# Micro Input 实施设计

状态：2026-11-02 用户批准；所有新增 API 名称、容量及策略均为设计契约，不是现有实现或固件保证。

入口：SPEC、BUILD_PROMPT、README、development 及相关工单引用本文件。工单持有状态，本文件持有实施契约；不维护第二套工单系统。

当前按用户最新指令在统一集成分支实施全部 35 张工单。按依赖推进代码与可验证切片；真实设备写入仍须满足协议、备份和恢复门槛，不接任务自动化。保留 35 张工单和 48 条用户故事；所有原有验收继续有效，以下仅明确歧义或显式调整依赖。原厂代码/资源不进入实现；现有证据等级不因本设计升级。

## D01 · 发现与身份分开交付（F01；#3/#4）

规则：R-D01/R-D02/R-D07、R-A02/R-A05。协议依据：`docs/protocol-readonly.md` 第 1–3、7 节；未知项以 `docs/spec/UNKNOWNS.md` 为准。

现有改动位置：

- `crates/mi-transport/src/lib.rs` 与 `discovery.c`：HidCandidate、is_candidate、MacTransport、非独占打开和生命周期；核对完整 usage pairs 与 USB 来源，生产首验仅 PID 0x8298。0x8297 只显示未验型号，不自动打开。
- `crates/mi-session/src/lib.rs`：身份查询、会话 generation、DeviceSchema；`tests.rs` 添加响应/取消场景。
- `crates/mi-ffi/src/lib.rs`：高层 `read_identity()` 返回 IdentityInfo；不把通用 RPC 暴露给 Swift。
- `app/MicroInput/Sources/MicroInput/Store.swift`：Backend 加 `readIdentity()`；把 connect 中自动 readConfig 拆成显式 `readDeviceConfig()`。
- `app/MicroInput/Sources/MicroInput/Model.swift`、`Workbench.swift`：连接/身份/配置三个独立状态及来源显示。
- `app/MicroInput/Tests/MicroInputTests/NavigationTests.swift`：桥接身份读取与配置读取按钮分离。

拟定 IdentityInfo：`session_id: u64`、`version: Option<String>`、`profile_index/layer_index: Option<u32>`、`source_method: IdentitySource`、`verification: VerificationState`。身份字段范围未知时只保留 raw 并显示未知，不把负值或浮点值强转为索引。物理设备标识不得取 PID 或单独路径；候选打开用临时 id，可靠序列号缺失时跨重连身份标记不确定，要求重新选择，不自动合并。

连接状态：Disconnected → Connecting → ConnectedIdentityUnknown → IdentityReady 或 IdentityFailed。身份失败仍有明确连接状态；断连立即取消旧 generation。读取配置是另一个显式操作，成功才产生新的 DeviceSnapshot；浏览、连接、身份查询不隐含读取文件。

#3 检查点：候选模型/筛选测试 → 原生枚举与事件 → UniFFI/Swift 候选 UI → 插拔验收。枚举不申请输入监控，不发 RPC。多候选不自动选择。

#4 检查点：证据适配和 D02 守卫 → D04 接收基础 → 身份解析 → 桥接 UI → 应用真实身份验收。状态查询失败或无 version 才使用版本回退；不编造缺失字段。先离线通过，再在明确授权的只读会话做真实验证。探针成功不能代替应用验收。

每个检查点运行适用单元测试；整票运行 `scripts/check`（含 Swift 构建），保留真实原生交互证据。仅文档修订不运行应用验收。

## D02 · 能力与出口守卫（F02；#4）

文件：`crates/mi-proto/src/lib.rs`、`crates/mi-ffi/src/lib.rs`、`scripts/check-boundaries.py`，以及各自测试。

将“生产 Framer 恰好为 UnimplementedFramer”的临时断言替换为行为约束。仍保留 UnimplementedFramer/UnverifiedSchema 作为未核验组合的拒绝实现。

生产 BackendCapabilities 按型号、transport、native adapter 和证据支持情况选择。静态支持只允许测试候选实现；应用实机验收才能记录对应组合已验证。不通过环境变量或通用 JSON 请求绕过能力选择。

只读出口检查 Method 为 device.status/sys.version/fs.list/fs.readbin；readbin 文件只能是 keymap.json/smart_actions.json。FsRead 即使保留给测试，也不能在生产出口通过。所有写入、激活、灯光、focused_app 上报仍拒绝。

IOKit report ID 参数与回调格式单独归 adapter，不能把 node-hid 的 64 字节缓冲直接视为 native API 契约。未知组合在发送前返回 Unverified。真实后端适配所需未知事实未补齐时，#4 保持未完成。

验证：未验证组合零发送；越界文件/非白名单方法零发送；正常合成向量与真实出口一致；保留未解 UNKNOWN 并更新已解项的证据引用，不做全量 TODO 删除。

## D03 · 配置读取事务（F03；#5）

规则 R-D08/R-C01。改动：`mi-session/src/lib.rs` 的 DeviceSchema/读取循环与测试；`mi-proto` 的候选 schema；`mi-model` 只读投影；`mi-session/Cargo.toml` 引入 Base64 解码与 SHA-1 库（具体依赖版本在实现时核对锁定，摘要只用于协议一致性）。

拟定类型/接口：

- `FileMeta { name: ConfigFile, size: u64, checksum: Sha1Digest }`；ConfigFile 只有 Keymap/SmartActions。
- `ReadChunk { total_size: u64, data: Vec<u8> }`。
- `files(response) -> Result<Vec<FileMeta>>`；`read_request(file, offset: u64, len: u32)`；`chunk(response) -> Result<ReadChunk>`。
- 设备 JSON 字段及 params 严格用只读协议第 4 节；类型是内部契约，不发明新 wire 字段。

流程：list → 校验目标元数据 → smart_actions（如存在）→ keymap → 验证原始字节摘要 → 第二次 list → 比较目标文件存在性、size、checksum → 解析只读投影 → 一次提交快照。

本地初始预算：每文件 4 MiB，两文件累计 8 MiB；块最大 3072 字节，末块缩短；单请求 10 秒，整次读取 60 秒，无自动重试。预算属于可配置客户端策略，不声称为设备容量/延迟保证。RPC JSON 累积上限 64 KiB，适用于单个 readbin 回应；超限失败，不继续无限累积。

每次检查实际解码长度非零（未到 EOF 时）、不大于请求 len/剩余空间；offset 用 checked arithmetic；total_size 在块间一致且与 list size 一致。非法 Base64、缺失/未知 checksum、文件变化或解析失败均不发布部分新快照。旧快照保留并标 stale。不使用缓存充当本次读取结果；不声称跨文件原子性。

测试包含零长文件、可选文件缺失、非连续 ID、未知字段、超限、空块、坏摘要、第二次 list 变化、取消及旧快照保留；成功/失败均核对发送白名单。

## D04 · 单 reader 会话（F04；#4/#14/#6）

规则 R-D07/R-N01。修改 `mi-session/src/lib.rs` 的 Queue/DeviceSession/exchange，`mi-transport` 的 HidLink 关闭契约，`mi-ffi` 的订阅回调。

每个 DeviceSession 持有且只启动一个接收任务。receiver 独占 Framer 解码状态；发送端持有独立编码器，按单请求在途队列发送，绝不自己调用 read。pending 保存唯一 request id、generation 和 oneshot sender。响应只可交给匹配的 pending；无 ID 通知走单独有界队列。

本地策略：输入通知队列 256 项；满队列时报告 InputStreamGap，清除按住状态并停止向动作执行层提供事件，直到明确重建订阅与状态。不能丢释放事件后继续动作。UI 只展示丢失计数，不记录私密载荷。生产未知通知只显示受限诊断，不执行动作。

保持已证据支持的 request ID 范围；同一会话不复用已使用 ID，耗尽则以明确原因结束会话并要求重新连接，避免无 nonce 协议下迟到响应命中新请求。不能仅凭小 ID 范围宣称多宿主安全；相同 ID 的外来响应无法可靠区分时，共存验收仍未通过，不标记已解决。

超时后取消 pending，迟到响应丢弃；接收错误结束整个会话并拒绝新请求。disconnect：标 generation 无效 → 拒绝/取消请求 → close link 唤醒 read → join receiver → 释放 manager/缓冲。close 必须幂等，禁止旧回调写入新会话。

拟定 FFI InputEvent：session_id、sequence、timestamp、逻辑引用、已核实的事件类型、verification；未知事件不填造物理控件 ID。订阅返回可显式关闭的对象；窗口关闭可以停 UI 订阅，但菜单栏仍持有应用会话；退出必须结束任务与句柄。

#14 增加输入解码和物理核对；#6 验证睡眠/断连/重连、错误分类和清理。权限/保护输入/占用不循环自动重启；提供原因和用户重试。无法区分的 native 错误标“原因未确认”，不能凭一个错误码猜权限。

## D05 · 通过真实桥接验收（F05；#3–#6）

新增 `mi-transport` 的 synthetic-backend feature；脚本测试设备输出有界合成字节流并捕获写出请求。`mi-ffi` 在相同测试 feature 下增加显式构造工厂，复用生产 MicroInput/DeviceSession/解析器和 DTO。不是 Swift 另外实现协议。发布构建不包含该工厂。

Swift Backend 加入所使用的草稿高层方法；Store 不再通过 `as? MicroInput` 绕过协议。测试可注入真实桥接测试工厂；原生 QA 使用隔离文档路径。

共享场景：多个候选、身份成功/失败、分块读成功/中断、空闲输入、过期回调、设备移除。测试分别记录 Rust 单元、UniFFI→Swift、真实设备三层结果。构建脚本生成对应 feature 的 bindings，防止测试/生产 ABI 混用。

## D06 · 文档 v2 与稳定引用（F06/F11；#15/#17/#18/#21/#39）

规则 R-P03/R-E02/R-E09/R-E10；改动 `mi-model/src/draft.rs`、`storage.rs`、`document_tests.rs`，桥接文档状态及 Swift 投影。

LocalDocumentV2（#15实现）：schema_version=2、document_id、revision、baseline、origin、graph、source_refs。origin.config是不可变来源投影及raw，graph是唯一可编辑图。baseline 可为空；非空时含设备身份置信状态、固件、读取时间、两文件存在性/摘要/raw。graph 是可编辑对象与绑定。source_refs 映射本地稳定 ID 到设备源 ID 或 legacy logical-N；不把源 index 转成实体 ID。

新建 Profile/Layer/Action/Binding 使用文档内独立 UUID；导入冲突先建立映射再重写引用，校验无悬空引用。原始文件不随草稿正规化，不因编辑覆盖未知字段。未知结构采用只读/受限投影，不自行补全设备格式。

v1→v2：先完整解析 v1 与 edits，保留其有效草稿语义和原始源值；baseline=null、来源=legacy-local，绝不伪装来自当前设备；持久化旧 logical 引用供后续显式核对。转换失败、高版本或未知字段仍保留原文件并只读。成功写新格式之前，把 v1 原字节写入独立不可覆盖备份并核验摘要；沿用锁、预期内容检查、pending+rename，任一步失败不提交内存状态。

撤销单元为可编辑 graph 与 source_refs 的完整变更，包含重命名、结构和绑定；一次用户确认产生一次 revision。快照事实、设备状态和磁盘迁移不在撤销栈。重连接受新基线后清空旧撤销栈，以免撤销偷偷跨越外部版本；重启不恢复撤销历史。

显式“从快照新建草稿”只复制当前会话完整、未过期的核验快照，保存独立draft-UUID.json和含文件精确字节的baseline，不覆盖原工作台。workbench.active.json仅保存同目录文档文件名，以独立锁、预期字节检查和原子替换切换；失败不改当前选择或内存，已生成新文件保留供检查。重开解析此选择记录；损坏或丢失不静默退回其他草稿。未知原绑定保留在original/origin，显式动作覆盖仅记录本地意图，不编译为设备载荷。

T08 本地宏使用现有 graph.actions 的共享 UUID，不新增持久化文件：`ActionKind::LocalMacro` 的 payload 包含 `macro_sequence`（`format_version=1`、有序 `events`）。事件仅为本地 `press { key }`、`release { key }` 和 `delay { ms: UInt32 }`；key 是用户本地标签，毫秒类型不是固件范围。`mi-model/src/macro_sequence.rs` 校验顺序与配对，`macro_library.rs` 提供共享引用、原子保存并绑定、解绑和引用保护删除；`save_macro_action` / `bind_macro_action` / `delete_macro_action` 通过现有 DocumentStore 保存。未知原宏继续只读保留 raw；显式本地意图不转换设备格式。编辑共享宏前显示全部受影响控件，未绑定宏保留在图中，结构删除不会暗中删除宏。

测试：v1 fixture 迁移语义等价、原件备份存在、失败保全、高版本拒写、结构删除引用拒绝、整次撤销/重做、导入重映射、损坏/锁/冲突失败不变内存。

## D07 · 比较结果和按钮（F07；#21/#22）

比较基于相同设备及可对照源 ID 的 base B、本地 L、设备 R；身份不确定、源 ID 歧义、未知 raw 变化一律人工处理，不自动合并。

| 对象/字段相对 B | 预选结果 |
| --- | --- |
| 仅 L 变 | L |
| 仅 R 变 | R |
| L/R 变为同值 | 此共同值 |
| L/R 不同值 | 冲突，禁止自动提交 |
| 删除与未变 | 删除；引用校验失败则冲突 |
| 删除与修改或 ID 对不上 | 冲突 |
| 未知 raw 改变 | 冲突，保留双方 raw |

“合并无冲突变化”：仅全体无冲突且引用有效时启用；base=R，draft=合并结果。“保留整份本地草稿”：显式确认忽略设备侧可编辑变化；base=R，draft=L，未知 raw 仍以 R 保存且只读，不用 L 覆盖。“采用整份设备版本”：显式确认舍弃本地编辑；base=R，draft=R。“取消”：B/L/revision 全不变。

前三种操作先保存前态恢复副本，再原子保存新文档；成功 revision+1、清空旧撤销栈、diff 由新 base 与 draft 重算。保留本地仍有 diff；采用设备 diff 为空。保存失败不改变内存。

#22 写前再次核验设备身份和文件摘要，变化即拒绝应用并要求重做比较；不能认为一次比较永久有效。

## D08 · 依赖与适配最低范围（F08/F09/F10）

规则 R-K06/R-X04。

静态变更：#19 的 blocker #14 换成 #4，保留 #16；#30 增加 #20。正常依赖数由 92 变为 93。

#19 负责逻辑编辑，不交付未经核验的实体方向映射。#14 仍拥有全部真实控件的输入验收。若当前映射不足，#14 明确记录缺失并增加 #23 blocker；#23 在未来获准写入且 #22 已能可靠恢复后准备最小绑定。该条件分支不再形成 #14→#19→#23→#14 环。#26 及最终 #44 不跳过 #14。

| 票 | 最低产品结果（已批准要求，不是接口已经可用） | 通过依据 |
| --- | --- | --- |
| #30 通用预设 | 安装到草稿；聚焦指定应用、向当前确认焦点插入文本、发送一个用户配置快捷键 | Codex/Cursor/Antigravity 各验；另选当前实际使用的一个终端和编辑器，记录名称版本；无自动提交文本 |
| #31 Herdr | 列出可选窗格/会话、聚焦指定目标、向指定目标放入文本而不提交 | 两个目标交叉实测，非目标无变化 |
| #32 DSH | 列出可选会话、聚焦指定会话、向指定会话放入文本而不提交 | 同上，目标失效明确失败 |
| #33 ZCode | 同 #32 | 同上 |
| #34 QwenWork | 先锁定准确产品/仓库/版本，再完成同 #32 集合 | 身份和接口当前 UNKNOWN，不误认其他 Qwen 产品 |

操作机制仅采用核实的公开接口或明确的焦点操作；不臆造 CLI/RPC。无法保证指定目标的快捷键不充当专用会话控制。缺少最低能力时保持票未完成，并提交明确的范围调整；不能用“不支持”满足原目标。Task/Command/Skill 的类型保留；本轮不增加任务调度或执行。

#29 即未来 Agent 草稿接口，首选一次性本地 CLI `mi draft read/validate/patch` + 版本化 JSON stdin/stdout，复用 Rust 文档服务的 revision 检查和锁；不开放网络服务、通用设备 RPC 或任务执行。本轮只固定接口形态；JSON 字段、错误码及客户端样例在其开工前单独补齐，未补齐前不标 ready。

## D09 · 远期交互的确定语义（F12/F13/F14）

规则 R-E11/R-K07/R-K08。

#24 灯光：预览前实读基线；不能实读恢复所需状态则禁用。取消只在同一设备当前状态可确认仍属于本次预览时恢复并读回；外部修改/断连不盲写、不宣称已恢复。未知并发条件作为未通过项，不以猜测绕过。

#36 提示：设备驻留映射来自已核验活动配置；宿主映射来自当前启用的宿主 revision；草稿不是事实。缺失、过期、来源不一致明确显示。浏览不改变生效来源。

#37 自动切换：默认关闭；手动覆盖持续到用户点“恢复自动切换”，跨前台事件和重启保留。自动模式只管理已批准的宿主配置范围，不暗含设备写入。

## D10 · 落地与验收

落地要求：更新相关工单正文与原生依赖；所有 35 票统一来源措辞；#44 保留所有故事的唯一完整覆盖索引。SPEC 与 BUILD_PROMPT 同步 D01–D09 的行为和验收，不改已有 SEEN/INFERRED/UNKNOWN 等级来迎合计划。

技术准备按票评估。当前 #3/#4 补完明确契约后可进入实施候选；缺 native adapter 证据的子项仍受门槛约束。远期票没有文件/API/检查点或证据不足时保持 needs-info，说明具体缺什么。这里的报告不是 35 张工单整体通过零决策审查的证明。

规划验收：35 票映射、48 故事保留；静态 DAG 无环；激活条件边 #23→#14 后也无环；每条发现有明确落点；链接与来源边界有效；GitHub 回读正文/标签/依赖一致。只修改文档时不把代码测试或原生验收标为本轮运行。复审单列结果，未复审不得宣称 EGM 已通过。
