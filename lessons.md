# 项目教训

## Dock 图标轮廓与系统处理

- 起因与后果：2026-11-10 用户发现 Micro Input 与 Koe 的 Dock 图标偏大；图案占画布 90%，启动时直接挂载原始 ICNS。
- 修复：原插画不变，轮廓采用 1024 画布中的 824 像素，圆角同比缩放；启动时使用 `NSWorkspace.icon(forFile:)` 返回的系统图标。
- 验证点：`swift scripts/verify-app-icon.swift target/AppIcon.iconset` 对照系统 Mail 图标的四档轮廓，改前失败、改后通过；构建入口自动运行该检查。安装包 ICNS 实测为 (100,100)–(924,924)，应用正常重启；Dock 专属截图未取得。Seen 1x（2026-11-10）。


## 固定应用层必须覆盖实际动作类型

- 起因与后果：六层托管初版只给 `AgentCommand` 加了目标窗口约束，但现有预设也使用 `Shortcut` 和 `PasteText`；这些键仍可能经过全局输入路径，破坏固定应用层的隔离。
- 修复：三个托管应用的快捷键和文本动作统一解析目标身份，目标与 adapter 冲突时拒绝执行，分别使用目标 PID 或同窗 AX 输入；不以抢到前台焦点代替定向发送。
- 验证点：`crates/mi-macos/tests/managed_controls.rs` 的 `shortcut_and_paste_action_types_never_use_global_input_for_managed_targets` 让全局输入、剪贴板和应用启动接口一旦被调用就失败；同时核对真实预设生成的动作类型。Seen 1x（2026-11-10）。

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

## 取消测试先确认已越过发送边界

- 起因与后果：S3 接入 fsync 操作记录后，旧测试以 10ms 固定超时触发取消，可能在首次发送之前结束，无法验证“发送后取消”的契约。
- 修复：在有界等待内观察合成传输实际出现 `fs.writebin` 后再丢弃 future；核对会话失效、持久记录重开为未知及重复计划零重放。
- 验证点：`cancelling_after_send_closes_session_and_never_replays`。发送前与发送后取消分别判断，不以经过时间猜测操作阶段；合成结果不替代实机拔插。
- 复发记录：Seen 1x（2026-11-03）。

## 预览恢复须比较运行态而非仅比较目标

- 同一活动层也可能被其他客户端改色。只校验层标识会覆盖外部修改，并错误报告恢复成功。
- 合成后端对目标和最后确认运行态执行原子比较，后续更新及结束恢复都受约束；冲突进入 Unknown，不重放。
- 回归：`same_target_external_change_is_never_overwritten` 覆盖更新与恢复两条路径；修复前失败、修复后通过。该合成协议不构成固件具有原子比较能力的证据。Seen 1x（2026-11-03）。
- 2026-11-03：切换水位必须取自唯一 reader 的入站序号，不能取消费者解码进度。未读的旧摇杆离中通知仍须按序更新物理状态；换扇区不能当作回中。回归：`testUnreadPreCutoffNorthRequiresRealCenterBeforeEast` 与 `ingress_watermark_precedes_decode_and_preserves_unread_radial_state`。
- 2026-11-03：异步 Backend 测试替身不自动运行在 MainActor。跨 executor 暴露 continuation 时必须同步发布；无锁轮询会偶发在 `CheckedContinuation.resume` 崩溃。`NavigationTests.DelayedBackend` 使用 NSLock 保证读写一致。

## 绑定 ID 不能仅凭静态排列认定物理左右

- 起因：将首行 AG01/AG00 反转写入“已验证”映射，合成用例又复用了同一假设。真实左键触发了右侧的聚焦动作。
- 实测：2026-11-03，在固件 0.6.3、Profile 0／Layer 1 下逐颗确认：左键 AG00、右键 AG01；候选流和映射流的绑定 ID 一致，按下/释放完整，采集前后活动索引不变。
- 修复：按实机证据修正路由、速查顺序与标签、UI 试运行和合成输入；不写设备，不迁移用户逻辑槽位。
- 回归：`verified_input_maps_observed_row0_order_joystick_sectors_and_merges_wide_key` 修复前因 AG00 被映射到 logical-1 而失败，修复后通过。物理验收一次只提示一颗键，收到回执后再继续；不把复合口令的模糊结果判成证据。

## 原生长请求连续发送与完成块核验

