# 项目教训

## 浏览配置不一定是只读

- 标签：#设备 #验收
- 起因与后果：Input 1.1.0 中点击 Profile 名称会出现 Updating device 倒计时；不能因另有激活按钮就推断名称选择仅影响编辑器。
- 修复与证据：观察结束恢复 Codex，两份本地配置哈希一致；未做固件读回。见 .scratch/input-study/out/layer-lighting-joystick-followup.md。
- 正确做法：自研区分 UI 选中对象、草稿和设备活动配置；写入通过明确入口执行。
- 验证点：后续测试应确认普通页面导航不发送设备写请求，写入后需读回；当前尚未实现这些检查。
- 复发记录：Seen 1x（2026-11-02）。

## 自动化焦点与参数证据

- 标签：#设备 #证据
- 起因与后果：点击无响应时键盘试探误入 Profile 名称；已精确恢复。AX 数值缺失也不足以证明输入校验范围。
- 正确做法：操作前验证焦点；数值约束分别验证编辑、保存、重开及设备执行层，不从单次 AX 显示推断完整范围。
- 验证点：最终配置文件与基线比对，并单列设备读回和会话撤销栈；不要宣称所有状态都未变。
- 证据：同上调研补记；Seen 1x（2026-11-02）。

## 原生键盘与菜单栏不能只看构建

- 标签：#macOS #验收
- 起因与后果：首轮只验鼠标主流程，设备组标签覆盖子控件名称，键盘方向移动不可靠；无主窗的状态菜单也需要明确键盘入口。
- 修复：保留逐控件标签，使用明确快捷键；原生状态面板共用Store，AppLifetime显式保留宿主。
- 验证点：NavigationTests.testClosingLastWindowKeepsHostAlive；原生实测关闭→⇧⌘M→重开保留草稿→退出进程消失，见BUILD_PROMPT S1a回执。
- 复发记录：Seen 1x（2026-11-02）。

## 本地草稿身份与保存边界

- 标签：#配置 #持久化
- 起因：S1a以内存演示数组下标定位，直接扩展会在重排后误绑定；原生重开会丢草稿。
- 修复：绑定使用文档内Profile/Layer ID和逻辑控件ID；保存成功才提交内存修订；未知/损坏文件不覆盖，QA目录隔离。
- 验证点：mi-model document_tests覆盖重排、参数往返、未来字段、损坏和冲突；NavigationTests真实桥接重开；原生文本参数退出重开已验。
- 复发记录：Seen 1x（2026-11-02）。

## 并行测试临时目录

- 起因：全量检查中两个持久化测试获得相同系统时钟值，create_dir 报 AlreadyExists。
- 修复：临时目录增加进程内原子序号，不再仅依赖时钟分辨率隔离并行测试。
- 验证：mi-model 并行测试与 scripts/check；不改变用户文档保存策略。Seen 1x。

## 响应分派先判断请求归属

- 起因与后果：T03原生只读验收中未知method echo可终结reader；旧路径在核对pending ID前校验所有回应，外来响应被当作当前请求错误。
- 修复与证据：先校验响应ID并丢弃非pending响应，再严格验证当前回应。NativeFramer回归覆盖空闲/在途外来ID、同ID不同method与当前ID畸形回应；见mi-session/tests.rs的foreign_response_echo_does_not_kill_reader_or_satisfy_current_request和owned_malformed_response_remains_an_error_after_routing。实际原生流只记录分类元数据，未取得原失败对应来包，原因关联仍有限。
- 正确做法：出站方法白名单约束本应用发送，不用它假定整个共享接收流只包含本应用回应；同ID冲突仍不能靠此消除。
- 验证点：cargo test -p mi-session response；测试必须既确认外来回应不杀会话，也确认当前无效回应仍失败，不能靠放宽echo接受条件通过。
- 复发记录：Seen 1x（2026-11-02）。

## 取消要覆盖原生打开、分帧发送和 UI 代次

