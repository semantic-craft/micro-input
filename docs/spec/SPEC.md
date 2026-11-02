# Micro Input — 完整产品规格 v1.2

2026-11-02，Asia/Shanghai。产品基线来自用户明确确认的 Q1–Q9 与阶段0–6；它定义目标产品，不宣称已交付。现行范围覆盖 Creator Micro 管理与 coding 场景，替代早期仅围绕兼容面的 v0.1。稳定规则 ID 保留，新产品规则使用 P/E/K/X 前缀。

PR #12/#13 已交付离线工作台与核心草稿（见 BUILD_PROMPT S1a/S2a）；当前按 35 张工单在统一集成分支实施，**不接任务自动化；未知协议不发送，设备写入仍须满足协议与恢复门槛**。后续阶段须单独明确实施范围；未来能力列入规格不等于现在授权执行。术语见 [GLOSSARY](../../GLOSSARY.md)，证据与范围见 [EVIDENCE](EVIDENCE.md)，构建及验收见 [BUILD_PROMPT](BUILD_PROMPT.md)，未知项见 [UNKNOWNS](UNKNOWNS.md)。

## Problem Statement

用户需要管理自己的 Creator Micro，并优先服务编辑器、终端和 coding agent 工作。用户认为 Input.app 臃肿，设置难用、不友好且稳定性体验不足；这是用户体验诉求，尚无可量化的性能对比结论。现有操作容易混淆浏览、编辑和设备生效，底层字段与复杂参数增加理解成本，使用不同 coding 工具时还需要自己拼接动作。

产品须让用户清楚知道当前设备会做什么、修改将影响什么、什么需要宿主常驻，以及出错后如何保留和恢复工作。技术栈不能代替轻量、可靠和好用的验收。

## Solution

提供原生 macOS Creator Micro 管理软件：菜单栏负责设备状态和常用切换，主视图以设备实体布局为中心，动作库围绕用户意图组织。Rust 核心 + SwiftUI，进程内 UniFFI；面向 Creator Micro 2 的 USB 只读链路先行。

完整闭环：接入设备 → 选择要浏览/编辑的 Profile 和 Layer → 点选实体控件 → 搜索、配置动作 → 在草稿中撤销和检查差异 → 明确应用 → 核对结果 → 日常手动切换。离线可编辑，重连先比较。通用 Coding 配置与工具专用配置并存，先明确切换，再提供可选应用感知。

Herdr、DeepSeek Harness（DSH）、ZCode、QwenWork 等是动作适配对象，不是产品中心。产品不演变成跨 agent 任务调度平台。Agent Experience 指代理能通过结构化接口协助准备、解释和检查配置草稿；设备应用仍由用户审阅触发。

## User Stories

以下故事均是目标要求，不是兼容能力声明。US 编号与下文规则、阶段验收共同追踪。