- 2026-11-03：H1 首块 ACK 后目标尚未出现在目录，不能把中间不可见当作整文件失败。每块仍检查原配置与非目标目录，`completed:true` 后才核验目标完整内容。回归：`buffered_uncompleted_chunks_need_no_visible_file_until_completed`。
- 唯一 reader 在同一 HID mutex 上执行 20ms read_timeout；逐报告重新拿锁会在长 RPC 中插入间隔。实际配置恢复曾收到缺少合法 ID / error 400；改为一个 RPC 一次锁定、连续发完后，原字节恢复成功。回归：`multiframe_request_uses_one_transport_batch`。保留严格响应检查，失败不自动重放，不把该修复当作多客户端竞争已解决。

- 2026-11-04：设备写后逐绑定重验仍残留旧首行翻转，导致恢复原配置后 AG00/AG01 仍不可用。混合应用的真实 FFI 合成场景复现后，重验按 E-100 已核验顺序 AG00→slot0、AG01→slot1 修正；同时覆盖修改后仅对应绑定暂停、原值恢复后重新可用。不能只修输入解码而遗漏写后重验。

## 菜单取消不能把确认键重新交给普通绑定

- 起因与后果：径向菜单在消费旋钮确认时发现路由变化，先取消菜单，再按当前“菜单未打开”分支返回，可能将同一确认事件执行为普通旋钮绑定。
- 修复：保留事件进入时的菜单所有权。路由或会话校验取消菜单后，原本属于菜单的确认仍被消费，零菜单动作、零普通绑定动作。
- 验证点：`RadialMenuBridgeTests.testRouteChangeConsumesMenuConfirmationWithoutFallingBackToBinding` 使用真实 UniFFI 草稿为新 Profile 旋钮绑定可执行动作，明确断言普通回退原本可执行，再核对确认零动作。测试只保留一条输入消费路径，保存草稿重新启动的自动订阅须与手动测试流互斥。Seen 1x（2026-11-04）。


## AX 停止后的控件重建与有效叶节点

- 起因与后果：千问停止录音后已回到空闲，但原控件重建使停止确认失败。改为原窗口只读回读后仍失败；只读探针发现有效 AXMenuButton 对未声明的 AXChildren 返回 noValue，被误当成扫描不完整。
- 修复与证据：PR #74 仅在对原控件尝试停止后，允许原进程实例、窗口身份与标题不变时完整扫描唯一空闲控件。noValue 仅在角色仍有效、属性列表成功且未声明 AXChildren 时作为叶节点；其他错误仍未知。最终 a683a857 的千问原生启停通过，替代控件不接收动作。
- 正确做法：连续修复仍未通过时先检查正常空闲状态下的扫描前提；区分没有子节点与读取失败，不根据 AXPress 返回值推定录音结束。
- 验证点：PushToTalkTests.swift 中 testChildrenReadDistinguishesVerifiedLeavesFromMissingOrFailedValues、testStopReadsReplacementButNeverPressesIt 和 testChangedWindowCannotConfirmOrReadReplacement；原生验收同时核对开始/停止回执与最终空闲界面。软件与合成输入不替代实体宽键验收。
- 2026-11-10：六层托管听写的独立评审再次发现缓存控件漏掉录音后新出现的按钮。状态动作改为在同一进程、窗口中重新发现控件；仅普通滚动使用短期缓存。应用重启后，只有确认旧 PID 已退出才解除旧录音约束，不能仅凭新 PID 不同推断旧录音结束。回归见 `crates/mi-macos/tests/managed_controls.rs` 的 `toggle_observes_mouse_changes_and_replacement_controls` 与 `restarted_app_recovers_only_after_the_previous_process_is_confirmed_exited`；安装版录音效果另验。
- 复发记录：Seen 2x（2026-11-04、2026-11-10）。同一次问题的多轮修复不重复计数。


## 旧代异步失败也须保持身份与未确认状态

- 起因与后果：T3 独立审查复现旧轮询在关闭后返回 expired，先把界面设为 connecting 再检查代次；另一条取消签发路径忽略撤销失败，后续空 close 报成功，掩盖仍存活的短期授权。
- 修复与证据：PR #76 在旧回调的任何状态或关闭副作用前校验代次；撤销未确认保留到授权有效期结束，空 close 不清除该事实且期间不再签发。独立原始复现与最终完整门禁均通过。
- 验证点：T3AttentionTests 的 testExpiredOldPollCannotCloseNewGeneration、testLateCancelledIssuanceCannotHideFailedRevocationWithEmptyClose 与 testDisableDuringIssuanceShowsFailedRevocationAfterCancelledLoop。测试须检查状态和副作用次数，不能只断言抛出错误。
- 复发记录：Seen 1x（2026-11-04）。


