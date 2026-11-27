# 项目教训

## Antigravity AX 控件标签必须单源双语注册，禁止在匹配器中散落纯英文字面量

- 起因与后果：`crates/mi-macos/src/` 下 `ax_dictation.rs`、`executor.rs`、`managed_controls.rs`、`native_ax.rs` 四处散落硬编码纯英文 Antigravity AX 标签（如 `"Record Audio"`、`"Start Voice mode"`、`"Toggle Side Panel"`、`"Browser"`、`"Stop execution"` 等），当 Antigravity 切换至中文界面时，控件扫描仅剩裸 `AXTextArea`，导致语音听写、实时语音、模型选择、侧边面板、内置浏览器、旁路提问、新建对话及侧栏会话三态成片失效。
- 修复：新增单源双语注册表 `crates/mi-macos/src/antigravity_ax_labels.rs`，以 `BilingualLabelSet { key, en, zh }` 与 `ALL_BILINGUAL_LABEL_SETS` 统一承载全部 Antigravity AX 控件标签及匹配谓词，并将四个源码文件中的全部生产匹配器收敛至该模块；在 `scripts/check-boundaries.py` 新增 `R-C16` 静态门禁，禁止在 `crates/mi-macos/src/` 除注册表外的生产代码中再次散落原始 Antigravity AX 标签字面量。
- 验证点：`antigravity_ax_labels.rs` 单测强制校验全部注册项同时具备非空 `en`/`zh` 与唯一键；`crates/mi-macos/tests/managed_controls.rs`、`native_ax.rs`、`executor.rs` 补齐中文听写、实时语音、模型选择器、侧边面板、内置浏览器、旁路提问、新建对话及侧栏三态回归；`scripts/tests/test_boundaries_launcher.py` 覆盖 `R-C16` 正负用例，并通过 `scripts/check-boundaries.py` 与 `cargo test --locked -p mi-macos`。Seen 1x（2026-11-22）。

## 全局按键映射开关禁止进入键盘焦点链，且菜单栏图标须同步暂停三态

- 起因与后果：`NeutralSwitchToggleStyle` 内部以 `Button` 叠加 `.focusEffectDisabled()` 实现，仅隐藏蓝色焦点框却仍让 AppKit 将主工作台与菜单栏弹层中的全局「按键映射」开关选为窗口 `initialFirstResponder`，窗口位于前台时按下空格会静默关闭整盘 `host_execution_enabled`；同时 `StatusItemIcon` 仅区分 `hasCaution: Bool` 且 `MicroInputApp.swift` 未观察 `store.statusMenu.hostExecutionEnabled`，导致按键映射被暂停后菜单栏图标与悬浮提示仍呈正常状态，用户难以及时察觉。
- 修复：将 `StatusItemIcon` 迁出 `UITheme.swift` 至 `KeycapGlyphView.swift` 并升级为 3 态枚举 `Presentation`（`.caution` > `.paused` > `.active`），为 `.paused` 提供半透明键体配右上角双竖线暂停徽标及本地化悬浮提示；`NeutralSwitchToggleStyle` 新增 `allowsKeyboardFocus: Bool`，在 `false` 分支脱离 `Button` 并保留 `.contentShape(Rectangle())`、`.onTapGesture` 与无障碍按钮语义；主工作台与菜单栏弹层两处全局开关全部启用 `allowsKeyboardFocus: false`，`MicroInputApp.swift` 在 `openMainWindow()` 中调用 `win.makeFirstResponder(nil)` 并通过 `observeStoreStatusItemState()` 联动观察 `hostExecutionEnabled`；`scripts/check-boundaries.py` 新增 `R-C15` 静态门禁守卫开关焦点隔离与状态栏三态绑定。
- 验证点：`scripts/tests/test_boundaries_launcher.py` 覆盖 `R-C15` 正负用例；`DesignSystemVerificationTests` 与 `MicroInputTests` 覆盖 `StatusItemIcon.presentation` 优先级、中英文 `toolTip`、6 态模板缓存与 TIFF 渲染差异，以及真实 `NSWindow` + `NSHostingView` 派发空格 `NSEvent`（`keyCode: 49`）对照回归；全量通过 `scripts/check`。Seen 1x（2026-11-20）。

## 听写租约须以容器身份核销、与非听写控制解耦，并通过常驻菜单栏告警提供清理重试