- 起因与后果：T05回归复现取消 `spawn_blocking` 的 open future 后迟到句柄未close；读取取消后旧回调仍能占用或清除新会话的busy；请求超时原先只覆盖等回应，卡住的报告发送没有预算。
- 修复：用RAII接管未被会话采纳的句柄；UI按generation结束busy并保留本地草稿；发送与回应共享deadline，中断发送使会话失效，完整发送后才允许在原会话显式重试。
- 验证点：`dropped_open_future_closes_late_native_handle`、`stalled_report_write_is_bounded_and_requires_a_new_session`、`cancelling_incomplete_send_invalidates_session` 和 `SessionLifecycleTests`；不能把合成取消等同于真实拔插。见E-082。Seen 1x（2026-11-02）。

## 本地草稿与设备来源不能复用浏览布尔值

- 起因与后果：从真实快照显式创建本地草稿后，浏览状态已不是设备视图，但原旋钮方向仍未知；按浏览布尔值显示方向会把未知映射标成已验。旧v1的历史DeviceSnapshot来源也不能继续作为当前快照来源。
- 修复：所有视图统一使用旋钮逻辑槽和方向未核实标签；legacy-local投影为LocalDraft，历史来源仅留在不可变origin.config，baseline不伪造。
- 验证点：`legacy_device_source_is_historical_and_never_claims_a_current_snapshot`核对迁移前后投影；真实快照→独立草稿验收时核对来源提示与逻辑槽标签，不能把编辑可用当成物理映射已验。Seen 1x（2026-11-02）。

## 多 worktree 原生验收身份

- 起因：CUA getApp 会按应用身份自动启动；相同 bundle ID 的旧 worktree 实例可能被绑定，退出后再取 UI 状态也可能重启实例。
- 修复：scripts/run --verify 创建独立 bundle ID/显示名和临时文档，打印精确 QA_APP_PATH；限时退出并清理副本。退出后查进程状态，不用 UI 获取已退出应用。
- 验证：独立 QA 路径/标识与实际进程对应；生产 app 未改；QA 验收不冒充生产权限验收。Seen 1x。

## 保留来源模式不应创建新的逻辑槽位

- 起因：T10 接入时，保存“保留来源模式”仍按未激活的扇区数扩展 control_order；复现测试在旧实现失败。
- 修复：仅 logical_sectors 模式可新增槽位；切回来源模式和缩减数量保留原槽位及绑定，界面在来源模式禁用数量输入。
- 验证：`source_mode_does_not_materialize_inactive_sector_intent` 同时检查来源模式不扩展、显式启用才扩展、旧控件身份与绑定不变。Seen 1x（2026-11-02）。

## 导入预览的数据与弹窗使用同一个状态对象

- 起因：T11 首次原生验收时，分别更新 summary/pending 与 isPresented，首个弹窗缺少名称、数量，并可能捕获旧的待导入内容。模型与桥接测试无法验证弹窗首次渲染。
- 修复：用包含内容和摘要的 Identifiable 对象驱动 sheet(item:)，确认操作使用弹窗参数，不从旁路状态取内容。
- 验证：首次打开原创预设必须显示名称与数量；取消后不改草稿，再次打开并确认应创建独立文档。保存范围使用 alert 的 presenting 参数。Seen 1x（2026-11-03）。

## 恢复来源列表保留独立可访问动作

- 起因与后果：T30 原生验收中，SwiftUI List 将来源和“检查”按钮合并成只有行标签的无障碍节点，点击行节点没有执行预览；改按钮样式仍未解除行合并。
- 修复：来源列表使用 ScrollView 和独立检查按钮，保留文件名称、路径与明确动作。
- 验证点：独立 QA 应用的 AX 树须出现“检查 legacy.backup”按钮，按该按钮直接得到版本/来源/摘要预览；不能仅凭坐标点击或编译通过签收。Seen 1x（2026-11-03）。