## 取消执行代时同时移交队列与恢复状态

- 起因与后果：径向菜单集成宏执行主线后，旧任务虽然被取消，恢复时仍能从新代共享队列取走动作。增加尾部代次门后，又暴露当前溢出代取消时无人清理 overflow，后续 dispatch 永久关闭。
- 修复与证据：取队列、等待后执行、T3 准入及收尾都检查当前执行代；取消时同步收回本代 overflow 并刷新输入水位。旧回调不修改新队列或覆盖最近动作。最终 1f1b758 的五项集成定向回归与完整检查通过。
- 验证点：RadialMenuTests 的 testCancelledPredecessorCannotConsumeReplacementRadialQueue、testCancelledPredecessorCannotClearReplacementOverflow、testCancellingOverflowOwnerReleasesItsGateAndRefreshesInputCutoff。分别覆盖旧代不能动新状态，以及当前代取消后能够恢复，不能只测其中一侧。T3 睡眠测试须等待取消前捕获的任务句柄，再断言 lease 为零，不能等待已清空的当前任务引用。
- 复发记录：Seen 1x（2026-11-04）。两次补丁属于同一集成事件，不重复计数。

## 对标控制深度前先固定目标应用

- 用户纠正：Paseo 仓库只用于对标控制深度；实际目标是 Antigravity、Cindy、ZCode 和桌面版 DeepSeek Harness，不使用 Paseo，也不要求复制它全部 MCP/CLI 产品出口。
- 起因：把参考实现的模块清单直接变成交付范围，并以通用宿主动作替代应用内部会话语义；遗漏 Cindy 已有原生集成的能力归属。
- 正确做法：先核对目标桌面版本、原生能力、源码和接口权限范围，再确定需要补充的能力。创建自有 runtime 的 SDK 或只能管理调用方会话的 API，不能充当控制用户现有桌面会话的接口。
- 验证点：四应用接入证据必须列出稳定目标身份、状态来源、动作回执和原生所有者；未能操作既有会话时保留缺口。Seen 1x（2026-11-05）。

## 原生所有权释放必须包含迟到操作的停止确认

- 起因与后果：所有权撤销时灯光预览可能仍在启动。若迟到 token 没有先保存在 Store，停止调用失败后界面仍为空，排空检查会误报成功。
- 修复：先保留 token，再走统一停止路径；只有明确 Stopped/Restored 才确认清理。握手同时等待宿主动作计数、PTT 的迟到启动和停止确认、设备操作及预览启动结束。
- 验证点：`testOwnershipDrainRetainsLatePreviewWhenStopFails`、`testOwnershipHandoffWaitsForLateVoiceStartAndRejectsUnconfirmedStop`、`testDrainWaitsForAlreadyAdmittedActionToFinish`。原生及实体证据单列，不用合成 peer 的释放回执替代 Cindy 实际排空。
- 复发记录：Seen 1x（2026-11-05）。

## 接力记录必须给新切片补写证据字段

- 起因：新版队列的未完成切片没有预置 Evidence 行，advance 只替换已有字段，标记完成后生成的下一棒提示证据为空。
- 修复：缺失时插入，已有时替换；重复执行不产生重复字段。
- 验证点：`test_advance_adds_missing_evidence_and_updates_it_without_duplicates` 与 `scripts/pstack-relay selftest`。Seen 1x（2026-11-05）。

## 订阅关闭与重新连接必须绑定同一个订阅身份

- 起因：DSH 桥初版只绑定授权和 Host 代次。旧关闭请求迟到时可能关闭新订阅；只给关闭增加身份后，迟到的旧订阅请求仍可能替换新订阅。
- 修复：每次订阅生成独立 ID，原生端拒绝其他 ID 替换活动订阅。控制器先等待旧连接结束，再关闭其订阅并建立新连接；清理未确认时保持关闭，不接管未知订阅。
- 验证点：`DSHSessionTests.testReconnectWaitsForLateSubscribeBeforeClosingItsSubscription`、`scripts/verify-dsh-native` 的旧订阅回归，以及 `scripts/verify-dsh-paired` 的迟到关闭和竞争订阅检查。Seen 1x（2026-11-05）。