1. **US-01** As a Creator Micro user, I want a small native menu-bar app, so that managing my device does not dominate my desktop.
2. **US-02** As a device owner, I want to see connection state and verified identity, so that I know which device I am configuring.
3. **US-03** As a device owner, I want to select among ambiguous USB candidates, so that the app does not act on the wrong interface or device.
4. **US-04** As a user, I want permission and connection failures explained, so that I can recover without guessing or terminating other apps.
5. **US-05** As a user, I want a layout matching the physical controls, so that I can select the control I intend to change.
6. **US-06** As a user, I want to inspect the current binding and its details, so that unfamiliar settings become understandable.
7. **US-07** As a user, I want Profile and Layer browsing separated from activation, so that inspecting settings does not change device behavior.
8. **US-08** As a user, I want to distinguish the edited Profile from the active Profile, so that I understand what is currently in effect.
9. **US-09** As a user, I want to create, rename, duplicate and organize Profiles and Layers in a draft, so that I can adapt configurations without losing existing work.
10. **US-10** As a user, I want searchable actions expressed as intentions, so that I can configure controls without learning raw tokens first.
11. **US-11** As a user, I want action categories and favorites, so that frequent operations are easy to find.
12. **US-12** As a user, I want prerequisites and unavailable states shown beside an action, so that I know why it may not work.
13. **US-13** As a user, I want advanced token and raw-value details on demand, so that I can diagnose mappings without cluttering normal editing.
14. **US-14** As a user, I want to assign keys and shortcuts, so that common development operations need fewer keystrokes.
15. **US-15** As a user, I want to configure macros with explicit press, release and delay semantics, so that repeated input is predictable.
16. **US-16** As a user, I want to edit tap, double-tap, hold and tap-hold actions, so that each control can support several deliberate gestures.
17. **US-17** As a user, I want to configure encoder and joystick behavior, so that these controls are useful in my coding workflow.
18. **US-18** As a user, I want to configure supported lighting, so that the device gives useful visual feedback.
19. **US-19** As a user, I want live previews to be explicitly enabled, so that browsing a lighting setting does not silently write to hardware.
20. **US-20** As a user, I want to undo draft edits and review a change summary, so that I can correct mistakes before applying them.
21. **US-21** As a user, I want to prepare configurations without the device connected, so that I can work away from my desk.
22. **US-22** As a user, I want reconnect-time comparison with the device, so that an old local draft cannot silently overwrite newer settings.
23. **US-23** As a user, I want one explicit apply operation with a clear target and revision, so that I know precisely what will change.
24. **US-24** As a user, I want a preserved pre-apply configuration and a verified result, so that a successful send is not confused with a successful save.
25. **US-25** As a user, I want failed applications to preserve my draft and report uncertain device state, so that I can make an informed recovery decision.
26. **US-26** As a user, I want a validated restore operation, so that I can return to an earlier configuration when recovery is supported.
27. **US-27** As a user, I want import and export with compatibility and reference checks, so that configurations can be reused without losing unknown data.
28. **US-28** As a developer, I want a general Coding preset, so that common editor and terminal operations are immediately useful.
29. **US-29** As a developer, I want tool-specific configurations for Herdr, DSH, ZCode and QwenWork, so that the device fits my chosen environment.
30. **US-30** As a developer, I want semantic actions where reliable tool interfaces exist, so that I can configure an intent instead of memorizing shortcuts.
31. **US-31** As a developer, I want shortcut-based actions to disclose their focus assumptions, so that they are not mistaken for precise session control.
32. **US-32** As a user, I want to switch configurations from the menu bar or device, so that routine changes do not require opening the full editor.
33. **US-33** As a user, I want optional foreground-app switching with visible active state, so that automatic changes remain understandable and reversible.
34. **US-34** As a user, I want a radial menu and mapping reference overlay, so that I can discover available actions without losing my work context.
35. **US-35** As a user, I want text, application and URL actions with clear execution conditions, so that device controls can perform useful host operations.
36. **US-36** As a user, I want command execution disabled by default, so that importing or browsing a configuration never runs a shell command.
37. **US-37** As a user, I want device-resident and host-dependent actions distinguished, so that closing the host has predictable consequences.
38. **US-38** As a user, I want disconnection, reconnection and lock/unlock handled correctly, so that stale operations cannot affect a later session.
39. **US-39** As a user, I want to keep the menu-bar host running when I close the editor and release it when I quit, so that lifetime behavior is clear.
40. **US-40** As a user, I want optional login startup and restrained notifications, so that the app follows my preferences without stealing focus.
41. **US-41** As a user, I want keyboard navigation, accessible labels and consistent light/dark appearance, so that the configuration experience remains usable and polished.
42. **US-42** As a user, I want private configuration content excluded from diagnostics by default, so that troubleshooting does not expose my macros or commands.
43. **US-43** As a user, I want an agent to explain and prepare configuration drafts, so that I can get assistance while retaining the final apply decision.
44. **US-44** As an integrating agent, I want versioned structured reads, draft changes, validation and diffs, so that my results are inspectable and do not depend on clicking UI coordinates.
45. **US-45** As a maintainer, I want simulated devices and external-behavior tests, so that changes can be validated without modifying a user's hardware.
46. **US-46** As a maintainer, I want isolated device, model, host and tool-adapter boundaries, so that adding one coding tool does not require rewriting unrelated parts.
47. **US-47** As a user, I want measured startup, idle resource use and interaction responsiveness, so that claims of efficiency reflect actual use.
48. **US-48** As a user, I want unsupported firmware, unknown fields and unverified capabilities labeled honestly, so that the app does not invent compatibility.

