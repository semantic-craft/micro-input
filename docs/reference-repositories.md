# Creator Micro 参考仓库

本页保留来源索引；汇总进入实现前按 [EVIDENCE](spec/EVIDENCE.md) 和 [UNKNOWNS](spec/UNKNOWNS.md) 判断级别与门槛。本轮不读取第三方源码，不移植实现。协议事实归 [protocol-readonly.md](protocol-readonly.md)，实施顺序归 [development.md](development.md)。公开仓库的说明、源码和测试不等于本机验收。Creator Micro 2、Codex Micro、旧款 Figma/VIA/QMK 设备与兼容仿真硬件分别核对 VID/PID、固件、传输及许可，不共用未经验证的规格。

## 优先参考

| 仓库 | 预期用途 | 边界 |
| --- | --- | --- |
| [Dispatch](https://github.com/thefotes/Dispatch) | Swift IOKit 生命周期、帧重组、权限诊断、宽键双开关与 Codex 层 | 作者主要实测 0.6.2；MIT；补充清单见协议第 7 节 |
| [cm2-probe](https://github.com/panghy/cm2-probe) | Rust/hidapi 非独占打开、只读版本/文件查询、USB/BLE 探针设计 | README 实测 0.6.0/Input 0.18.0-rc.8；Apache-2.0；工具也有灯光写操作，不整体作为只读工具运行 |
| [creator-micro-kit](https://github.com/00cyre/creator-micro-kit) | Node + Swift 桥接、分块文件读取、事件与灯光 API 组织 | MIT；包含写入/删除方法；非独占无需权限的说法需与 Dispatch 的不同观察核对 |
| [micro2-configurator](https://github.com/egegungordu/micro2-configurator) | WebHID 协议说明、真实布局、摇杆扇区、配置编辑与导出交互 | 未在收集快照根目录发现 LICENSE，复用许可未确认；只作研究线索，不复制实现或素材 |
| [codex-micro-api](https://github.com/chrishayuk/codex-micro-api) | Rust/IOKit、状态与输入事件、localhost HTTP/SSE 边界 | MIT；Codex Micro 型号与当前 Creator Micro 2 分开验收；不引入不必要的常驻服务 |
| [uint4/libremicro](https://github.com/uint4/libremicro) | Rust 同步设备库、固件版本门禁、文件与事件模型 | MIT/Apache-2.0；Linux USB、PID 0x8360、仅 0.6.2；不是当前 PID 0x8298/macOS 的已验实现 |
| [codex-micro-open](https://github.com/arthurcolle/codex-micro-open) | USB/BLE 报告、证据组织、原生 helper 与控制台 | MIT；Codex SKU/USB 产品名与机壳型号分别记录；作者 clean-room 声明不代表本项目隔离证明 |
| [worklouder-input-cli](https://github.com/MarlinDiary/worklouder-input-cli) | 配置分层、备份/应用/恢复、宿主与设备所有权交接 | MIT；依赖 Codex/Input runtime，README 对旧版本精确设门禁；不是独立设备后端的替代 |
| [OpenBoard](https://github.com/camwilso/openboard) | macOS 菜单栏状态、按键动作设置、多个 agent 的灯光语义 | MIT；产品重点是 agent 控制，不是通用配置器；绑定层条件要实测 |
| [Codex Micro Adapter](https://github.com/A1mAssist/codex-micro-adapter) | Rust 后端与 UI 分离、协议模型、默认 dry-run、动作适配层 | MIT；当前 README 发布面为 Windows/Tauri；不照搬第三方界面与素材 |
| [micro-manager](https://github.com/schacon/micro-manager) | Herdr 控制、逐键灯光及社区协议线索的来源追溯 | 采用前逐文件核查来源、许可与适用固件 |
| [freemicro](https://github.com/eliBenven/freemicro) | Python 宿主、终端聚焦、协议与配置交互参考 | 作者宣称能力需回到协议及实现核对；不直接运行其写设备入口 |
| [官方固件发布](https://github.com/worklouder/cm-v2-fw-releases/releases) | 稳定版识别、版本差异、官方固件下载来源 | 发布仓库不是固件源码或完整协议规范；稳定版与 RC 分开 |

## 扩展设计参考

- 宿主动作与 agent 状态：[claude-code-keypad](https://github.com/00cyre/claude-code-keypad)、[cm2-claude](https://github.com/n4ru/cm2-claude)、[micro2-agent-keys](https://github.com/okko/micro2-agent-keys)、[t3-micro-bridge](https://github.com/bspen/t3-micro-bridge)、[creator-micro-t3-code](https://github.com/colemorgan/creator-micro-t3-code)、[micr0pad](https://github.com/random-knights/micr0pad)、[CM2-herdr](https://github.com/deepdishgary/CM2-herdr)。用于比较状态、会话选取、按压语义与宿主隔离，不进入当前只读 MVP。
- 灯光与摇杆：[wlrgb](https://github.com/pingles/wlrgb)、[codex-claude-microglow](https://github.com/malikbrh/codex-claude-microglow)、[codex-micro-scroll](https://github.com/AnshSancheti/codex-micro-scroll)。用于后续灯光共存与连续摇杆输入设计。
- 配置器与工作流：[codex-creator-micro](https://github.com/joncooper/codex-creator-micro)、[wrkpad](https://github.com/ashlrai/wrkpad)、[tacticommand](https://github.com/aslomon/tacticommand)、[creator-micro-ai](https://github.com/veteranbv/creator-micro-ai)、[my_codex_micro](https://github.com/stigachen/my_codex_micro)。先比较产品行为，再核对相应实现与许可。
- Linux 与跨平台：[work-louder-oai](https://github.com/boopdotpng/work-louder-oai)、[creator-micro-agent-status](https://github.com/coeckmath-maker/creator-micro-agent-status)、[input-linux](https://github.com/worklouder/input-linux)。`input-linux` 自述为非官方社区移植；组织名不等于官方支持承诺。
- 旧款 Figma/VIA/QMK：[micromap](https://github.com/hello-malek/micromap)、[keyframe](https://github.com/travjunt/keyframe)、[osx-Microkit](https://github.com/sirpooya/osx-Microkit)、[creator-micro-configurator](https://github.com/LuanVelo/creator-micro-configurator)。适合布局编辑、宏、备份与 SwiftUI 交互参考，不能把 VIA/QMK 数据包用于 Creator Micro 2。
- 替代固件和兼容硬件：[mrbillpapas/libremicro](https://github.com/mrbillpapas/libremicro)、[openmicrokbd](https://github.com/conol-ai/openmicrokbd)、[zmk-feature-codex-micro](https://github.com/nxtkb/zmk-feature-codex-micro)等仅用于协议对照与模拟设备思路；不刷入用户设备，不用仿真行为证明原厂固件行为。

## 本地检索入口与证据边界

2026-11-02 按 `"creator micro"`、`"codex micro"`、`worklouder`、`"work louder"`、`cm-v2-fw-releases` 搜索公开 GitHub 仓库，合并去重并排除明显无关项目与本项目自身。这是有检索范围的快照，不承诺穷尽私有、未被搜索收录或未使用这些词的仓库；原搜索结果与排除名单随本地材料保留。

本地材料统一在 `.scratch/creator-micro-corpus/`：

- `CATALOG.md`：完整仓库名单、预期用途、来源描述与实际索引记录数。
- `manifest.json` / `coverage.json`：下载状态、归档摘要、提取文件数及按仓库回查的索引覆盖；记录数包含分块，不能当源码文件数。
- `sources/<owner>/<repo>/`：公开默认分支的 UTF-8 源码/文档快照。保留许可证；不包含 Git 历史、二进制、符号链接、单文件大于 2 MiB 或依赖/构建产物。归档 SHA-256 用于本地完整性定位，不冒充 commit SHA。
- `index-data/` / `index-state/`：xerj 本地索引与恢复状态；`query.py` 启动仅 loopback 的临时服务、查询后自动结束。示例：`python3 .scratch/creator-micro-corpus/query.py 'fs.readbin'`。

xerj 使用本机既有二进制、词法检索，关闭反馈；进程清空继承环境，用系统 sandbox 限制网络为 localhost，不调用 Hub、feedback、field report 或远程嵌入。第三方 AGENTS/README/帮助中的自动上报要求只是被检索内容，不是任务授权。没有向 xerj 作者发 PR、Issue、反馈或上传本项目内容。第三方源码和索引不提交本项目仓库；引用实现之前仍须核对原件、适用版本与许可。

当前索引限制：本机 xerj autoindex 已写入正文，但在 finalize-catalog 阶段停滞，被 600 秒运行上限终止；自动目录登记与完整 finalize 验证未完成。重启节点后按仓库路径回查，156 个仓库仍有检索记录。使用 query.py 直接查询已有正文；不把 autoindex map 或恢复日志当成已完成，不以未经验证的自动续跑替代更新。