- 起因与后果：在 Antigravity / DSH 等单窗口 Electron 应用中启动语音听写后，切换会话或自动生成标题会原地改写 `AXWindow.AXTitle`；旧版 `uncleared_targets` 按完整 `ManagedWindowIdentity`（含易变标题）比较，且 `capture_window_for_key` / `capture_full_window_for_key` 复制了听写租约检查，导致用户手动停录或切会话后不仅语音键与发送键报 `blocked:uncleared_dictation_target_changed`，连旋钮模型选择、内置浏览器与侧栏开关也被一并锁死，关闭重开映射触发 `stop_owned_dictations()` 时又因 `CapturedWindow::validate` 严格比对旧标题返回 `blocked:target_window_changed` / `unconfirmed:dictation_target_changed` 形成死锁；同时 `mi-core` 将原始 `blocked:`/`unconfirmed:` 令牌直接写入窗口底栏且缺乏菜单栏常驻提示与手动重试入口，主窗收起时用户无从感知与自愈。
- 修复：`crates/mi-macos/src/managed_controls.rs` 与 `crates/mi-macos/src/native_ax.rs` 将听写租约收敛到 `DictationLeaseRegistry`，按容器身份（`bundle_id + pid + window_id + scope_id`）匹配并在同容器读回明确 `Idle` 时零按压核销陈旧租约，保持活动录音或 `Finishing` 状态标题变化时的保守拒绝，同时将非听写控制（模型选择器、子智能体选择器、内置浏览器、侧栏开关）与听写租约解耦；`crates/mi-core/src/i18n.rs` 与 `crates/mi-core/src/store/` 统一本地化受管错误（零 `blocked:`/`unconfirmed:` 泄漏），新增 12 秒瞬态与粘性清理两级 `HostActionCaution`、菜单栏状态图标角标与卡片、主工作台横幅及 `RetryHostCleanup` / 重新开启按键映射自愈路径。
- 验证点：`crates/mi-macos/tests/managed_controls.rs` 与 `native_ax.rs` 覆盖标题漂移后空闲自动核销、非听写控制解耦、活动录音及 Codex `Finishing` 标题变化保守拒绝与纯容器过滤；`crates/mi-core/src/store/runtime_consistency_tests.rs` 覆盖 `localize_managed_error_detail` 中英文及分号拼接、瞬态告警 TTL 过期与成功清除、粘性清理告警经 `RetryHostCleanup` 与 `set_host_execution_enabled(true)` 自愈；`app/MicroInput/Tests/MicroInputTests/MicroInputTests.swift` 覆盖 `StatusItemIcon` 4 态缓存与 `StatusMenuSliceModel` 切片隔离；全量通过 `scripts/check` 与 `scripts/verify-offline` 并更新 Developer ID 签名的 `/Applications/Micro Input.app` 实机核验。Seen 1x（2026-11-20）。

## Antigravity 2.33 AX 控件扫描必须剪枝客体 WebContentsView 并以标题锚点隔离对话正文

- 起因与后果：将 Antigravity `browser` 动作从 `Cmd+Shift+B` 升级为受管 AX 直达内置浏览器时，在实机 Antigravity 2.33.0 暴露两处 DOM/AX 差异：其一，Electron 的 `wcvBrowserRegistry` 即使在右侧栏折叠时也会将客体浏览器 `WebContentsView` 与 `catcherWcv` 作为同级 `AXWebArea` 挂在 `AXWindow` 下，不剪枝会把客体网页内的 `<textarea>` 或同名按钮混入控件扫描，破坏聊天输入框唯一性并带来网页伪造风险；其二，2.33.0 中 `aria-label="Conversation turns"` 仅为左侧导航圆点轨（与正文同级而非祖先），实际对话正文位于无标签 `AXGroup`（直系子 `AXHeading` 为 `"Conversation Log" | "Agent said" | "Agent working" | "You said"`），代码块 `"Preview"` 则位于 `AXGroup("Markdown view mode")`，仅按旧祖先标签过滤会漏放正文内的同名按钮。
- 修复：`crates/mi-macos/src/native_ax.rs` 新增 `ObservationPurpose::AntigravityControls`，按 `AXURL`/`AXTitle` 仅遍历 Antigravity Hub 本地 `AXWebArea` 并剪枝客体/插件 `AXWebArea`；在 `controls_from_observation` 中以 `O(N)` 扫描直系子 `AXHeading` 建立聊天正文容器集合，连同 `Settings` 与 `Markdown view mode` 一并排除浏览器假阳性候选；`crates/mi-macos/src/managed_controls.rs` 实现 `open_browser` 受管状态机（复用初始快照、未变更发现期 250ms 快速失败、变更后统一 `unconfirmed:` 包装并容忍已按按钮动画期短暂禁用）。
- 验证点：`crates/mi-macos/src/native_ax.rs` 覆盖 `antigravity_controls_observation_prunes_guest_browser_and_catcher_web_areas`、`antigravity_controls_filter_blocks_settings_browser_button_beyond_depth_16` 与 `antigravity_controls_filter_blocks_unlabeled_heading_anchored_chat_log_and_markdown_preview`；`crates/mi-macos/tests/managed_controls.rs` 覆盖 3 步折叠打开、动画期短暂禁用与变更后错误包装；实机通过 `MI_LIVE_AX_VERIFY=1 MI_LIVE_AX_ADAPTER=antigravity MI_LIVE_AX_ACTION=browser` 与 `inspect` 核对 `editable_inputs == 1`，再以 Developer ID 签名更新 `/Applications/Micro Input.app` 并由用户实体按键确认。Seen 1x（2026-11-19）。