## Implementation Decisions

### 产品、交互与权限边界（DECIDED）

| 规则 | 决定 | 证据 |
| --- | --- | --- |
| R-P01 | 产品中心是Creator Micro管理，coding为优先场景；不建设agent任务面板、跨工具调度器或新聊天客户端。 | E-049 |
| R-P02 | 菜单栏+主视图；主视图以实体布局→选中控件→分配动作组织；原生样式、键盘焦点、深浅色和无障碍标签一致。菜单栏日常操作不要求主窗打开。 | E-049 |
| R-P03 | 选定工作台：左侧配置浏览，中间设备逻辑布局，右侧所选控件编辑面板；动作名称可见，搜索/分类后更换动作，差异显示配置/层/控件及前后值。修改才突出变更入口，支持撤销/重做；菜单栏与主窗共享浏览状态。紧凑窗口下布局区可滚动，系统字体与语义色支持跟随系统/浅色/深色。菜单栏面板共享状态，关闭主窗保留进程，明确退出结束进程。键盘入口：⌘[/⌘]选控件、⌘E动作库、Tab进入搜索、回车分配首个匹配结果、⌘D差异、Esc取消、⌘Z/⇧⌘Z撤销/重做、⇧⌘M菜单栏面板。S2a草稿按版本化本地文档保存；参数编辑点击保存后持久化，退出重开恢复；未保存的参数输入不冒充已保存草稿。 | E-067 |
| R-E01 | 浏览、草稿、设备快照、设备活动配置四种状态分开。导航零写入；宿主设置与设备设置显示各自生效方式。 | E-043, E-050 |
| R-E02 | 编辑进入本地草稿，提供撤销、重做、保存和变更预览；切换编辑对象保留草稿，显式丢弃才移除未应用修改。 | E-050 |
| R-E03 | 应用以明确设备、基线快照、草稿修订号为目标；先展示变化再由用户执行。基线变化则阻止覆盖并重新比较；连续点击不重复提交。 | E-050 |
| R-E04 | 离线编辑保留来源/固件/基线；重连按设备当前值与草稿比较，冲突不静默选边。不兼容或未知字段不能在编辑/导出/应用中悄悄丢弃。 | E-050, E-055 |
| R-E05 | 应用前保留设备配置副本，记录可确认的步骤；失败保留草稿并报告设备状态是否确定。只有经过实机验证的恢复方案才能标为可恢复，不承诺事务原子性或自动回滚。 | E-050, E-054 |
| R-E06 | 灯光等实时预览是单独的显式入口，标明设备副作用；不继承为所有编辑自动保存。本轮与只读阶段均禁用。 | E-050 |
| R-K01 | 统一可搜索动作库，分类与收藏；条目有稳定标识、可读名称、参数、目标、前置条件、宿主依赖、可用性和结果语义。高级信息按需展开。 | E-051 |
| R-K02 | 通用Coding与工具专用配置并存。适配Herdr/DSH/ZCode/QwenWork等前逐工具验证“启动/聚焦、具体会话定位、语义操作、结果确认”；未验证能力不可标已支持。快捷键回退标明焦点前提。 | E-051, E-065 |
| R-K03 | 设备触发的coding操作必须有明确目标；去重按下/释放与宽键事件；多宿主执行/灯光所有权未解决前不启用重叠消费者。切换会话不是自动分派任务。 | E-051, E-060 |
| R-K04 | 先手动菜单栏/设备切换配置，生效对象始终可见。自动随前台应用切换可选、可关闭；需确认设备活动配置语义及切换副作用后启用，草稿不因此自动应用。 | E-051, E-026 |
| R-X01 | AX指Agent Experience，另保留无障碍要求。代理只读结构化状态、准备/校验草稿和差异；与UI共用模型/校验，不暴露任意RPC或无审阅应用入口。 | E-049, E-051 |
| R-X02 | 性能预算先测量再制定；验收记录版本、环境、提交和来源。模拟/探针/应用/合并/发布分开报告。诊断默认脱敏，无遥测要求。 | E-049, E-063, E-064 |
| R-X03 | 最终产品范围不构成本轮执行许可：S1a/S2a 已交付，当前按依赖图实施 35 张工单；不接自动化，不写设备。每阶段独立任务和完成门槛。 | E-051 |

