# EGM bootstrap 与漂移报告 — 2026-11-03

交付基线：`origin/main` 的 `f61331f0d5a9ac7e48f469b1ff274aa0a20d46a6`。PR 只新增文档，不带入集成分支业务代码。

用户指定目录的首次扫描基线为 `codex/product-integration` 的 `499483535efc376f65c3658660848d129642311e` 加当时已有未提交实现；该工作区的结论单列于下文，不能套用于 PR 源码。原工作区未重置或切换分支。

Bootstrap 首次 BFS 发现 38 个子目录，按预授权执行全树；交付 worktree BFS 发现 23 个子目录，同样执行全树。排除隐藏/忽略、生成、纯测试、fixtures 与无源码的文档目录，保留源码中间节点。首次生成 19 份 EGM.md；交付基线没有 mi-model/src/bin，因此交付 18 份。使用 EGM.md 保留全部手写 README，未修改它们。

核对范围：所有交付 EGM.md、README、development、implementation-plan、protocol-readonly，以及 SPEC/BUILD_PROMPT/UNKNOWNS 的实现要求。EVIDENCE/reference-repositories 仅核对其历史/外部证据边界，未重放硬件事实、复核第三方仓库或查询工单状态。没有把现有文档的未来目标写成已交付能力。

最终源码边界由三个独立 fresh-context、只读 scout 完整读取：四个生产 Swift 文件和包配置；mi-model 三个生产文件；其余四 crate 的生产 Rust/C/build/bin 和 manifests。测试文件及生成代码不作为契约目标。初次大工作区扫描因槽位占用由 worker 内提取；最终交付基线额外完成独立 scout 核对。fresh context 不证明宿主全局指令隔离。

验证：逐份回读 18 份契约，检查章节、代码符号依据和相对链接；`python3 scripts/check-boundaries.py` 通过。只做静态文档检查，未运行全量构建、应用、设备或宿主动作验收。以下仅列差异；“计划差距”是技能要求记录的设计与实现距离，不等于回归或越权。

## README.md（交付基线）

No drift detected.

## docs/development.md（交付基线）

No drift detected.

## docs/implementation-plan.md（交付基线）

- **D01，计划差距**：独立身份/配置操作尚未实现。`Store.swift:Backend` 无 readIdentity，`Store.connect` 自动 readConfig；`mi-ffi::read_identity` 仍返回 JSON 字符串，`UnverifiedSchema` 拒绝身份构造。下一步应按 D01 引入身份 DTO、明确状态和显式读取，不能以现有内部 connect 方法签收。
- **D01，计划差距**：`mi-transport/src/lib.rs:is_candidate` 只检查 VID、两种 PID 和 usage_page；没有完整 usage pairs。`MacTransport::open` 可打开两个 PID 的 USB 候选，而设计首验限定 8298、8297 仅展示。
- **D02，计划差距**：没有 BackendCapabilities 或参数/文件名生产守卫；`Method` 含 FsRead，`RpcRequest` 接任意 JSON params。当前出口依赖 UnimplementedFramer/UnverifiedSchema 关闭，不能宣称四方法/两文件行为守卫已经实现。
- **D03，计划差距**：`DeviceSchema::files` 为文件名列表、bytes 为整块数据；`read_config_snapshot` 先 keymap 后可选 smart，仅每文件一请求。FileMeta/ReadChunk、Base64/SHA-1、大小/offset 校验、二次 list 和 4/8 MiB、3072 byte、60 秒预算尚未实现；SessionConfig 默认接收 timeout 仍为2秒。
- **D04，计划差距**：`DeviceSession::exchange` 请求期间独占 read/feed，无独立 reader、pending oneshot/generation 表、空闲通知接收或可关闭输入订阅；broadcast 容量32而非256，无 InputStreamGap 策略。ID耗尽只返回错误，未结束会话；watcher Drop 设停止位但不 join。
- **D05，计划差距**：manifest 无 synthetic-backend feature/桥接测试工厂；`Store.draftCore` 仍使用 `as? MicroInput`，文档方法未进入 Backend 协议；不能据已有 MockLink 认定 D05 真实桥接合成路径完成。
- **D06/D07，计划差距**：`mi-model::Draft` 仍为 schema v1、config+edits，来源 ID+logical-N 定位；无 v2 UUID 图/source_refs、迁移备份、B/L/R 比较或接受新基线事务。撤销只覆盖 edits。
- **D08/D09，计划差距**：当前无预设安装器、专用工具适配器、本地 mi CLI、灯光预览、映射浮窗和自动切换；动作目录中的工具项只是意图。文档开头已注明这些为未来契约，保持待实施，不改写为当前能力。

## docs/protocol-readonly.md（交付基线）

- **§2/§6，计划差距**：实际 `UnimplementedFramer` 的 encode/feed 全部拒绝。`JsonCodec::decode` 不支持响应 compact i，不校验 result/error 恰一或 error code/message；静态协议说明尚未转为生产实现。
- **§2，计划差距**：文档要求超时清缓冲，`DeviceSession::exchange` 的 timeout 没有 clear 或断连；仅明确断连路径清 framing。不能以文档要求证明已具备超时后隔离。
- **§1/§3/§4/§7，计划差距**：完整 usage pairs、身份可选字段/回退、分块/摘要/二次 list 尚未落地，具体源码位置同上 D01/D03。该文档对 Input/社区行为的描述是外部证据，不应复制成当前 Rust 契约。

## docs/spec/SPEC.md（交付基线）

- R-D13/R-E09/R-E10/R-K06–08/R-X04 对应的唯一 reader、v2、三方比较、工具适配、浮窗/自动切换和本地 CLI 仍缺实现；定位见 implementation-plan 差距。SPEC 明确为目标产品，未把这些缺口判为已交付回归。