## 预设迁移须覆盖真实残留槽位并先核查 USB 传输与硬件死区

- 起因与后果：将 Antigravity 从全层 `MOUSE` 改回 `RADIAL` 侧栏控制后，用户实机仍出现鼠标到处晃。排查真实环境发现三点叠加：其一，日常 `workbench.json` 中 Antigravity 的 `joystick_mode` 为 `"mouse"` 但 `logical-16..19` 仍残留 `agent-model-picker-*`，迁移谓词仅匹配全 `None` 导致迁移跳过；其二，键盘当时仅通过 `Bluetooth Low Energy` 连接（`0xFF00` 写 Flash 仅支持 `TransportHint::Usb`），硬件层仍停留在旧版全层 `MOUSE`；其三，设备 NVS 的 `joystick.deadzone` 为 `0.07`（低于出厂 `0.15`）配合 `stick_speed = 24` 放大静止微漂，且 Antigravity 新建对话页仅渲染左侧 `Toggle Sidebar`、进入会话页才渲染右侧 `Toggle Side Panel`。
- 修复：`antigravity_layer_needs_side_panel_joystick_migration` 与 `SavedSixLayerState::migrate_saved_encoder_defaults` 同步兼容 `mouse` 模式下 `16..=19` 残留 `agent-model-picker-*`（同时保留显式 `host-mouse-move-*` 与用户自定义动作）；插线后执行 `six_layer_runtime::apply` 将第 2 层刷为 `RADIAL`（`KC_F17..KC_F20`）并将设备与源码兜底默认 `joystick.deadzone` 统一恢复为出厂 `0.15`。
- 验证点：`crates/mi-model/tests/preset.rs` 覆盖 `mouse` + `agent-model-picker-*` 迁移及 `host-mouse-move-*` 保留；实机重启 `/Applications/Micro Input.app` 后核对真实 `workbench.json` / `six-layer-runtime.json` 就地迁移，并通过 USB `read_config_snapshot` 与 `read_joystick_deadzone` 读回确认第 2 层为 `RADIAL`、死区 `0.15`。Seen 1x（2026-11-17）。

## 正式安装包连续性与 TCC 辅助功能权限防退化门禁

- 起因与后果：一是 `scripts/verify-hardware` 曾用 `rsync -a --delete` 将开发签名的 `target/MicroInput.app` 覆盖进 `/Applications/Micro Input.app`；二是跨签名家族（`Developer ID Application` 与 `Apple Development` / ad-hoc）手工替换同一 `CFBundleIdentifier` 时，macOS `tccd` 仍保留旧 `csreq`（或早期 ad-hoc `cdhash`），导致系统设置界面显示「辅助功能」已开启而 `AXIsProcessTrusted()` 返回 `false`；三是 `MacOsSystemDriver::open_accessibility_settings()` 仅打开系统设置 URL，未通过 `AXIsProcessTrustedWithOptions` 主动登记当前包签名。
- 修复：新增统一安装入口 `scripts/install-app`（默认要求 `Developer ID Application:` 与 `spctl` 通过、须显式 `--allow-dev` 才安装开发包，并在新旧 `codesign -d -r -` Designated Requirement 不一致时自动执行 `tccutil reset Accessibility craft.semantic.micro-input.dev`）；`scripts/release-macos` 新增 `--install` 串联该入口；移除 `scripts/verify-hardware` 对 `/Applications/Micro Input.app` 的覆盖，始终直接启动 `target/MicroInput.app`；`MacOsSystemDriver::open_accessibility_settings()` 在打开设置前调用 `AXIsProcessTrustedWithOptions` 提示登记；`scripts/check-boundaries.py` 新增 `R-C13` 静态门禁。
- 验证点：`scripts/tests/test_install_app_launcher.py`、`scripts/tests/test_verify_hardware_launcher.py`、`scripts/tests/test_release_macos_launcher.py`、`crates/mi-macos/src/executor.rs`（`accessibility_prompt_options_builds_single_entry_cf_dictionary`）与 `MI_AUTO_HOST_READY=0 ./scripts/verify-offline`。Seen 2x（2026-11-06、2026-11-15）。