### 核心结构与存储

| 规则 | 决定 | 证据 |
| --- | --- | --- |
| R-A01 | Rust核心+SwiftUI原生macOS应用，非IME，不要求兼容Electron实现；macOS 14+为当前构建基线。 | E-003, E-049, E-063 |
| R-A02 | mi-model拥有配置/raw/草稿校验；mi-proto拥有framing与RPC；mi-transport拥有OS设备与模拟传输；mi-session拥有串行会话与快照；mi-ffi暴露异步高层能力。UniFFI进程内静态库；UI不直接发HID/RPC。 | E-049, E-063 |
| R-A03 | 菜单栏、主窗和浮层共享核心状态，各自只获得所需能力；主窗关闭与退出应用分开。 | E-005, E-051 |
| R-A04 | 后续mi-host/HostKit承接宿主动作、工具适配与系统权限；Overlays承接径向菜单/映射提示。需要时才增加模块/进程；当前不引入守护进程、HTTP服务或XPC。 | E-021, E-049 |
| R-A05 | 高层方法白名单；只读、草稿、未来设备应用分别建能力面。UI和agent均无通用RPC透传。 | E-022, E-051 |
| R-A06 | 丢弃edge-js/.NET和原厂资源依赖。未引用静态事实不等于证明原程序运行时绝不加载。 | E-040 |
| R-C01 | 设备快照以实际设备文件为准，keymap必需，smart文件可缺且区别于空文件；演示/导入/草稿不能冒充设备读回。读取结果完整且校验后才替换视图。 | E-030, E-054 |
| R-C02 | 未来写入按明确应用流程设计；原客户端smart后keymap及校验只作证据，不能证明事务或恢复。当前不引入生产写方法。 | E-031, E-050 |
| R-C03 | 保留原始token与未知字段；已知显示投影与设备值分开。类型错误可拒绝，未知token不显示为空绑定；原token转换非完整键码表。 | E-038, E-055 |
| R-C04 | 导入导出含引用完整性/冲突检查与格式版本；自有格式和厂商Profile/Layer兼容分别标识。厂商schema未验不能承诺无损互导。 | E-039, E-050 |
| R-C05 | Profile ID与index分开；设备活动索引未知时显示未知。模型包含Layer、13开关矩阵[2,4,4,3]、旋钮3槽候选、摇杆扇区、两区灯光、宏、Multi四分支/term、分组和linkedApps。实体按键图与开关映射单独验证；设备schema≠演示schema；数值范围、容量与完整事件编码待确认。 | E-044–E-046, E-055, E-060 |
| R-C06 | 自有持久化保存快照、草稿、差异、备份与设置；不读取或迁移原Input数据库。具体存储选型随阶段2确定，先保证原始值和修订一致性。 | E-028, E-047, E-050 |
| R-C07 | 命令执行默认关；通知与登录启动由用户控制。数据默认本地，配置内容不进入公开诊断。 | E-029, E-051 |
| R-C08 | 自编Coding预设与原厂Preset兼容分开；安装预设形成草稿，引用ID冲突明确处理；不能暗中激活或执行宏/命令。 | E-045, E-051 |

