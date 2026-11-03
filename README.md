# Micro Input

面向 Work Louder Creator Micro 2 的独立 macOS 配置器与宿主服务。技术方向：**Rust 核心 + SwiftUI 界面**。项目名为工作名。

## 当前开发状态

首轮原生离线工作台：Rust五核心模块 + UniFFI + SwiftUI。支持配置浏览、动作搜索/分类、结构化动作参数、Profile/Layer新建/复制/改名/排序/删除、本地普通宏按下/释放/延时步骤编辑与共享引用管理、四分支 Multi 的本地时序与共享动作引用编辑、本地草稿持久化、结构与动作差异及整次撤销/重做。动作库按名称、分类和目标搜索，收藏作为独立本机偏好保存；动作条目显示目标、前提与执行不可用原因，应用意图通过系统选择器选择目标。主窗和菜单栏共享状态；关闭主窗保留菜单栏，退出结束会话。View菜单可选择深浅色和紧凑窗口；⇧⌘M打开菜单栏面板，⌘E搜索动作，⌘[/⌘]选择控件，⌘D查看差异。

```sh
scripts/verify-offline # 全量检查 + 三个隔离合成应用场景 + 自动回执/清理核对；不连接真实设备
scripts/check          # Rust/Swift测试（含真实UniFFI合成发现）、桥接生成、开发构建和协议边界检查
scripts/run            # 启动当前checkout的target/MicroInput.app
scripts/run --verify   # 独立QA标识/文档，10分钟自动退出并回收副本
```

需要macOS14+、Rust stable、Swift工具链。构建不加载凭据。仅本地ad-hoc开发签名，未正式发布。

启动载入所选本地草稿，无文件时载入合成配置；点击“查找 / 刷新 USB”后只读枚举 USB HID 描述符并持续监听插拔，不会打开硬件或申请权限。候选经完整 usage pairs 与 OS USB 来源筛选，多候选必须明确选择；0x8297 标为未验型号；候选只展示 OS 身份，显式查询前固件显示未知。插拔事件更新候选，选择不读取或应用设备配置。本地文档默认在用户Application Support/MicroInput/workbench.json；v1浏览不改文件，首次修改先备份原字节再保存v2。设备快照保持只读，点击“从快照新建草稿”才保存独立draft-UUID.json；原工作台保留，所选文档记录带锁/冲突检查，退出重开继续新草稿。参数表单需点击保存，撤销历史仅在当前会话。原生配置写入尚未开放，宿主动作能力见下节。支持显式USB身份查询，当前303A:8298/固件0.6.3已在应用路径读回；显式身份为受支持的0.6.3后可读取两份限定配置文件，按块校验原始SHA-1并再次list，再一次发布只读快照。查看设备不会改变本地草稿，返回后保留编辑与撤销。核验快照可与文档基线、本地草稿作三方比较；整份保留本地、采用设备或取消均需明确选择，不写设备。接受前先保存不可覆盖恢复副本，再原子保存并清空跨基线撤销；身份不确定、源 ID 歧义或未知原值变化禁用自动合并，临时接口变化须先核对并确认同一设备，确认仅对当前会话快照有效；已知身份不同仍不能套用基线。其他固件仍拒绝读取，不把演示或导入数据作为设备快照。

系统睡眠通知会立即使旧会话和待完成结果失效，取消请求并关闭输入订阅，保留本地草稿及会话内撤销历史；旧快照标为过期。唤醒等待旧会话关闭，只在用户此前已开启 USB 发现时刷新描述符；设备选择清空，连接、身份和配置仍须明确重新操作。观察者随应用退出注销。此路径已通过合成通知与真实 UniFFI 回归，真实睡眠/唤醒、锁屏/解锁、读取中物理拔插及原映射行为仍待验收，见[E-096](docs/spec/EVIDENCE.md)。