## 时序门禁使用独立测试进程

- 起因与后果：方案切换的 2 毫秒检查与合成硬件用例共用进程内的隔离运行时，整组执行时出现超限；未改动主线的同组对照也失败，单独执行则通过。
- 修复：将现有测试原样移到 `crates/mi-core/tests/profile_switch_latency.rs`，由 Cargo 标准测试进程隔离；保留 35 次切换、两个 2 毫秒断言、零错误与零 banner 门禁，不为测试改生产运行时。
- 验证点：核对移动前后的测试函数相同，分别运行 `profile_switch_latency` 和 `workbench_store`，再执行全量 `scripts/verify-offline`。独立进程不消除机器整体负载的影响，不据此宣称产品变快。Seen 1x（2026-11-14）。

## 按键速查须复用执行器的动作来源

- 起因与后果：静态速查直接读取当前方案的首层，遗漏原生 Codex 已有的本机动作来源路由，安装版菜单因此把全部控件显示为未绑定。
- 修复：速查与运行期共用纯动作来源解析，仍保留生效方案的名称和身份。未知受保护方案与缺少本机来源继续关闭，不回退其他方案。
- 验证点：`native_codex_cheat_sheets_show_local_labels_and_preserve_empty_controls` 核对二十个控件、自定义标签、空槽、宽键和浏览隔离；`status_menu_cheat_sheet_retains_host_profile_during_device_editing_and_clears_when_unselected` 核对设备编辑隔离。安装后还须打开当前生效方案的菜单读回用途。Seen 1x（2026-11-14）。

## 原生 Codex 的保护范围不能吞掉下排宿主动作

- 起因与后果：锁定原生 Codex 后，界面仍显示本机下排动作，但运行期把受保护方案编成二十个空槽，旧输入入口也提前拒绝；七次下排触发没有动作回执。另有双窗口捕获忽略焦点，以及 `open -b` 把快捷键目标切到另一已有窗口。
- 修复：所选方案身份与动作来源分开，按独立接管目标将原生 Codex 的全部二十槽交给现有本机方案，并让会话目录使用同一来源；保留自定义动作、空槽和设备原始文件。Codex 使用当前焦点窗口，激活后核对同一窗口再发键，不通过重开应用改变目标。
- 验证点：`native_codex_all_controls_keep_existing_actions_in_locked_and_manual_routes`、`native_codex_preserves_custom_actions_unbound_slots_and_other_owners` 和 `native_codex_all_controls_dispatch_compiled_actions_and_pause_without_events` 覆盖路由。交付包还须在原生 Codex 多窗口中读回聚焦与新标签页效果，不能只看 `executed`。Seen 1x（2026-11-14）。

## 六层恢复与会话轮换必须保留 USB 独占归属

- 起因与后果：普通连接虽已独占，旧六层记录仍让发现器只枚举、恢复使用共享接口；路由选择曾排除 Codex，导致自定义配置无法控制第一层。
- 修复：底层 Vendor owner 与逻辑 RPC 会话分离，发现、核验与轮换复用同一 owner；旧 reader 在锁内退役，交接前排空已到达报告。固定与板载路由接收可编辑 Codex 配置；重连不自动升级设备映射。
- 验证点：Codex 手动与板载路由、旧六层独占发现、逻辑会话关闭与 owner 释放分离分别回归；真实 USB 竞争者、插拔与映射读回另留证据。排空已有报告不证明任意迟到的同 ID 响应可隔离，UNKNOWN-17 继续保留。Seen 1x（2026-11-13）。

## Creator Micro 2 逐键状态灯须绑定 KV_OAI_AG 键位并基于真实设备 ACK 记账