## 原生验收入口必须走到进程边界

- 标签：#验收 #自动化
- 起因与后果：A 启动器仅做 Python 语法检查和离线 Rust 测试；`Path.open` 不支持 `opener` 参数，真实入口在准备日志时抛出 TypeError，尚未创建子进程或接触设备。
- 修复与证据：改用支持自定义 opener 的内置 `open` 创建 0600 日志；`scripts/tests/test_probe_a_launcher.py` 将实际入口复制到临时根目录，替换子进程和信号边界，验证正常启动、超时停止和退出未确认三条路径，以及 0700/0600 权限。
- 正确做法：语法通过不等于启动入口可运行；先对实际入口运行隔离的系统边界回归，再进行真实设备验收。
- 验证点：`python3 -B -m unittest discover -s scripts/tests -p 'test_probe_a_launcher.py'`；已加入 `scripts/check`，原生设备测试继续保持 ignored。
- 复发记录：Seen 1x（2026-11-03）。


## 窗口重开不能替换应用级生命周期观察者

- 标签：#macOS #生命周期
- 起因与后果：T05 集成评审复现主窗 onAppear 重设同一个 Store，触发观察者替换并取消正在等待 disconnect close/join 的唯一 wake 任务；关闭完成后 Store 仍处于睡眠状态，显式重连被拒绝。
- 修复：AppLifetime 只在 Store 对象身份实际变化时替换观察者，同一对象重设保留观察者及唤醒任务。
- 验证点：`WorkspaceLifecycleTests.testReopeningWindowWithSameStoreDoesNotCancelPendingWake` 在进程内发布合成通知，持住 disconnect，重设相同 Store 后释放，明确重连应成功；本测试不能替代真实 OS 睡眠验收。修复前确定失败，修复后通过。
- 复发记录：Seen 1x（2026-11-03）。

## 真机自动连接只统计已验证候选且兼容出厂 Codex 层

- 标签：#设备 #macOS #验收
- 起因与后果：一是 `Store.autoSetupHostExecutionIfNeeded` 曾同时要求 `devices.count == 1`，在真实 macOS 上因系统自带 `/dev/cu.Bluetooth-Incoming-Port`（`verifiedModel == false`）导致只插一把 `303A:8298` 时跳过自动连接；二是固件 `0.6.3` 出厂 Codex 层（1-based `layer_index: 1`）混用 `KV_OAI_AG00`–`05`、`KV_OAI_ACT06`–`12`、`KV_OAI_ENC_CC/CW/CLK` 与 `v.oai.rad`，若只认 `AG*` 或 0-based 层索引会误报层漂移或漏解码。
- 修复与证据：自动连接守卫改为 `devices.filter(\.verifiedModel).count == 1 && verifiedUSB.count == 1`；`check_passthrough_health` 与 `VerifiedInputDecoder` 同时兼容 `AG*`/`ACT06`–`12`/`ENC_*`/`v.oai.rad` 及 `0.6.3` 1-based 层索引。见 `E-098`、`E-099`。
- 正确做法：多候选安全门禁按 `verifiedModel` 候选计数，不将系统无关串口算作冲突候选；出厂默认层协议须以实机读回的 `keymap.json` 为准。
- 验证点：`HostExecutionTests`、`crates/mi-model/tests/host_router.rs` 及真机 `./scripts/run --verify --timeout=10` 产出 `action_type: "host_ready"`。
- 复发记录：Seen 1x（2026-11-03）。

## 前台应用与辅助功能焦点不是同时就绪

- 起因与后果：本轮桌面 Agent 验收中，NSWorkspace 已确认目标到前台，但 Cindy 的 AXFocusedWindow 和 Codex 输入区 AXFocused 属性尚未更新；立即检查会把成功操作误报为失败。
- 修复：窗口获取与焦点回读各采用有界等待；窗口回退仅接受唯一窗口，输入区必须唯一，最终重新确认前台进程。设置页面没有输入区时保持失败，不凭系统调用返回成功签收。
- 验证：隔离 QA 中实际触发 Cindy / Codex 聚焦，回读执行回执并由 agent-desktop 确认输入区 focused；搜索分别确认可访问搜索框出现。软件操作与真实 HID 事件分开登记。
- 复发记录：Seen 1x（2026-11-03）。