## 只请求一个投影不代表只读取元数据

- 起因：DSH `stateOf(session, key)` 会先生成全部已注册投影，缺缓存时遍历内存历史。桥没有调用 transcript API，仍不足以证明没有读取正文。
- 修复：增加只读取当前缓存的 `cachedStateOf`，缓存不存在或水位不符时返回未知。待回应计数由生产者和当前缓存提供；桥不主动补历史。状态观察在同一事件的投影全部更新后同步读取，保留连续变化。
- 验证点：`scripts/verify-dsh-native` 包含未生成缓存会话的历史读取陷阱、连接前已存在的审批，以及连续待回应变化回归。安装版与实体结果单列。Seen 1x（2026-11-05）。

## 辅助功能授权需清理旧 cdhash 登记并响应式刷新

- 起因：早期 ad-hoc 签名首次写入 TCC 时记录了固定 `cdhash`，后续切到 `Apple Development` 签名后，在系统设置中只拨动开关不会更新 `TCC.db` 的 `csreq`（`tccd` 报 `SecStaticCodeCheckValidity status: -67050`）；同时 `/Applications/MicroInput.app` 与 `target/MicroInput.app` 多副本争夺同 bundle ID，且界面直接读静态 `AXIsProcessTrusted()` 未随授权变化刷新。
- 修复：清理重复应用包并在 `scripts/build` 构建后注销 `target/MicroInput.app` 的 LaunchServices 登记；通过 `tccutil reset Accessibility craft.semantic.micro-input.dev` 重置旧 `cdhash` 使 `tccd` 改按 `Apple Development` 证书 requirement 登记；`Store` 发布 `@Published var accessibilityTrusted` 并在未授权时轮询、切回前台及打开菜单栏时刷新，同时提供一键「重置授权」入口。
- 验证点：`HostExecutionTests.testAccessibilityTrustRefreshesReactivelyOnMonitorAndForegroundChange` 及 `tccd` 日志核对 `CodeReq` 为 `Apple Development` 证书规则。Seen 1x（2026-11-06）。

## 原生所有权围栏重启续接与集成补丁未跟踪文件门禁

- 标签：#macOS #验收 #自动化
- 起因与后果：一是 `NativeOwnershipController` 每次启动生成随机内存 `clientInstance = UUID()`，而 `cindy-ownership.sock.fence` 跨进程保留旧 `clientInstance`，导致 `Micro Input.app` 重启后被 `different_client` 锁死；同时 Cindy 仅在 `MainLayout` 和 `VoiceInputOverlay` 注册所有权排空桥，停留在 `#/login` 或带预热 `#/sidebar-window` 时 `quiesceOwnershipRenderers` 恒超时返回 `native_drain_unconfirmed`。二是 `9393bf5` 在未 `git add -N` 新文件的工作树上用 `git diff HEAD` 覆盖 `integrations/{cindy,dsh}/*.patch`，静默删掉新增桥接与夹具脚本；三是 `Workbench.task` 启动时无条件重跑 `prepareDesktopProfiles()` 覆盖了 `USB 透传层已就绪` 状态栏。
- 修复与证据：`UnixNativeOwnershipTransport.readPersistedFence()` 在控制器初始化时恢复本机 `0600` `.fence` 的 `clientInstance` 并推进 `generation &+ 1`；`NativeOwnershipController.isUnbridgedDesktopActive` 移除 `!bridgeObserved` 与 `nativeIdentity == nil` 粘滞门禁，在 `cindy-ownership.sock` 消失时自动清空旧桥接状态并恢复本地桌面控制，重新出现时立即重新围栏；Cindy `App.tsx` 顶层挂载 `ensureWorkLouderOwnershipBridge()`；`Store.prepareDesktopProfiles()` 以 `hasAllDesktopAgentPresets` 幂等守卫；`FourAppVoiceManager.startDictation` 必须等待 `PushToTalkController.waitForStartOutcome()` 确认启动后才进入 `.recording`；`scripts/verify-{cindy,dsh,zcode}-*` 自动 `git add -N` 补丁目标，`scripts/check-boundaries.py` 增加 `R-C01`–`R-C04` 静态门禁。
- 验证点：`NativeOwnershipTests.testRestartedControllerResumesPersistedFenceIdentityAndGeneration`、`NativeOwnershipTests.testUnbridgedDesktopRecoversWhenBridgeSocketDisappearsAndRefencesOnReappear`、`FourAppVoiceTests.testStartDictationFailsClosedWhenOwnershipBlockedOrDriverRejects`、`HostExecutionTests.testPrepareDesktopProfilesDoesNotClobberReadyStatusWhenPresetsExist` 与 `python3 scripts/check-boundaries.py`。Seen 3x（2026-11-06）。