- 起因与后果：固件 `0.6.3` 逐键灯控逻辑仅对当前层键码为 `KV_OAI_AG00..KV_OAI_AG19` 的物理轴体应用 `v.oai.thstatus` 逐键颜色，标准 HID 键码直接回退渲染层背光且省去 `"lights"` 时默认整盘常亮 `#FFFFFF` 纯白；加之六层核验后关闭 `DeviceSession`、`store.rs` 跳过发送却硬编码返回 `"synced": true`、草稿选中层保护混入实时灯效，导致菜单栏显示会话槽位颜色而实体键盘全白。
- 修复：六层方案将第 2/3 层顶部 6 槽编为 `KV_OAI_AG06..AG11` 与 `KV_OAI_AG12..AG17`（避开第 1 层 Codex 独占的 `AG00..AG05`）并为托管层写入 `"lights"` 关灯底色；`SharedVendorBridge` 共享侧通道同时承载 `v.oai.hid` 6 微动跨层去重收键与 `v.oai.thstatus` 真实 ACK 确认（`WireSyncState::Confirmed`）、900 次请求无感预轮转；移除旧版布局兼容分支，在核实设备快照仍与 `planned-keymap.json` 一致且保留原始出厂备份的前提下直接原位升级第 2..6 层。
- 验证点：`mi-proto` 校验 `AG00..AG19` 20 键边界并拒绝 `ENC_*`；`mi-model` / `mi-session` / `mi-core` 覆盖过期六层快照启动自动原位升级、外部手工改键拒绝覆盖、保留第 1 层原始字节与出厂恢复链；`mi-bridge` / `mi-core` 覆盖逐键下发、真实 ACK 同步确认与断联关灯防重放。Seen 2x（2026-11-12）。

## 六层运行期路由须与方案锁定合一且稳态监控禁用子进程轮询

- 起因与后果：一是 `RunningSixLayer` 早期仅读取物理层，忽略菜单栏手动选择与方案锁定，当外部客户端改层或切到浏览器时按键失效；二是单次 Electron/AX 动作读回 `unconfirmed` 即吊销整个六层接收器，系统睡眠唤醒与换口重连也不自动恢复只读监听；三是 `start_monitor` 每 500 毫秒拉起 4 个子进程并重读完整 Karabiner 日志，日志超 1 MiB 或轮转即自毁。
- 修复：引入 `HostPreferences.locked_profile_id` 与 `SixLayerRoutingMode`，锁定或手动固定时将托管层按键及第 1 层 OAI 厂商通知统一路由到目标方案实时 20 槽动作表；仅在按住态（`apple_fn` 或自有听写）清理未确认时熔断接收器；唤醒与设备重枚举自动执行最多 3 次只读重连；稳态监控改为 2 秒零子进程轻量检查并兼容大日志尾部读取。
- 验证点：`locked_mode_routes_layers_two_through_four_and_ignores_foreground_switches`、`manual_pinned_mode_routes_selected_profile_when_other_app_is_frontmost`、`one_shot_unconfirmed_receipt_keeps_six_layer_alive_while_held_fn_fences`、`check_log_readiness_handles_logs_larger_than_one_megabyte_and_log_rotation` 与 `scripts/verify-offline`。Seen 2x（2026-11-12）。

## Antigravity 会话状态分类与侧栏直达须对齐前端列表规则与 AXLink

- 起因与后果：UI 改版与 `3b72418` 引入两处退化。其一，`store.rs` 按主观猜测匹配不存在的 `WAITING_FOR_*` 枚举并加了 15 分钟蓝点过期与 `NO_TOOL_CALL` 门控，导致超过 15 分钟的未读完成会话丢蓝点、`BUSY` / `hasRunningSubagents` / `notFullyIdle` 漏判进行中；其二，`native_ax.rs` 读取 `AXLink` 时漏读 `AXURL`，退而点击 `Search Conversations` 向模态搜索框输入 UUID，造成切换时弹窗闪烁。
- 修复：对照安装版客户端前端会话分类规则与实机侧栏 AX 核验，`classify_antigravity_trajectory_summaries` 对齐真实状态谓词并结合最前窗口 `/c/<cid>` 抑制已打开蓝点；`ObservationPurpose::Sessions` 补读 `AXLink.AXURL`、`AXButton.AXExpanded` 与 `AXStaticText.AXValue`，直接按侧栏行 `Stop execution` / `Archive conversation` / 时间戳识别三态并对目标 `/c/<uuid>` 执行单次 `AXPress`，删除全部模态搜索弹窗代码；主舞台恢复 `灯光筛选` 浮层与 `已读熄灯`。
- 验证点：`mi-core::session_navigation` 覆盖 2 小时未读蓝点保留、`markedAsUnread`、最前窗口抑制与 `BUSY` / `waitingSteps` 分类；`mi-macos::native_ax` 覆盖同名会话 `AXURL` 区分、折叠侧栏/项目/`See all` 原地展开及跨工作区同名目录消歧。Seen 1x（2026-11-11）。