### 设备通信与独立实现边界

| 规则 | 决定 | 证据 |
| --- | --- | --- |
| R-D01 | Creator Micro 2候选VID303A、PID8297/8298；当前本机验证范围8298/USB。8297仅静态识别；8360及其他型号不因名称近似承诺兼容。 | E-010, E-052, E-057 |
| R-D02 | OS事件驱动发现，约175ms合并；检查vendor usage及完整usage pairs，不能将PrimaryUsage固定为唯一依据。USB用OS Transport证据；多候选明确选择，PID不作唯一物理ID。 | E-011, E-052, E-059 |
| R-D03 | 当前不用串口和legacy文本协议；旧型号/bootloader不进入Micro2 USB实现。 | E-011, E-017 |
| R-D04 | Framer可替换。已有node-hid静态布局与一次IOKit短查询，不再把全部字节定义写成无来源；但每种后端须明确report ID是否单独传入、长度和接收边界。先证据→离线向量→获准只读实机验证；未知时不发送。 | E-052, E-057, E-059, E-062, E-066 |
| R-D05 | RPC响应按持有ID匹配，通知单独派发，保留未知字段/错误。长短字段与完整JSON/换行边界须按来源验证。0–998仅当前客户端策略，不证明固件上限或防跨进程串台。 | E-013, E-053, E-059 |
| R-D06 | BLE是后续候选，传输切换/去重/配对槽另验；不因HID推断或社区报告扩入USB MVP。 | E-048, E-061 |
| R-D07 | 每设备串行请求，约50ms间隔；可取消和有界超时/重组内存。断连使pending open、队列、回包与快照失效，新会话隔离；非独占，不抢占/结束他人进程。超时值是策略，不是固件保证。 | E-012, E-035, E-056, E-063 |
| R-D08 | 只读生产候选仅device.status、sys.version、fs.list、fs.readbin；文件仅keymap/smart_actions。fs.read只是社区替代路径。list→readbin→字节校验→再list检测变化，不能声称跨文件原子性。参数与schema依据统一证据补齐，未验证部分保持占位。 | E-054, E-059 |
| R-D09 | 身份字段缺失显示未知，不以OS VersionNumber冒充固件；状态缺version时可查询版本是自研策略。host.focused_app留待宿主阶段；危险方法不进白名单。 | E-053, E-057 |
| R-D10 | 显示固件与兼容状态，保持稳定版策略；不刷固件、不复位、不启动bootloader/selftest。历史latest查询只对记录日期有效。 | E-017, E-058 |
| R-D11 | 不引入LVGL/屏幕/壁纸/媒体卡片功能。 | E-019, E-025 |

### 宿主动作与权限