## 宿主单实例文件锁与启动器严格参数门禁

- 标签：#macOS #验收 #自动化
- 起因与后果：仅靠 `scripts/build` 注销 `target/MicroInput.app` 的 LaunchServices 登记不足以阻止多实例双开。当 Agent 执行 `./scripts/run --list && ./scripts/run --kill` 清理遗留实例时，旧 `scripts/run` 仅用 `'--verify' in sys.argv` 判断模式，未知参数被静默忽略并直接用 `subprocess.Popen` 拉起 `target/MicroInput.app/Contents/MacOS/MicroInput`（且继承了宿主 IDE 的 `__CFBundleIdentifier=com.google.antigravity`），绕过 LaunchServices 单实例合并，与 `/Applications/Micro Input.app` 同时驻留菜单栏产生两个图标。
- 修复与证据：`MicroInputApp.init()` 在创建 `Store` 与 `StatusMenu` 前对 `Store.defaultDocumentDirectoryURL()` 下的 `instance.lock` 获取非阻塞 `flock(LOCK_EX | LOCK_NB)`（`DocumentInstanceLock`），若已被占用则激活现有实例并立即退出（隔离 QA 因设置 `MI_DOCUMENT_DIR` 拥有独立锁，互不干扰）；`scripts/run` 改为 `argparse.ArgumentParser(allow_abbrev=False)` 严格解析参数，实现不拉起应用的 `--list` / `--kill`（仅管理 `target/MicroInput.app` 与 `.scratch/qa-runs/`，绝不误杀 `/Applications/Micro Input.app`），剥离继承的 `__CFBundleIdentifier` / `XPC_SERVICE_NAME`，并在非 `--verify` 模式下预检 `instance.lock`；`scripts/check-boundaries.py` 新增 `R-C05` 门禁。
- 验证点：`scripts/tests/test_run_verify_launcher.py`、`HostExecutionTests.testDocumentInstanceLockRejectsDuplicateOwnerUntilReleased` 与 `python3 scripts/check-boundaries.py`。Seen 3x（2026-11-06）。


## SwiftUI 细粒度域切片观察与 FFI 代次短路门禁

- 标签：#macOS #性能 #架构
- 起因与后果：`CoreStore` 最初使用 `ObservableObject` + 单一 `@Published var model: ProjectionModel` 并向 `CreatorMicroStageView` / `CreatorMicroBoardView` / `CreatorMicroInlineInspectorView` 传递整棵 `ProjectionModel`，导致在 Inspector 搜索框每输入一个字符就触发 9 个顶层与子视图（含隐藏在 `NSHostingController` 内的 `StatusMenuPopoverView`）共 45 次 `body` 重算（耗时 67.52ms）；同时 `mi_core_dispatch_json` 与 `poll_hardware_events` 未按投影代次短路，重复点击已选中槽位、窗口焦点切换及空前台通知仍跨 FFI 序列化并反序列化完整 JSON（耗时 34.34ms）。
- 修复与证据：对标 `mattt/paseo-creator-micro`（`@MainActor @Observable` + `@ObservationIgnored` + `guard newState != state`）与 `metasequoiaime/msime`（C-ABI 快照代次门禁 + AppKit 相等性守卫），将 `CoreStore` 迁移为 `@Observable`，拆分为 8 个 `Equatable` 域切片（`header`、`sidebar`、`canvas`、`sessionStrip`、`inspector`、`statusMenu`、`overlay`、`footer`）及 `activeModal`，仅当切片 `!=` 时才写入触发对应观察者；Rust `WorkbenchStore` 维护 `projection_generation` 与 `last_status_item_snapshot` 相等性门禁，`mi_core_dispatch_json` 与 `poll_hardware_events` 在代次未推进时直接返回 `NULL`。改造后 Inspector 连续 5 次搜索仅触发 `CreatorMicroInlineInspectorView` 5 次重算（其余 8 个视图重算为 0，耗时降至 25.39ms），4 类无状态变化事件 FFI 解码与视图重算均为 0（耗时从 34.34ms 降至 0.58ms）。
- 验证点：`MicroInputTests.testRenderInvalidationAndLatencyProfile`、`crates/mi-ffi/src/lib.rs`（`ffi_roundtrip_projects_and_dispatches_commands`）与 `scripts/check-boundaries.py`（`R-C06`）。Seen 1x（2026-11-09）。

