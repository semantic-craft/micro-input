# 桌面 Agent 控制验收

以下实测来自 2026-11-03 的版本。该轮以本机应用的 AX 控件、Herdr App 自带 `keys.json`/`herdr-app commands` 和官方安装包为依据。采用研究入口中的 IA-01、IA-10、IA-13、IA-14：配置/路由与设备写入分离，不把社区 CLI 示例当作当前桌面 App 协议。

## 当前范围

- 四个桌面 Agent Profile 与通用预设，菜单栏仍保留可自定义的四个 2×2 常用位置。菜单栏提供四个默认常用位置，可手动替换为任意内置方案或通用预设。
- Antigravity、DSH 的未改动旧预设迁移为指定应用的动作；不再向任意前台窗口发送推测快捷键，不再执行不存在的 DSH CLI 子命令。
- 对话型 Agent 的 05–09 分别打开模型、思考力度、听写、浏览器和终端入口。缺少入口或目标不唯一时明确失败。模型/力度只打开选择菜单，不替用户选值。
- Herdr 桌面 App 使用 `dev.herdr.app` 定向快捷键，与 Herdr TUI 分离。Space、Tab、分屏键会创建终端；默认不配置审批、发送或关闭已有工作区。
- 迁移保留 Profile/Layer ID。只有整套动作与历史内置定义完全一致才升级；修改过的 Profile 保留。一次撤销可恢复整次升级，重复安装无变化。

## 已观察的应用差异

| 应用 | 实测入口及限制 |
|---|---|
| Antigravity | 打开、聚焦 Message input、搜索弹层、新会话已实测。模型/力度共用选择器；已修复长会话正文耗尽 AX 扫描的问题，最终真实菜单复测通过。 |
| DSH | 打开、聚焦、搜索、新建按钮、模型与推理等级菜单、内置浏览器已实测；终端使用右侧工具提示的 Control+反引号，已由 Micro Input 实测打开并聚焦终端。未观察到听写入口。新建按钮在空首页验收，不代表服务端已创建带消息的会话。 |
| Herdr App | 打开、Space/Tab 选择器、命令面板、新 Space、新 Tab、右/下分屏、前后 Tab 已实测。测试 Space 命名 Micro Input QA，创建后仅关闭该 Space，并恢复原 Space。未读取终端内容或向代理发消息。 |
| Codex | 官方 Control+Shift+M 已由 Micro Input 实测打开模型/力度菜单；Command+T 直接实测打开内置浏览器。Control+反引号依据官方终端快捷键配置，终端视觉结果尚未确认。听写已由真实执行器确认启动/停止，并观察录音波形。 |

## 验证边界与证据

- `.scratch/ag-dsh-controls/` 保存构建、门禁、官方安装器清单和本轮 QA 定位日志；原始 UI 与本地配置不提交。
- 基础生产 QA：`.scratch/qa-runs/1793698749168-47607/evidence/`，13 条真实执行回执，三个应用的四项基础动作及一次搜索复验；launcher 确认 QA 进程、包、文档、偏好已清理。
- 扩展生产 QA：`.scratch/qa-runs/1793699154255-53998/evidence/`，包括真实 Herdr App 与模型/浏览器/听写入口；保留失败回执用于区分应用界面不支持、设置页和长会话扫描问题。
- `scripts/check` 验证路由、迁移、用户修改保留、模型/力度能力差异和内容控件拒绝。合成 `host-execution` 场景已核对 20 条 simulated 回执，包括四个非受保护应用的打开、AG/DSH 的基础动作与滚轮/提交/宏以及 Herdr 宽键合并/摇杆；它不证明真实 UI 行为。
- 实际 UI 动作通过“试运行此动作”调用同一 Rust 路由与 Mac 执行器。未要求用户按实体键盘；不将软件验收声称为新的实体按键验收；本轮不写设备 Flash、不发送消息。

- 最终生产 QA：`.scratch/qa-runs/1793700139810-69090/evidence/`，executed 回执逐项确认 AG 模型菜单、DSH 终端、Codex 模型/力度菜单；`result.json` 与清理回执均已核对。
- 合成 QA：`.scratch/qa-runs/1793699869097-64960/evidence/`，simulated 回执与清理结果通过。
- `scripts/check` 最终完整门禁通过；旧版安装时曾自动升级预设，辅助功能旧签名登记刷新后，Herdr Space/Tab 选择器由当时的安装版实际打开，`herdr-app status` 观察到 `overlays: ["picker"]`。
- 最后菜单修复重新构建并安装，刷新权限后再次打开 Herdr picker。四个卡片均实际点击切换成功，无需滚动；“其他配置”与四个位置的候选中包含 Herdr 等内置配置，均无演示入口。
- 安装版原有两个演示 Profile 带历史数据而保留；菜单栏不再展示这些演示入口或将其列为常用候选，数据仍可在工作台访问。

来源：[官方桌面快捷键](https://learn.chatgpt.com/docs/reference/commands)、[Herdr App 官方说明](https://herdr.dev/)、本机 Herdr App 生成的接口说明与实际 AX 控件。应用更新、换模型、切换界面或改快捷键后应重新验收。