| 规则 | 决定 | 证据 |
| --- | --- | --- |
| R-H01 | macOS原生宿主，沿用当前非沙盒开发方向；正式签名、分发方式和权限稳定性后续单独验收，不能用ad-hoc一次成功替代。 | E-001, E-002, E-063 |
| R-H02 | 枚举不请求Input Monitoring。真实open/read失败区分权限、占用、Secure Input及其他I/O原因；按实际能力引导设置并重检，不以预检猜测所有失败。 | E-008, E-037, E-059, E-063 |
| R-H03 | 没有实际CoreBluetooth路径时不申请蓝牙权限。 | E-048 |
| R-H04 | 可选前台应用感知采用事件驱动；需连接设备且支持该能力；只在变化时上报，断开即停。采集应用、自动切换、设备上报是不同动作；不照搬5秒采集作为全局规则。 | E-025–E-027, E-046 |
| R-H05 | kb.sa.inserttext由宿主执行，成功/失败与剪贴板恢复可验；不因恢复覆盖用户期间新复制的内容。实现与权限方式待验证。 | E-024, E-027 |
| R-H06 | kb.sa.openapp/openurl验证参数与目标；无效目标不执行，向用户提供可理解原因，不泄露私有参数。 | E-024, E-049 |
| R-H07 | CGEvent/AppleEvents等权限按具体动作申请和诊断，不在枚举或只读配置阶段请求辅助功能/自动化权限。 | E-027, E-037 |
| R-H08 | kb.radial浮层不抢焦点，显示器/Spaces行为明确；摇杆选择和确认语义、扇区上限待验。 | E-024, E-032 |
| R-H09 | kb.sa.exec默认关闭，显式许可且标明执行目标/上下文；导入、浏览、校验草稿不得执行命令。 | E-024, E-029 |
| R-H10 | kb.cs.show/hide/toggle映射提示浮层，不抢焦点；大小、定位、拖动及隐藏生命周期可验。 | E-024, E-032 |
| R-H11 | 菜单栏主视图开关、明确退出和可选登录启动；关闭主窗保留必要宿主，退出释放设备/监听。后续实现时明确通知和常驻策略。 | E-033, E-049 |
| R-H12 | 动作标明device-resident或host-required；已见kb.sa通知由宿主执行，宿主缺席设备行为未知。不能承诺所有宏都需宿主或所有动作都能离线执行。 | E-024, E-031 |
| R-U01 | 自有更新方案后续选择；无自动升级固件或替用户升级工具。 | E-034, E-049 |
| R-U02 | 诊断仅描述版本/能力/失败阶段及经批准的只读结果，默认不含宏、命令文本、序列号、原始回包；不照搬厂商诊断RPC清单。 | E-036, E-049 |
| R-N01 | 当前规格任务及只读MVP禁止设备写入、删除、事务、Profile激活、灯光预览、复位/刷机；枚举不发RPC。已验证的读请求可能用HID output发送，不等同配置写入。 | E-043, E-051 |
| R-N02 | 单设备单会话起步；多候选由用户选择；与Input/Codex并存不能靠非独占或ID范围宣称安全。 | E-012, E-059 |
| R-N03 | 未知字段、未知token、缺失引用、数据来源与过期状态可见，原始值保留；未知项不可写成已实现。 | E-044, E-055 |
| R-N04 | 复用MockTransport/高层API及自编合成向量；禁止将测试Framer/schema当真实协议。实现只用自撰行为规格，不复制厂商代码、资源或用户数据。 | E-041, E-062, E-063 |

## Testing Decisions

1. **最高共同边界是异步高层核心接口，经真实UniFFI驱动SwiftUI。** 在该边界验证来源、配置浏览/草稿/差异、错误和会话状态；避免分别为每个视图建立一套模拟业务逻辑。后续agent接口复用同一核心契约。
2. 只对边界不可观察的协议细节使用下层测试：Framer/Codec的有界分片与重组、Method白名单、读文件校验、会话取消/迟到响应。模拟设备提供可编程连接、通知和内存文件表，不接实际设备。
3. 已有候选先例是PR #8的解析、Swift桥接/导航零请求、断连与pending open取消、大整数保留、同批通知测试；选择性接续后必须在新提交重跑，不将旧结果移植为新提交通过。
4. 好测试关注外部结果和副作用：页面选择前后出口零请求，错误/未知不会变为空或成功，失败不污染已有快照，草稿校验不执行动作，应用/恢复只在获准阶段验收。不能仅断言内部调用顺序或复述实现。
5. 核心模拟通过后，使用实际交付构建验收UI、当前设备/固件/USB/backend、权限、生命周期；合成/探针不能代替应用路径。硬件未在场、权限或协议未解锁时记未完成。
6. 分阶段检查项与停止条件以 [BUILD_PROMPT](BUILD_PROMPT.md) 为唯一验收清单；本规格引用同一阶段编号，不维护第二套勾选状态。每项交付关联规则ID、证据ID、提交及环境。
7. 性能先记录启动、菜单和切换延迟、idle CPU/RSS、断连恢复、长时间使用与签名重启；具体预算为UNKNOWN-15，阶段5签收前制定并执行，不能单凭技术栈宣称改善。