## docs/spec/BUILD_PROMPT.md（交付基线）

- AC-R01–R11 是待实施验收：当前源码尚无独立身份/读取事务、唯一 reader、v2/比较、专用适配器/CLI 与后续交互。当前静态扫描不能补签 S1/S2 父阶段或硬件验收；历史 S1a/S2a 回执未在本次重放。

## docs/spec/UNKNOWNS.md（交付基线）

No drift detected.

## docs/spec/EVIDENCE.md（交付基线，限代码边界）

No drift detected.

## docs/reference-repositories.md（交付基线，限参考与实现边界）

No drift detected.

## README.md（首次指定工作区；不属于交付基线）

- 原文仍有“无法执行动作或写入设备”，但 `Store.startHostInputLoop` 已调用 `LiveHostActionExecutor.performLive`。应分别表述宿主执行与设备 Flash 只读。
- 原文称菜单栏与工作台都有宿主开关、三套预设和宿主 Profile 切换；实际三项入口只在 `MicroInputApp.swift:StatusPanel`，Workbench 没有对应控制。

## docs/implementation-plan.md（首次指定工作区；不属于交付基线）

- **P1**：T09 仍规定 Multi 不接宿主执行；`host_router.rs:convert_action` 已把 tap/Action 引用递归转换成宿主动作，未实现其他分支与时序，`action_library.rs:capability` 却仍标不可执行。需明确产品契约或修实现，本轮不修代码。
- **P1**：D01要求缺失活动索引保持未知；`host_router.rs:check_passthrough_health` 对 profile/layer 索引使用 unwrap_or(0)，可能把首层判为 Ready，FFI原样传入缺失值。不能将首层代替已确认活动层。
- **P2**：D06一次确认一次 revision；`preset.rs:install_agent_starter_profiles` 多次调用 Draft 结构/绑定变更，产生多次 revision/undo。外层 FFI DocumentStore::update 和 Swift Store 都不压缩事务，一次撤销仅回退最后一次分配。

## docs/module-contracts.md（首次指定工作区；不属于交付基线）

- 当前生产 decoder 仍不解释物理按键的说明已落后：`mi-session/src/input.rs` 有 candidate/verified 两条路径；FFI 已导出 verified 订阅。入口本身不校验固件/健康/执行开关，应准确说明条件与实机证据边界。
- 模块职责表仍停留先前研究轮，未纳入 host_router、宿主偏好和预设安装。应注明历史时点并连接当前契约，不能把历史“本轮不新增”误作永久限制。
- Framer 的 Lines/行术语与 `NativeFramer::feed` 的完整 JSON 对象边界有歧义；Line 只是 Vec<u8> 别名，不要求 LF。建议修正文案，不改变 wire 行为。

## docs/protocol-readonly.md（首次指定工作区；不属于交付基线）

- 第二个输入边界段仍说生产仅 UnverifiedInputDecoder/all Unknown；实际默认 CandidateInputDecoder，另有 VerifiedInputDecoder 和宽键合并。
- §2要求每次超时清通道；完整发送后的 exchange timeout 仅清 pending，保留 framer/session。应统一该要求与后段允许显式重试的描述。

## docs/designs/host-execution-and-agent-presets.md（首次指定工作区；不属于交付基线）

- QA目标副作用回读尚未实现于 `MI_SYNTHETIC_SCENARIO=host-execution`：执行器直接返回 simulated，不走 CGEvent/剪贴板/CLI。不能把该场景 PASS 当真实副作用已验。
- 设计称注入 AG00–AG18，`Store.prepareHostExecutionScenario` 仅用 AG00/01/02/06/10/11/15 子集。
- 设计的会话枚举/非提交文本注入 API 不在 AgentAdapters 中；实际只有 executeCommand/focusSession，argv 为 Herdr focus-session、DSH session。actor 可在 await 重入，没有专用串行进程队列；2秒后 terminate 即返回，没有退出确认。
- gap 后实际重订阅 subscribeVerifiedInput，不是文档中的 subscribeInput；宿主循环无告警计数，候选面板计数不能代替。
- paste_text 直接作用前台，无目标应用确认；300ms仅为 openApplication 返回后的轮询预算，不是全程 deadline。
- ResolvedHostActionPayload/PassThroughHealthView 实际定义于 HostActionExecutor.swift，而非 Model.swift。

## EGM.md（交付契约）

No drift detected.


## app/EGM.md（交付契约）

No drift detected.


## app/MicroInput/EGM.md（交付契约）

No drift detected.


## app/MicroInput/Sources/EGM.md（交付契约）

No drift detected.


## app/MicroInput/Sources/MicroInput/EGM.md（交付契约）

No drift detected.


## crates/EGM.md（交付契约）

No drift detected.


## crates/mi-ffi/EGM.md（交付契约）

No drift detected.


## crates/mi-ffi/src/EGM.md（交付契约）

No drift detected.


## crates/mi-ffi/src/bin/EGM.md（交付契约）

No drift detected.


## crates/mi-model/EGM.md（交付契约）

No drift detected.


## crates/mi-model/src/EGM.md（交付契约）

No drift detected.


## crates/mi-proto/EGM.md（交付契约）

No drift detected.


## crates/mi-proto/src/EGM.md（交付契约）

No drift detected.


## crates/mi-session/EGM.md（交付契约）

No drift detected.


## crates/mi-session/src/EGM.md（交付契约）

No drift detected.


## crates/mi-transport/EGM.md（交付契约）

No drift detected.


## crates/mi-transport/src/EGM.md（交付契约）

No drift detected.


## scripts/EGM.md（交付契约）

No drift detected.