## 生产环境启动即插即用连接与全自动硬件验收

- 标签：#设备 #macOS #验收 #自动化
- 起因与后果：一是 `WorkbenchStoreConfig::production()` 默认配置了 `ownership_socket_path = Some(<document_dir>/cindy-ownership.sock)`，而 `WorkbenchStore::open` 曾用 `store.config.ownership_socket_path.is_none()` 作为是否调用 `connect_default_usb()` 的守卫，导致生产应用启动时虽启动了热插拔监听，但对启动前已插好的 Creator Micro 2（`303A:8298`）不会自动连接；二是当 `cindy-ownership.sock` 存在时，启动首轮所有权握手走 `12ms` 异步后台快速轮询，连接瞬间 `lighting_arbiter.last_hardware_sync_json` 尚未生成，若直接跳过灯光同步会导致首轮 `v.oai.rgbcfg` 未验证。
- 修复与证据：在 `WorkbenchStoreConfig` 中显式引入 `pub auto_connect_usb: bool`（`production()` 为 `true`，`isolated()` 与单测夹具为 `false`）；`WorkbenchStore::connect_with_transport` 建立连接后在 `tokio_runtime` 后台自动执行 `run_automatic_hardware_acceptance`（依次核验 `read_identity_info` 固件 `0.6.3`、`read_config_snapshot` 多分块 `keymap.json` SHA-1/SHA-256 与 `smart_actions.json`、以及 `send_oai_rgbcfg` / `send_lights_preview` / `send_oai_thstatus` 三项灯光 RPC，未生成仲裁载荷时回退 `default_acceptance_lighting_payload` 并在 `HardwareAutoAccepted` 与 `OwnershipReply` 到达后刷新实时会话灯光），写出 `<document_dir>/hardware-auto-acceptance.json`；新增一键插上键盘全自动验收脚本 `scripts/verify-hardware`（串联 `H0` 只读身份与配置备份、`H1` 临时文件多分块写读删与 14 次快照零污染校验、以及生产 `Micro Input.app` 自动连接与灯光同步回执校验）。
- 验证点：`crates/mi-core/tests/workbench_store.rs`（`usb_connect_automatically_verifies_identity_snapshot_lighting_and_writes_acceptance_receipt`）、`scripts/tests/test_verify_hardware_launcher.py` 与实机 `./scripts/verify-hardware`（产出 `.scratch/hardware-acceptance/latest-result.json` 与 `~/Library/Application Support/MicroInput/hardware-auto-acceptance.json`）。Seen 1x（2026-11-09）。

## SwiftUI 平面按钮矩形热区、Inspector 搜索防抢占与 Dock 恢复生命周期门禁