## Out of Scope

- 当前任务实现已批准产品规格，不接自动任务触发/调度；任何设备写入/激活仍须满足协议、备份和恢复前提。阶段3及之后的设备操作须后续明确任务并满足证据与恢复前提。
- 产品不做跨agent任务编排/聊天平台、IME、内核驱动、厂商遥测、媒体/壁纸/LVGL，不刷固件、不做bootloader/selftest。
- 首轮不做BLE、多设备并行、Windows/Linux、旧VIA/QMK设备或其他Work Louder型号；未来扩大范围需新证据与规格决定。
- 不承诺全token/参数范围、全部工具API、厂商Preset互导、事务原子性或自动恢复；未知项继续占位。
- 不复制、调用或移植原厂Input/wl-device-kit实现与资源；不读取原Input数据库/宏载荷补规格。历史调查存在静态分析，本项目不声称已经取得严格人员隔离的clean-room证明。

## Further Notes

阶段0规格统一 → 阶段1真实只读 → 阶段2离线编辑 → 阶段3可验证应用/恢复 → 阶段4Coding日常使用 → 阶段5切换/体验完善 → 阶段6Agent协助扩展。离线编辑可以在只读补证等待期间独立推进，但不因此提前解锁设备写入。

发布到GitHub的规格工单是本版本快照与任务入口；仓库文档维护现行产品契约，后续规则变更须同步工单版本/引用。`ready-for-agent` 表示该票的接口、文件范围、检查点和证据门槛足以开工，不能理解为允许执行所有后续阶段；未细化票使用 `needs-info` 并记录缺项。原 #1–#6 仍只覆盖阶段1，全部保持原完成状态；旧PR #7/#8关闭未合并，主分支没有它们的应用实现。


## S2a — 独立本地文档与社区设计边界

| 规则 | 契约 | 证据 |
| --- | --- | --- |
| R-E07 | 绑定以文档内Profile ID、Layer ID、逻辑控件ID定位，禁止持久化数组下标或显示名称作为身份。缺失/重复ID拒绝可编辑文档。物理控件、键帽呈现、动作角色独立；当前逻辑位置不是已验证实体映射，不把任务键或灯光能力固定给某一实体位置。 | E-069, E-070 |
| R-K05 | 动作带结构化类型与参数：Shortcut、Text、OpenUrl、AgentCommand、FocusSession、SkillReference。SwiftUI编辑参数，Rust校验。工具引用仅为本地意图，不表示API、事件或执行能力已验证；未知类型/字段保留原文件并只读。 | E-069, E-070 |
| R-E08 | 自有JSON schema_version=1持久化基线来源/raw、绑定、动作参数和修订。保存先校验，临时文件sync后同目录rename；保存失败不提交内存修改。多实例修改拒绝覆盖。损坏/高版本文件原样保留并停止编辑；本版本没有旧持久格式，不编造迁移。未来迁移须先备份并通过迁移后校验再替换。撤销栈仅会话内，撤销后的内容同样持久化。 | E-070 |
| R-D12 | 设备状态分候选发现、句柄打开、只读通信验证、能力验证。SDK枚举/打开成功不等于兼容完成。权限、疑似占用、断连、超时、协议未验证分开诊断；无证据不能把I/O错误断言为权限失败。此轮只整理证据，连接状态机扩展仍由S1验收。 | E-057, E-069 |
| R-N05 | Cindy固定版本仅作为社区设计参考。不得加载它所依赖的原厂SDK，不移植源码/资源，不运行Agent keymap绑定、灯光、配置恢复等设备写路径；社区事件字段不解锁本项目UNKNOWN。 | E-069 |

