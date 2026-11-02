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