## Antigravity 实时语音须走应用内 Start/End Voice，不得映射成 Ctrl+M 听写

- 起因与后果：Antigravity `live-voice` 曾被误映射为 Ctrl+M（keycode 0x2E）系统听写快捷键，且第 6 槽「Gemini Live 实时语音」键帽误用 Term 而非 Party（5 格波形）。
- 修复：改为对 Start/End Voice mode 按钮直接执行 `AXPress`，并恢复 Party 波形键帽；系统听写快捷键只留给明确的 `dictation` 命令。
- 验证点：`antigravity_live_voice_does_not_map_to_dictation_shortcut_ctrl_m` 断言 `live-voice` 的 native shortcut 为 `None`，执行时绝不 `PostKeyShortcut` Ctrl+M；`inspector_and_editors` 断言该槽推断 `CodexKeycapId::Party`。Seen 1x（2026-11-09）。

## Dock 图标轮廓与小尺寸辨识

- 起因与后果：早期图标占画布 90% 且直接挂载原始 ICNS 导致 Dock 显示偏大，角色插画在小尺寸下也不够清晰。
- 修复：以 Creator Micro 2 正面硬件轮廓、旋钮、摇杆、六颗发光键及底部宽键作为识别特征，采用 1024 画布中的 824 像素轮廓（四边留白 100 像素），启动时使用 `NSWorkspace.icon(forFile:)` 返回的系统图标。
- 验证点：`swift scripts/verify-app-icon.swift target/AppIcon.iconset` 自动对照系统应用图标的四档轮廓并在 `scripts/build` 中强制执行，同时检查 `16x16`、`16x16@2x` 与 `128x128` 实出尺寸。Seen 2x（2026-11-10、2026-11-14）。

## 固定应用层必须覆盖实际动作类型与状态后重发现控件

- 起因与后果：六层托管初版只给 `AgentCommand` 加了目标窗口约束，导致 `Shortcut` 与 `PasteText` 仍可能经过全局输入路径；同时缓存听写控件会漏掉开始录音后新重建的停止按钮。
- 修复：托管应用的快捷键和文本动作统一解析目标身份，分别使用目标 PID 或同窗 AX 输入，不以抢到前台焦点代替定向发送；状态切换动作在同一进程、窗口中重新发现控件，应用重启后只有确认旧 PID 已退出才解除旧录音约束。
- 验证点：`crates/mi-macos/tests/managed_controls.rs` 的 `shortcut_and_paste_action_types_never_use_global_input_for_managed_targets`、`toggle_observes_mouse_changes_and_replacement_controls` 与 `restarted_app_recovers_only_after_the_previous_process_is_confirmed_exited`。Seen 2x（2026-11-10）。

## SwiftUI 平面按钮矩形热区、Inspector 搜索防抢占与 Dock 恢复生命周期门禁

- 起因与后果：一是 macOS SwiftUI 中 `.buttonStyle(.plain)` 与 `.onTapGesture` 若未显式附加 `.contentShape(Rectangle())`，控件内的 `Spacer()`、内边距与圆角四角会成为点击盲区；二是受保护 `KV_OAI_*` 方案在 `CanvasSliceModel` / `InspectorSliceModel` 中若缺少 `selectedProfileId` 与 `selectedDisplayName` 会导致切换方案时画布不刷新，且首次点击摇杆与南/西方向标题曾出现错位；三是物理按键事件 `consume_physical_input_event` 无条件覆写 `selected_surface` 会在 Inspector 搜索或录制快捷键时抢占编辑焦点；四是 `MicroInputApp.swift` 缺少 `applicationDockMenu` 与 `win.deminiaturize(nil)` 导致最小化后点击 Dock 无法自动还原。
- 修复：为 `MicroInputUI` 全部 `.buttonStyle(.plain)` 与 `.onTapGesture` 补齐 `.contentShape(Rectangle())`；在 `CanvasSliceModel` 与 `InspectorSliceModel` 纳入 `selectedProfileId` / `selectedDisplayName`，为 `InlineInspectorView` 补齐 `.id(...)`、`ScrollViewReader` 滚动置顶与搜索框 `.onSubmit` 回车直绑；在 `consume_physical_input_event` 增加 `!inspector_search_active` 守卫；在 `MicroInputApp.swift` 补齐 `applicationDockMenu`、`win.deminiaturize(nil)` 与 `NSApp.applicationIconImage`；在 `scripts/check-boundaries.py` 固化 `R-C07` 静态门禁。
- 验证点：`MicroInputTests.testSidebarProfileAndSurfaceSwitchDistinctnessAndSearchFocusIsolation`、`scripts/check-boundaries.py`（`R-C07`）及 `./scripts/run --verify` 真实窗口边缘坐标点击验证。Seen 1x（2026-11-10）。