- 标签：#macOS #UI #验收 #自动化
- 起因与后果：一是 macOS SwiftUI 中 `.buttonStyle(.plain)` 与 `.onTapGesture` 若未显式附加 `.contentShape(Rectangle())`，控件内的 `Spacer()`、内边距与圆角四角会成为点击盲区（如 17 键画布格子边缘、旋钮/摇杆四角、Inspector 动作行右侧空白、状态栏磁贴空白处点击无响应）；二是 `Codex` 与 `Cindy` 两个受保护 `KV_OAI_*` 方案在 `CanvasSliceModel` / `InspectorSliceModel` 中未投影 `selectedProfileId` 与 `selectedDisplayName` 且 `taskKeycapSurface` 硬编码 `.apps` 与 `会话 1..6`，导致切换两方案时画布与 Inspector 渲染完全一致；同时首次点击摇杆时连续分发 `SelectSurface: "Joystick"` 与 `SelectJoystickDirection` 跳过了 `North`（上）方向，且 `titleForSlot` 将 `17 (South)` 与 `18 (West)` 标题颠倒；三是物理按键事件 `consume_physical_input_event` 无条件覆写 `selected_surface`，在用户在 Inspector 输入框搜索或录制快捷键时会抢占当前编辑槽位并打断焦点；四是 `MicroInputApp.swift` 缺少 `applicationDockMenu` 与 `win.deminiaturize(nil)`，窗口最小化后点击 Dock 无法自动还原。
- 修复与证据：为 `MicroInputUI` 全部 `.buttonStyle(.plain)` 与 `.onTapGesture` 补齐 `.contentShape(Rectangle())`；在 `CanvasSliceModel` 与 `InspectorSliceModel` 纳入 `selectedProfileId` / `selectedDisplayName`，为 `InlineInspectorView` 补齐 `.id(...)`、`ScrollViewReader` 滚动置顶与搜索框 `.onSubmit` 回车直绑，修正摇杆首次点击与上下左右槽位标题并在画布旋钮/摇杆渲染实时方向指示；在 `consume_physical_input_event` 增加 `!inspector_search_active` 守卫；在 `MicroInputApp.swift` 补齐 `applicationDockMenu`、`win.deminiaturize(nil)` 与 `NSApp.applicationIconImage`；在 `scripts/check-boundaries.py` 固化 `R-C07` 静态门禁。
- 验证点：`MicroInputTests.testSidebarProfileAndSurfaceSwitchDistinctnessAndSearchFocusIsolation`、`python3 scripts/check-boundaries.py`（`R-C07`）及 `./scripts/run --verify` 真实窗口边缘坐标点击验证。Seen 1x（2026-11-10）。


## 生命周期迁移须验证通知入口和旧任务释放

- 起因与后果：Rust 核心迁移删除了 Swift `WorkspaceLifecycle`，但 README 仍声称支持睡眠唤醒。旧 USB 会话、按住听写和六层后台任务没有统一退役入口。
- 修复：Swift 仅转发 workspace 通知，Rust 先关闭输入准入，再等待旧 reader 与后台任务结束。唤醒使用描述符发现器；独占发现器会提前打开接口，不能仅靠忽略发现回调阻止连接。听写清理保留原窗口句柄，不能依赖执行器最近访问的窗口。
- 验证点：`WorkspaceLifecycleTests`、`workspace_lifecycle` 模块测试和 `managed_controls` 的原目标清理测试。覆盖实际通知入口、延迟 reader、迟到运行对象与回执、草稿撤销重做、跨应用切换后的原目标清理。合成验证与真实合盖效果分开记录。
- 复发记录：Seen 1x（2026-11-10）。

## 浏览方案不能激活宿主；QA 窗口须隔离硬件

- 起因与后果：原 `SelectProfile` 同时选择编辑对象与激活宿主，新的设置式界面若复用它浏览，会改变实际控制目标。空层清空编辑选择又暴露所有权维护依赖编辑对象的问题。原 QA 包虽隔离文档，仍以生产 CoreStore 启动并自动连接 USB。
- 修复：新增 `BrowseProfile`，六层预览只切编辑对象，空层明确清空；运行所有权独立于编辑选择。QA 包必须有隔离文档目录并使用 isolated CoreStore，缺目录拒绝启动；日常包保持原初始化路径。
- 验证：mi-core `browsing_profiles_changes_editor_without_activating_host` 与六层空态/所有权回归，mi-ffi 浏览边界回归；`scripts/run --verify` 原生 UI 应显示未连接，不枚举或打开真实键盘。Seen 1x（2026-11-10）。

## 固定键帽与动态标题分离

- 完整会话标题不能放入固定高度键帽后强制不限高度换行。键帽显示编号与短状态，完整标题由自然高度列表承载。
- 布局验收必须覆盖真实投影格式、长中英文标题与所有状态。`SessionKeycapLayoutTests` 已覆盖内容区高度、全文行高及 JSON null 空槽；仍需在原生运行界面检查对应关系。

## 过期按键与连接失效分别处理