本地文档默认保存在用户Application Support/MicroInput/workbench.json；MI_DOCUMENT_DIR仅覆盖开发/验收目录，不用于设备发现。首次修改创建文件，修改自动保存；参数表单必须点击“保存参数到草稿”。单文件上限4 MiB是本地资源策略，不是固件限制。坏文件和高版本文件不自动重置；.lock/.pending残留拒绝覆盖并显示检查提示。T06已提供v1→v2字节备份迁移与Profile/Layer本地管理；快照经显式操作另存独立草稿，重开恢复所选文档。T08 已提供自有本地普通宏步骤编辑和共享引用管理（E-085），厂商宏格式/容量未核实；导入导出、Multi 编辑、设备应用及恢复按钮仍未开放，不宣称S2父阶段完成。

## 已批准的实施与交互契约

2026-11-02 用户接受实施准备审查修订。以下是设计要求，非新增设备事实；详细接口和步骤见 [实施设计](../implementation-plan.md)。原有 48 条故事与 35 张工单保留，完整索引为 [#44](https://github.com/semantic-craft/micro-input/issues/44)。

| 规则 | 契约 | 来源 |
| --- | --- | --- |
| R-D13 | 发现、连接、身份查询、配置读取独立；连接不隐式读配置。每会话唯一 reader 分派响应/通知，旧 generation 无效；有界队列溢出停用输入并报告缺口，不丢释放后继续执行。未知 backend 不发送；四种读方法/两文件白名单在生产出口强制执行。详见 D01–D05。 | E-073 |
| R-E09 | 首次结构编辑由 #15 引入本地 v2，baseline/raw、可编辑图和源 ID 映射分离；v1 先备份、校验、原子替换，失败不改内存/原件。旧 logical-N 不是物理身份。撤销覆盖可编辑图；恢复票复用真实迁移，不为关票制造新格式。 | E-073 |
| R-E10 | 被引用对象拒绝删除并列明引用。重连按 D07 比较表处理：无冲突才可合并；异值、删除/修改、未知 raw 变化阻断自动合并。保留本地/采用设备均为明确的整份选择；取消不改状态。成功保存才更新 base/draft/revision，写前重新核对身份和摘要。 | E-073 |
| R-E11 | 灯光预览须先实读可恢复基线；取消仅在当前状态仍属于本次预览时恢复并读回。缺可靠读回/恢复协议禁用预览；断连/外部变化停止写入，报告恢复未完成，不自动覆盖。 | E-073 |
| R-K06 | 通用预设复用 #20 安装器，仅生成草稿；专用适配最低为发现目标、聚焦明确目标、向该目标放入文本而不提交，两个目标交叉验证。焦点快捷键不是精确路由；工具身份/接口缺证据继续阻塞，不以不支持签收。工具范围及原生验收见 D08。 | E-073 |
| R-K07 | 映射提示分别标注宿主启用 revision 与设备验证读回的活动配置；未知/过期/不一致显式显示。草稿和浏览选中不冒充当前生效状态。 | E-073 |
| R-K08 | 自动切换默认关闭；手动覆盖只由用户“恢复自动切换”解除，跨前台事件和重启保持。宿主切换不暗含设备激活；本轮不启用该能力。 | E-073 |
| R-X04 | 逻辑编辑 #19 不以前置全控件实测 #14 为门槛，但不得宣称物理映射已验。#14 保留全实测验收；事件不足时才依赖后续获准的 #23 最小绑定准备/恢复。#26 与最终 #44 不跳过 #14。Agent 草稿接口选一次性本地 CLI + JSON，复用文档服务，无网络/任务执行；schema 与客户端验收仍待 S6。 | E-073 |

完整范围不代表全部实施就绪。采用本设计不解除 UNKNOWN，不授权设备写入或宿主执行。原生、设备和跨版本验收仍必须在对应阶段实际完成。