## 手工验收先确认动作发生，再判定结果

- 用户纠正：未及时按键、只按了第一个键，不能解释为设备操作失败。优先通过真实执行器的 UI 试运行自动验证；仅物理输入无法替代的部分请求人工操作，逐个提示并以 HID 回执确认。
- 安装验收：QA 权限不等于日常应用权限。更新后的临时签名可能不匹配旧授权；系统开关打开仍须以安装版实际执行结果确认，必要时在系统设置中重新添加当前应用。

## 菜单栏可访问节点存在不等于按钮完整可见

- 起因：菜单栏 ScrollView 仅指定 maxHeight，原生弹出窗口首次测量时内容高度被压缩，四个 Agent 虽在 AX 树中却被裁切。上轮只检查节点和主窗口截图，漏掉实际弹出菜单。
- 修复：四个常用 Agent 改为固定 2×2 非滚动布局，明确按钮高度并使用内容固有高度；搜索与长诊断退出主面板。
- 验证点：实际打开安装版菜单，截图确认四个按钮全部可见；逐个点击并回读当前配置，关闭主窗口后重复打开。不能仅以 AX 节点存在或构建成功签收。Seen 1x（2026-11-03）。

- 2026-11-03：Agent 品牌名不能证明接口通用。DSH 的 CLI 启动器、Herdr TUI、Herdr App 是不同控制面；预设必须以实际安装的目标和原生控件/快捷键验证，不能用模拟 runner 的成功证明桌面可控。
- 2026-11-03：AXPress 成功不证明听写启动；必须读取启动/停止状态。长会话的正文不能占满操作控件的有界扫描预算。

- 2026-11-03：听写的启动/停止标签可能带当前快捷键后缀。千问办公使用“停止录音（Right ⌘）”；仅匹配无后缀名称会漏掉停止入口和状态。回归：`testQwenDictationRecognizesShortcutSuffixAndBothStates`，真实执行器启动/停止闭环，不用按钮调用成功替代状态确认。

- 2026-11-03：切换水位必须取自唯一 reader 的入站序号，不能取消费者解码进度。未读的旧摇杆离中通知仍须按序更新物理状态；换扇区不能当作回中。回归：`testUnreadPreCutoffNorthRequiresRealCenterBeforeEast` 与 `ingress_watermark_precedes_decode_and_preserves_unread_radial_state`。
- 2026-11-03：异步 Backend 测试替身不自动运行在 MainActor。跨 executor 暴露 continuation 时必须同步发布；无锁轮询会偶发在 `CheckedContinuation.resume` 崩溃。`NavigationTests.DelayedBackend` 使用 NSLock 保证读写一致。

## 绑定 ID 不能仅凭静态排列认定物理左右

- 起因：将首行 AG01/AG00 反转写入“已验证”映射，合成用例又复用了同一假设。真实左键触发了右侧的聚焦动作。
- 实测：2026-11-03，在固件 0.6.3、Profile 0／Layer 1 下逐颗确认：左键 AG00、右键 AG01；候选流和映射流的绑定 ID 一致，按下/释放完整，采集前后活动索引不变。
- 修复：按实机证据修正路由、速查顺序与标签、UI 试运行和合成输入；不写设备，不迁移用户逻辑槽位。
- 回归：`verified_input_maps_observed_row0_order_joystick_sectors_and_merges_wide_key` 修复前因 AG00 被映射到 logical-1 而失败，修复后通过。物理验收一次只提示一颗键，收到回执后再继续；不把复合口令的模糊结果判成证据。