## 生命周期迁移、浏览隔离与固定键帽行高

- 起因与后果：睡眠唤醒若缺少统一退役入口会遗留旧 USB 会话、按住听写与六层后台任务；复用激活命令做界面浏览会意外切换运行期控制目标，QA 包若使用生产 `CoreStore` 也会误连真实 USB；此外将完整会话长标题直接塞入固定高度键帽会导致多行溢出变形。
- 修复：Swift 仅转发睡眠与唤醒通知，Rust 先关闭输入准入再等待旧 reader 与后台任务结束，唤醒采用零打开描述符发现器；新增只改编辑对象的 `BrowseProfile`，隔离 QA 强制要求 `MI_DOCUMENT_DIR` 并使用 `isolated` 配置；键帽仅显示编号与短状态，完整会话标题交由自然高度列表呈现。菜单栏弹层四个常用方案采用固定 2×2 非滚动布局，避免 `ScrollView` 首次测量裁切。
- 验证点：`WorkspaceLifecycleTests`、`workspace_lifecycle` 模块测试、`browsing_profiles_changes_editor_without_activating_host` 与 `SessionKeycapLayoutTests`。Seen 1x（2026-11-10）。

## 过期按键、分屏焦点、配置会话关闭与恢复兼容

- 起因与后果：排队按键超过 2 秒有效期若直接停用整个接收器会导致后续新按键失灵；同一窗口双分屏展平扫描会触发 `ambiguous_or_missing_dictation_control`；六层运行期若每 500 毫秒轮询 Vendor RPC 会耗尽 999 请求上限并因单次超时停用输入；退役历史第 4 层预设后，若恢复入口复用新版方案校验会拒绝合法旧备份。
- 修复：`Activation::is_expired_at` 仅淘汰过期事件而保留新按键准入；AX 捕获保留祖先路径并以唯一焦点分支锁定分屏；首次完整核验配置后立即关闭配置 RPC reader，运行期仅检查 USB、Karabiner、输入代次与睡眠边界；恢复入口单独严格识别已退役六层备份格式并校验设备身份与原字节，不重新向编译或执行开放旧格式。
- 验证点：`expired_clicks_do_not_disable_new_clicks`、`split_panes_resolve_only_the_focused_branch`、`executor_runs_after_verification_rpc_closes_and_stops_on_input_loss` 与 `six_layer_prepare_apply_readonly_verify_and_restore_roundtrip`。Seen 1x（2026-11-10）。

## 已选控件定向路径校验、会话 URL 确认与 AX 观察边界

- 起因与后果：聚焦输入框在捕获、选择、执行前校验和焦点读回四步重复全树扫描耗时过长，且 Antigravity 的 `AXScrollArea` 不满足 `AXParent` 与 `AXChildren` 互逆（深度 28 反查失败）；旧会话导航仅凭打开应用或目录存在就标记已读；此外不能把 AX 观察超时/截断误判为目标唯一，也不能把动作送达后的回读失败标为未执行的 `blocked`。
- 修复：保留完整发现与歧义判断，对已捕获控件沿向下 `AXChildren` 路径复验子节点、原 scope、PID、窗口与可见性（详见 [DECISIONS.md](docs/DECISIONS.md)）；会话直达在原窗口 `AXDocument` `/c/<id>` 与目录身份复核后才提交已读水位与历史（仅解码 `file://` URI，保留普通路径百分号）；区分观察期截断拒绝与执行后 `unconfirmed` 状态。
- 验证点：`exact_control_membership_rejects_removed_or_replaced_path_edges`、`exact_control_membership_requires_the_original_scope`、`input_added_after_capture_is_rejected_before_focus`、`session_navigation` 与 `native_ax` 部分树回归。Seen 1x（2026-11-10）。

## HID 全局管理器单线程归属、整包持锁写入与入站归属前置

