# 当前实施入口

接手未完成工单或产品改动时，先执行[研究成果接入规则](agents/research-context.md)。其中包含 Input 1.1.0 的证据索引、按功能阅读路径和可直接使用的接续提示词；实际工单状态以 GitHub 当前记录为准。

PR #12/#13 已交付离线原生工作台、稳定文档 ID、结构化动作与 Rust 本地持久化。设备发现、身份查询及 #5 有界配置读取已实现；当前0.6.3的核心读回和原生只读浏览见E-079。#6完整生命周期、其他固件和物理映射仍待验收，未知后端继续禁止发送。

本地v2文档、v1原字节备份迁移、Profile/Layer管理和真实快照显式创建独立草稿已实现，证据见E-083；三方比较已实现，身份仅有临时接口时保留明确的整份选择，自动合并保持禁用；设备应用仍未开放。

先读 [实施设计](implementation-plan.md) D01–D07 与 [BUILD_PROMPT](spec/BUILD_PROMPT.md) 的 S1，再按对应工单推进。完整产品与依赖入口为 [#44](https://github.com/semantic-craft/micro-input/issues/44)。当前继续开发本地功能与适用验证；设备操作遵守证据和恢复门槛，不因规划批准而解锁宿主执行或自动化。

# 开发起点

## 已确定的产品方向

为用户自己的 Creator Micro 2 做独立 macOS 应用，Rust 承担配置模型、协议与核心逻辑，SwiftUI 承担原生界面。目标是改善原 Input 的使用体验与性能；性能问题尚未量化，不能把技术栈本身当成性能验收。

## 兼容能力范围

需要逐步覆盖 Profile / Layer、基础按键与旋钮、普通宏、四分支 Multi Action、灯光两区、径向菜单、AppSense、Cheat Sheet，以及宿主执行的 Text / Command / URL / App Smart Action。设备保存的行为与需要宿主常驻的行为分开建模。

本机观察版本为 Input 1.1.0、设备固件 0.6.3；不代表其他版本。普通层灯光有 Off/Solid/Snake/Rainbow/Breath/Gradient 六类。AppSense 通过目标应用前台停留 5 秒采集。具体证据和来源见 README 的调研入口。

## 首轮开发范围

首版为 USB 只读配置查看器，范围与可勾选验收项统一维护在 [MVP 总工单 #1](https://github.com/semantic-craft/micro-input/issues/1) 及其子工单，不在本地重复维护工单状态。初始化基础已由 PR #12/#13 提供；下一步是 [#3](https://github.com/semantic-craft/micro-input/issues/3)，不重复初始化工程。

模拟数据用于建立可运行技术链路，不等于设备验收。身份和配置读回须在当前实机验证；原 Input 缓存不能充当设备快照。首版不含设备配置写入或宿主动作执行，后续写入仍需独立的备份、提交、读回及恢复设计。

## 未解决的问题

### Dispatch 证据进入开发的顺序

依据及逐项待验边界见[协议第 7 节](protocol-readonly.md#7-dispatch-补充证据与当前固件核验)。Dispatch 0.6.2 的社区实测与本机 0.6.3 的事实分开记录；工单验收仍以当前设备为准。

1. **#3 设备发现**：检查 device usage pairs 与 primary usage 的区别；保持 USB、VID/PID 和多候选选择约束。枚举不请求 Input Monitoring。
2. **#4 身份与 RPC**：核对 report ID 的后端适配、JSON 对象/行结束边界、长短字段、外来 ID 与迟到响应；连接失败区分权限与 Secure Input。本次独立探针已读回 0.6.3，但不能据此关闭应用身份票。
3. **#5 配置查看**：继续采用 fs.readbin、大小限制及前后校验；把 fs.read 留作社区候选。加入 [2,4,4,3] 行宽、宽键双开关、旋钮方向、未知绑定和非连续 ID 的合成夹具，再与真实配置及物理布局核对。
4. **#6 生命周期**：验证 manager/缓冲区寿命、断连取消、重连新会话、Input/Codex 共存和退出后重新打开。ID 范围不能替代跨进程冲突检查。
5. **MVP 之后**：再设计 BLE、宿主事件、宽键去重、Codex 灯光共存与配置写入/恢复；不因发现社区可调用方法而开放写入口。

2026-11-02 固件核验：独立 USB `sys.version` 返回 **0.6.3**，官方 latest 同为稳定版 **0.6.3**，无需升级；用户选择稳定通道，不安装 0.7 RC。本次没有配置读取与写入，映射行为验收仍归后续实机任务。

相关仓库的设计用途与本地检索入口见 [参考仓库](reference-repositories.md)。它们提供候选证据，不自动扩大 MVP 或设备写入范围。

### 仍待验证

- 当前版本的读协议已整理到 [只读协议说明](protocol-readonly.md)：发现条件、HID 分帧、状态回退、分块读取和缓存边界已有静态证据；直接 IOKit/Rust 后端的报文适配和实机 RPC 仍待验证。
- 厂商没有在已检索资料中给出完整、版本化协议规范；社区项目的型号、固件、USB/BLE 和 HID 报文布局不同，不能直接混用。
- 灯光滑杆的 min/max/step、Magic 含义、径向扇区上限、Joystick 模式条件，以及宏容量等仍未证实。
- 未保存草稿中的数值显示不能当固件范围；Multi 初始 250ms、Delay 初始 200ms 只是观察值。
- 官方 7 工具与社区 8 扇区（含取消）的关系尚未通过边界实验验证。
- 性能根因、后台 CPU/内存、事件延迟、USB/BLE 重连、设备读回与异常恢复尚未测试。

## 来源与实现边界

已有材料包含官方文档、本机黑箱观察及带来源的社区公开研究，部分社区材料源自作者分析旧版应用；2026-11-02 按用户授权新增 Input 1.1.0 随包 JavaScript 静态分析，未执行原模块或反编译原生二进制。当前没有严格 clean-room 隔离证明。原始材料留在 .scratch，独立实现只接收整理后的协议说明和合成向量；不复制标注 UNLICENSED 的 device-kit 实现。新界面使用原创设计与素材，社区代码或模型按其许可和版本核查后再使用。

当前应用实施与验收状态以 MVP 工单为准；本页的独立版本查询不代表应用已经完成。原有映射仍需保留；之前的临时 Profile 切换授权不等于任意设备写入、刷固件或安装替代驱动的授权。

## 应用与菜单栏图标

应用使用已选定的薄荷绿短发角色原画 `app/MicroInput/Resources/AppIcon-artwork.png`。`scripts/build` 调用 `scripts/generate-app-icon.swift` 生成 16–1024 px 的图标表示并打包为 AppIcon.icns；圆角和 5% 透明外边距与 Koe 的 macOS 图标一致。原画不带截图外框。

菜单栏由 `StatusItemIcon.swift` 绘制 20 × 18 pt 的键盘与旋钮符号，设置 `isTemplate`，由系统处理深浅色及选中着色。尺寸是本项目选择，不是 Apple 强制尺寸。遵循 [Apple 菜单栏规范](https://developer.apple.com/design/human-interface-guidelines/the-menu-bar) 的黑色/透明符号建议及 [NSImage.isTemplate](https://developer.apple.com/documentation/appkit/nsimage/istemplate) 的模板语义。