输入监视支持候选绑定观察（`subscribe_input`）与已核实宿主路由流（`subscribe_verified_input`）：在固件 0.6.3 硬件透传层（`AG00–AG18`）就绪时，`VerifiedInputDecoder` 按实机核验映射第 0 行按键（左键 `AG00` → `logical-0`，右键 `AG01` → `logical-1`）与摇杆 4 扇区偏移（`AG15–AG18` → `logical-16–logical-19`），并由 `WideKeyMerger` 将底部 2U 宽键双微动（`AG10` + `AG11`）按位掩码合并为单次触发。菜单栏与工作台支持启用 Mac 宿主动作执行、安装八套桌面 Agent 预设（Cindy、Codex、千问办公、ZCode、Antigravity、DSH、Qoder CN、Herdr App）及零 Flash 写入切换 `host_active_profile_id`，详见[宿主动作执行与 Coding Agent 预设架构](docs/designs/host-execution-and-agent-presets.md)与 [E-098](docs/spec/EVIDENCE.md)。

旋钮三个逻辑槽位可独立分配本地动作；摇杆可选择保留来源模式或编辑本地逻辑扇区。减少活动扇区或切换模式不会删除旧槽位及绑定，未知物理方向继续标识；0–32 是本机编辑器限制，不是设备容量。保存、撤销/重做和重开沿用本地文档服务。见[控件编辑契约](docs/implementation-plan.md#t10-本地控件编辑契约)。

“本地文档”提供恢复与另存：列出本机草稿、迁移/重连副本和中断文件，也可明确选择来源；先显示版本、来源、修订和 SHA-256，确认后创建独立 v2 文档并切换。损坏、高版本和未知字段仅可另存原始字节副本，不转换或重置；当前草稿遇到原件并发变化或中断保存时，可另存独立文档继续编辑。来源、原工作台和中断记录保留，选择记录使用既有锁与预期字节检查；选择记录冲突时拒绝切换。v1 恢复复用既有转换，不新增格式。本地恢复不代表设备恢复。见[E-090](docs/spec/EVIDENCE.md)。

“导入与导出”支持整文档、当前配置或当前层的自有 JSON 预设，以及原创 Coding 起步预设。导入先预览，再创建独立草稿；原文档保留，本地对象 ID 重映射，历史设备基线不作为当前设备事实。为保留未知原值，导出仍包含完整来源档案，保存前会提示；不宣称兼容厂商格式。见[预设交换契约](docs/implementation-plan.md#t11-自有预设交换契约)。

Agent 可通过 `target/debug/mi draft read/validate/patch` 读取已保存的本地文档、校验受限操作并生成独立提案。工作台“审阅 Agent 提案…”显示来源修订、摘要和逐项差异，拒绝保持当前草稿，接受才创建独立草稿。过期身份/修订/原字节、非法字段、悬空引用与保存冲突均拒绝。示例：`scripts/draft-client.py /absolute/workbench.json /absolute/proposal.json '新配置名称'`；CLI 只支持配置/层改名及已有本地动作绑定，不创建新动作、不控制设备或执行宿主动作。协议、错误码与来源保全见[T29 契约](docs/implementation-plan.md#t29-agent-本地提案契约)。

“准备基础绑定应用”提供单个普通设备按键的本地预览：将已知的六项 DeviceKey 意图与宿主快捷键分开，只接受来源明确且无其他结构变化的草稿。预览绑定当前会话核验快照、文档身份/修订/原字节摘要，显示来源 Profile/Layer/逻辑槽和前后值；活动索引不用于猜测目标。明确保存后，在私有目录保存两文件原字节/有无、元数据、计划身份与单 token 编译候选，fsync 并读回核对；不覆盖旧包。原生设备应用与恢复按钮保持禁用，副本不代表设备恢复已验收。见[T13 准备契约](docs/implementation-plan.md#t13-单基础绑定准备契约软件准备设备应用仍未开放)与 E-097。

规格见 [SPEC](docs/spec/SPEC.md)、分阶段验收见 [BUILD_PROMPT](docs/spec/BUILD_PROMPT.md)、[证据](docs/spec/EVIDENCE.md)和[未知项](docs/spec/UNKNOWNS.md)。

## S3 离线实现

Rust 模型已支持 Layer 可选灯光目标、单区/当前Profile多层编辑、撤销/重开/复制及自有预设往返。准备器支持单个普通键（包含有限已知AG/ACT原绑定）或仅灯光字段的候选；未知字段、另一灯区和其他映射保持原字节。保存副本后冻结计划仍可取得一次，原生实例的应用/恢复按钮继续禁用，专用合成实例可执行完整流程。

`cargo test -p mi-model --test lighting` 验证文档与候选；`cargo test -p mi-session --features synthetic-backend --test config_write` 用实际codec/framer/唯一reader和可变合成文件验证写回、恢复及失败路径。合成写入器仅在synthetic-backend构建中存在，并拒绝原生后端；高层 FFI 和恢复界面仅对专用合成实例开放。生产读接口仍只读。

下一段离线恢复已实现：备份目录内持久操作记录绑定原副本、候选和冻结文档；首次可能发送前落盘，重开不续写。合成环境支持损坏 keymap 的显式新计划恢复、独立读回核对，以及只修本地保存的幂等重试。

高层 FFI、宿主动作排空、逐绑定重验及恢复界面已接入专用合成后端（E-102）。这不是完整 S3 交付：两区 RGB 编辑与合成预览生命周期已接入；真实设备写入、原生预览及实体效果验收仍未开放。当前验证不连接真实设备。方案见[设备改键与RGB设计](docs/designs/device-write-and-rgb.md)。

RGB 面板提供 Backlight / Underglow 的六种已知效果、六位颜色值及可用的亮度/速度/Magic 参数，禁用或未知字段保留原值；当前配置全层修改为一次可撤销操作。预览是显式临时状态，既不修改草稿也不写 keymap，30 秒自动结束，关闭面板/退出时显式结束。合成状态机使用专用 `synthetic.lights.*` 测试消息证明顺序、目标绑定、独立运行态读回与失败行为，**不是官方 `lights.preview` 的已验证实现**。原生预览入口保持关闭。

`scripts/verify-offline` 自动执行统一门禁、构建合成应用、依次跑宿主/配置/灯光三个隔离场景、核对回执与清理并生成每轮 `result.json`，最后恢复生产构建而不启动它；任何缺失回执或错误地宣称物理/视觉通过都会失败。`--qa-only` 只省略单元门禁，仍构建、验收和清理应用。

## 常用 Agent 与日常界面

菜单栏采用原生锚定弹出面板：四个常用配置以 2×2 按钮完整平铺（默认第一行 Antigravity、DSH，第二行 Cindy、Codex；“其他配置 → 调整常用配置”可逐位置更换，已置顶项会交换位置，排列自动保存），无需滚动即可切换当前控制配置；其他配置与连接详情使用次级菜单，支持暂停按键控制和打开工作台。工作台采用浅绿强调色、常用 Agent 侧栏、按键预览与动作编辑区；设备发现和输入监视收在“设备与连接详情”。浏览配置不会改变当前控制目标，需点“启用此配置”。

菜单栏新增“自动跟随前台应用”“应用关联…”和“按键速查”。自动跟随默认关闭；选择应用并关联稳定 Profile 后才生效。未关联应用保持上一套；手动选择（包括重选当前配置）保持到“恢复跟随”。切换成功后显示 1.5 秒提示，速查显示实际宿主 Profile 的首层映射，不显示文本内容或执行按钮。启动保留已有选择和按键暂停状态；输入流丢失时使用“恢复输入”，松开按键、摇杆回中后再触发。无径向菜单或设备写入。设计与边界见 [S5](docs/designs/app-following-and-hud.md)。

首次安装预设时默认优先 Cindy，并提供 Codex、千问办公（QwenWorkCN）、ZCode 的独立配置。按键 01 打开／聚焦指定应用；02 聚焦当前窗口唯一输入区；03 搜索任务；千问办公和 ZCode 的 04 为新任务。聚焦与 Cindy / Codex / ZCode 搜索使用目标应用辅助功能控件，千问办公搜索与两者新任务使用已核对快捷键；不预置发送或审批。按键 02 不是精确会话 ID 路由，多个候选、缺少输入区、权限不足或界面变化时明确失败。辅助功能权限与设备连接状态单独显示。

“试运行此动作”使用同一 Rust 路由与真实 Mac 执行器，要求当前编辑配置已启用且按键控制开启；它不伪造硬件事件。原生实测与合成按键链路分别验收。Cindy / Codex 搜索与聚焦、千问办公搜索／新任务／聚焦、ZCode 搜索／聚焦／新任务已在本机 UI 核对；早期 Cindy 回执按旧映射记录为 AG01 打开应用、AG00 聚焦输入框；后续逐键实测确认左键 AG00、右键 AG01，已修正首行映射（见 E-100 续验），修正版已取得左键 AG00 打开 Cindy、右键 AG01 聚焦输入框的 executed 回执；其他 Agent 的实体按键逐项验收、完整多会话控制仍不由这些结果替代。

启动时补齐缺少的常用配置；移除“安装 Coding 起步预设”入口，并在预设安装时仅移除与内置来源完全相同、没有动作或控件设置修改的演示／示例配置。已编辑、改名或导入的配置保留；安装与移除作为同一次可撤销修改保存，重复安装不覆盖已有同名配置。

## 调研入口

Input 1.1.0 的[业务架构与证据定位](docs/input-architecture.md)、[写入和宿主通知候选协议](docs/protocol-candidates.md)、[独立实现模块契约](docs/module-contracts.md)已基于当前安装包整理。该轮为静态分析，未新增设备操作或运行时插桩验收；原始材料仍保留在 `.scratch/input-study/`。

可直接用于只读客户端实现的结论见 [USB 发现、身份与配置读取协议](docs/protocol-readonly.md)。依据 Input 1.1.0 静态分析及本机 IOKit 枚举；独立IOKit探针曾读回0.6.3；应用身份路径已读回0.6.3；当前0.6.3的配置读取及原生只读浏览已有E-079验收；完整生命周期和物理映射仍待验收。

以下为本机原始调查材料，保存在 Git 忽略的 `.scratch/input-study/`，不随源码提交：

- [官方来源、能力矩阵与公开协议线索](.scratch/input-study/out/research.md)
- [已连接 UI 清单](.scratch/input-study/out/connected-ui-inventory.md)
- [灯光、摇杆、层、AppSense 与参数实验](.scratch/input-study/out/layer-lighting-joystick-followup.md)
- [观察规格](.scratch/input-study/out/spec.json)和[证据记录](.scratch/input-study/out/evidence.jsonl)
- [原任务历史](.scratch/input-study/task-history.md)

旧入口 `~/agent-tasks/worklouder-input-study` 仅保留晋升记录和兼容链接。后续项目事实、代码和进度只在本项目维护。

T05已补取消、会话终止通知及退出时等待资源释放；当前设备两次独立身份查询→关闭→重开已通过。配置读取中物理拔插、睡眠和原有映射行为仍待实际验收，不能据此签收完整生命周期。见[E-082](docs/spec/EVIDENCE.md)。

## 下一步

[实机验收准备](docs/designs/hardware-acceptance.md)已定义先备份、单临时文件最小实验、停止与恢复条件；`scripts/prepare-hardware-acceptance` 仅生成/校验本地准备包，不连接设备、不授予执行许可。当前没有采集新实机备份，配置写入与RGB阶段继续独立阻断。

S3 离线模型与候选（E-100）、持久操作记录与失败恢复（E-101）已完成核心和合成验证；高层 FFI、宿主动作排空及产品恢复界面已完成离线接线（E-102）；两区 RGB 编辑、六种效果参数及合成预览/恢复已实现（E-103）；已与应用跟随/HUD、左右键映射修正整合，并补齐同层外部改色保护和队列排空回归（E-104）。原生候选的分块与回应、RGB 两区编码及零发送边界也已完成离线实现（E-105）；后续缺口限于文件覆盖/恢复、预览目标绑定/运行态核验和对应硬件效果证据，不能以合成结果开放真实设备。真实设备连接与写入不属于本轮授权。

PR #12/#13 已交付原生离线工作台、结构化动作和本地持久化。当前已有设备发现、身份查询与[#5 配置读取](https://github.com/semantic-craft/micro-input/issues/5)实现；配置原生浏览验收见[E-079](docs/spec/EVIDENCE.md)，[#6 生命周期](https://github.com/semantic-craft/micro-input/issues/6)及完整硬件写入恢复仍待验证。范围继续保持设备 Flash 只读，不从核心读回推定全部 S3 完成。

完整目标与 35 张工单入口为 [#44](https://github.com/semantic-craft/micro-input/issues/44)，技术契约见 [实施设计](docs/implementation-plan.md)，阶段验收见 [BUILD_PROMPT](docs/spec/BUILD_PROMPT.md)。用户已授权限定临时文件的自动实机验收，A 的11字节单次写入和 B 的12字节两段分块写入均完成读回、删除及配置保全，见 E-091/092；结果只覆盖各自固定临时文件路径，生产应用继续只读，配置应用与恢复仍不可用。供工作台审阅的本地草稿 CLI v1 已实现，支持改名 Profile/Layer 和重绑定已有动作，提案需用户明确接受。S2/S4 宿主动作执行闭环（`Shortcut`、`PasteText`、`OpenApp`、`OpenUrl`、`HerdrAdapter`、`DshAdapter`）、`WideKeyMerger` 2U 宽键去重与三套 Coding Agent 预设已落地并通过合成端到端验收（见 E-098）。

原生验收使用 `scripts/run --verify` 输出的 `QA_APP_PATH` 精确选择应用；默认使用固定 `com.semanticcraft.microinput.qa` bundle ID（避免 TCC 权限漂移）与 `.scratch/qa-runs/qa.lock` 单实例锁，退出后自动清理 `.app` 与临时文档并保留 `evidence/launcher.json` 及 `evidence/audit.jsonl`。QA 身份不能替代生产签名与权限验收。

### 桌面 Agent 常用控制

七个对话型 Agent 共用位置：01 打开、02 聚焦输入、03 搜索、04 新任务（应用提供时）、05 模型菜单、06 思考力度入口、07 听写、08 内置浏览器、09 终端面板。操作绑定应用标识，通过当前窗口原生控件或已核对快捷键执行；未提供的功能会返回明确提示。模型和力度键打开菜单供用户选择，不自动更改模型。Qoder CN 与千问办公当前没有单独的力度入口；千问办公“标准／高级”属于模型档位。听写必须确认录音状态变化才记录成功，且不发送消息。

Herdr Profile 面向 `dev.herdr.app` 桌面 App：01 打开、02 Space/Tab 选择器、03 命令搜索、04 新 Space、05 新 Tab、06/07 右/下分屏、08/09 前后 Space、10/11 前后 Tab、13 面板缩放；旋钮按压切换侧栏，摇杆四向移动面板焦点。新 Space、Tab、分屏会创建终端。依据本机 `~/.config/herdr-app/keys.json` 与 [Herdr App 官方说明](https://herdr.dev/)；用户重绑 Herdr 快捷键后需同步修改 Profile。它与 Herdr TUI 的会话和 CLI 不通用。

仅升级与旧内置动作完全一致的 Profile；保留 Profile/Layer ID、用户改过的动作及其他配置。新增操作不会写设备 Flash。当前验证证据与应用能力差异见 [桌面控制验收](docs/verification/desktop-agent-controls.md)。

听写验收：Cindy、Codex、千问办公已由真实执行器核对启动和停止；千问按钮带快捷键后缀，状态识别同时接受“语音输入（…）”与“停止录音（…）”。若应用本身无响应需先恢复应用，不能把 AXPress 返回值当作录音成功。菜单与侧栏使用本机应用图标，紧凑窗口侧栏可独立滚动；四个菜单常用项仍固定平铺。

H0/H1 独立验收执行器已接线：`scripts/hardware-acceptance` 默认只运行假 USB 回归，实机入口需要明确范围、路径及材料，H1 另需原配置摘要和未决操作核查。见[执行说明](docs/designs/hardware-acceptance.md#h0h1-独立执行入口已接线实机未运行)。本轮未连接设备，生产配置写入和 RGB 仍未开放。