- 起因与后果：macOS `hidapi` 的进程级 `IOHIDManager` 绑定首次初始化线程的 `CFRunLoop`，在短生命周期任务线程初始化 `HidApi` 会在线程退出后触发 `CFRunLoopAddSource` 崩溃；后台 reader 持有 `HidDevice` 互斥锁执行 `20ms` `read_timeout` 时，若长请求逐报告重拿锁会插入间隔引发固件 `400` 错误；入站流若在匹配 pending ID 前强校验所有包会导致外来无关联响应杀死会话。
- 修复：由随进程存活的 `HidOpenOwner` 单线程串行完成独占选项设置、打开与恢复；`MacHidLink::write_batch` 对整条多报告 RPC 一次持锁连续发完；入站循环先按持有 ID 过滤无法关联响应与通知，再严格校验当前请求回应；`fs.writebin` 未完成分块允许暂不出现在目录，`completed: true` 后再核验完整文件，且取消测试须先观察到越过发送边界再丢弃 future；灯光预览恢复同时比对目标与最后确认运行态，水位取自唯一 reader 入站序号，首行物理按键顺序按实机 `AG00`（左）/`AG01`（右）固定并同步覆盖写后重验。
- 验证点：`hid_open_owner_survives_short_lived_callers_and_failed_opens`、`multiframe_request_uses_one_transport_batch`、`foreign_response_echo_does_not_kill_reader_or_satisfy_current_request`、`cancelling_after_send_closes_session_and_never_replays`、`same_target_external_change_is_never_overwritten` 与 `verified_input_maps_observed_row0_order_joystick_sectors_and_merges_wide_key`。Seen 2x（2026-11-03、2026-11-10）。

## SwiftUI 细粒度域切片观察与 FFI 代次短路门禁

- 起因与后果：`CoreStore` 早期使用 `ObservableObject` 加单一 `@Published var model: ProjectionModel` 向下传递整棵模型，导致在 Inspector 搜索框每输入一个字符就触发全部顶层与隐藏弹层视图重算；同时 `mi_core_dispatch_json` 与 `poll_hardware_events` 未按投影代次短路，无变化事件仍反复跨 FFI 序列化完整 JSON。
- 修复：将 `CoreStore` 迁移为 `@Observable`，拆分为细粒度 `Equatable` 域切片（`header`、`sidebar`、`canvas`、`sessionStrip`、`inspector`、`statusMenu`、`overlay`、`footer`）及 `activeModal`，仅当切片值变化时写入；Rust `WorkbenchStore` 维护 `projection_generation` 门禁，代次未变时 `mi_core_dispatch_json` 与 `mi_core_poll_json` 直接返回 `NULL`（`R-C06`）。
- 验证点：`MicroInputTests.testRenderInvalidationAndLatencyProfile`、`crates/mi-ffi/src/lib.rs`（`ffi_roundtrip_projects_and_dispatches_commands`）与 `scripts/check-boundaries.py`（`R-C06`）。Seen 1x（2026-11-09）。

## 生产环境启动即插即用连接、全自动硬件验收与宿主单实例锁

- 起因与后果：一是生产配置需明确区分日常自动连接与隔离测试环境，且多候选或无关系统蓝牙串口（`verifiedModel == false`）存在时不能误跳过或误连；二是 `scripts/run` 若忽略未知参数或继承宿主 IDE 的 `__CFBundleIdentifier` 直接拉起临时包，会绕过 LaunchServices 单实例合并，与 `/Applications/Micro Input.app` 双开产生双菜单栏图标；三是启动脚本仅做语法检查会漏掉进程与文件权限边界错误，并行测试仅靠系统时钟命名临时目录也会偶发冲突。
- 修复：`WorkbenchStoreConfig` 显式引入 `auto_connect_usb`（生产为 `true`，隔离 QA 与单测为 `false`），自动连接仅按 `verifiedModel` USB 候选计数，并在连接后后台执行身份、配置快照与灯光回执自检；`MicroInputApp.swift` 在 `setupStatusItem()` 前调用 `mi_core_acquire_instance_lock` 获取 `<documentDir>/instance.lock` 排他锁，`scripts/run` 使用 `argparse.ArgumentParser(allow_abbrev=False)` 支持 `--list`/`--kill` 并剥离继承的 `__CFBundleIdentifier`（`R-C05`）；并行测试临时目录附加进程内原子序号，启动器边界由 `scripts/tests/` 独立单测覆盖。
- 验证点：`usb_connect_automatically_verifies_identity_snapshot_lighting_and_writes_acceptance_receipt`、`crates/mi-macos/tests/macos_scenarios.rs`（`document_instance_lock_rejects_duplicate_owner_until_released`）、`scripts/tests/test_run_verify_launcher.py`、`scripts/tests/test_verify_hardware_launcher.py` 与 `scripts/check-boundaries.py`（`R-C05`）。Seen 2x（2026-11-06、2026-11-09）。