- 起因与后果：六层输入排队后还需核验 Cindy 所有权。实机观察到一次事件核验前已等待 1617 毫秒，核验另用 1156 毫秒；超过两秒有效期后，原实现永久停用整个接收器，后续新按键也没有反应。
- 修复：`Activation::is_expired_at` 统一有效期，接收与执行阶段只淘汰过期事件。所有权拒绝、断连、代次终止及溢出继续停止执行。
- 验证点：`expired_clicks_do_not_disable_new_clicks`、`reads_preserve_fresh_clicks_after_an_expired_prefix` 和 `a_click_expiring_during_ownership_check_does_not_stop_fresh_clicks`。必须证明旧动作没有执行，同一代次的后续新动作仍会执行。Seen 1x（2026-11-10）。
- 同轮发现：底层执行器不支持的会话导航曾返回成功而无系统动作。`unsupported_` 回归以真实执行器和记录型系统驱动验证明确拒绝，并用受支持的聚焦输入动作作正对照。明确拒绝不代表导航能力已实现。

## 前台应用与分屏必须分别确定目标

- 起因与后果：六层独立执行器只读取物理层，已有前台通知不能改变它的目标；Antigravity 两个分屏的输入框和语音按钮又被展平成整窗列表，触发 `ambiguous_or_missing_dictation_control`。应用同时运行与控件歧义不能混为同一个原因。
- 修复：第 2–4 托管层在已安装方案中优先匹配前台应用，切换时淘汰旧队列及等待权限核验的事件；AX 捕获保留祖先路径，以唯一焦点分支确定分屏，后续执行和读回固定到同一目标。实时语音复用该路径，不再维护整窗选择器。第 1 层 Codex 原生通道不在此规则控制范围。
- 验证点：`foreground_switch_discards_queued_and_waiting_clicks_without_retargeting`、`split_panes_resolve_only_the_focused_branch`、`recording_cannot_migrate_to_another_pane_in_the_same_window`。多分屏无明确焦点时拒绝；窗口标题变化时旧录音清理仍可能拒绝，不能仅凭 AX 分屏引用宣称会话身份未变。Seen 1x（2026-11-10）。

## 配置查询不能成为实体输入的常驻依赖

- 起因与后果：六层输入已经通过 Karabiner 专用键码携带层位，却仍每 500 毫秒查询共享 Vendor RPC。持续实机验证先后捕获协议响应错误与两秒查询超时，每次都会永久停用输入；持续查询还必然耗尽单会话 999 个编号。
- 修复：首次完整核验配置后关闭 RPC reader，运行期只检查真实 USB、Karabiner、输入代次与睡眠边界；实体事件负责更新最近输入层。删除本轮一度增加的自动重连逻辑，不为多余轮询维护恢复状态机。连接投影依据实际输入通道，不能把已关闭的配置会话当成断开的键盘。
- 验证点：核验后 reader 已退出，多个监视周期零 RPC、输入仍可执行；USB/readiness/输入代次失效仍停用，睡眠和迟到任务仍清理。保留 mi-session 终止原因与延迟 reader 退出回归：可取消的请求 future 不能取走 JoinHandle 后等待。层位文案区分首次核验与最近输入；不承诺实时发现外部工具静默改写设备 Profile。Seen 1x（2026-11-10）。

## 共享响应先判断归属，再检查本请求契约

- 起因与后果：实体聚焦输入成功后，持续连接验证捕获 `invalid response id`。接收器在 pending 归属判断前要求所有 result/error 消息都有合法数字 ID；无法关联的响应能直接终止会话。
- 修复与验证：先分类无法关联响应、通知与本请求响应，再严格解码本请求。回归覆盖缺失/null/字符串 ID、不匹配 ID、匹配别名冲突，以及只有无关联响应时仍超时。无法关联不等于已证实来自 Codex；同 ID 跨客户端歧义仍属 UNKNOWN-17，不能以忽略非本请求响应宣称完全隔离。Seen 1x（2026-11-10）。

## 专用托管按键不依赖无关应用的原生租约

- 起因与后果：六层专用标准键由 Karabiner 抓取，却继承了 Cindy 原生 Vendor 通道的释放回执门禁。用户退出且不再使用 Cindy 后，Antigravity 仍无法连接。
- 修复与验证：六层输入只检查它实际使用的 USB/Karabiner、输入代次、前台目标及用户执行开关，不依赖 Cindy 进程或 socket。第 1 层原生 OAI 映射不变，传统 Vendor 通道的所有权机制单独保留。回归覆盖无所有权桥仍执行、新旧输入代次隔离及真实输入条件失效；不为不再使用的应用新增进程探测框架。Seen 1x（2026-11-10）。
