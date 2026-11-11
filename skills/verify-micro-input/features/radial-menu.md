# 轮盘菜单

默认关闭。菜单栏或工作台开启「轮盘菜单」后，先回中再选择八个方向，旋钮按下确认，回中取消。中间距离不允许确认，确认后须再次回中。关闭时保留原四向与旋钮绑定。设置仅保存在本机偏好，不写设备映射。

## 软件门禁

运行 `MI_AUTO_HOST_READY=0 scripts/check`。Rust 测试核对 typed 角度/距离跨同扇区、中间区和回中通知，候选及 `kb.radial` 不升级。`mi-core::radial` 与 `workbench_store` 回归核对当前会话/快照/映射、排空门禁、冻结会话、过期回中、重复确认、五秒选择、路由变化时确认不能落回普通绑定、PTT 释放和取消路径。

只读 `radial_menu_input_readiness` 使用当前 Rust 会话、身份、非过期快照及既有映射核验，不发送硬件请求。当前摇杆不是 VENDOR 或旋钮确认映射不符时菜单不可执行。用户配置中的动作与菜单固定意图区分，菜单不虚构绑定 ID 调用设备动作解析。

## 隔离原生界面

```sh
scripts/build --synthetic
MI_AUTO_HOST_READY=0 MI_SYNTHETIC_SCENARIO=radial-menu scripts/run --verify --timeout=40
```

该场景使用专用合成设备与 `mi-core` 合成输入流。只隐藏 QA 自己的工作台主窗，展示独立非激活面板。前 12 秒选中无历史的后退槽，随后 8 秒选中按键速查，再确认两次、回中、打开后取消。它不会调用任何外部应用、会话或硬件。

在实际面板可见期间截图，核对八槽布局、选中描边及禁用原因。若截图仅捕获主窗而非面板，不能据此签收视觉布局。`radial_menu_preview` 三条回执须为 `simulated`，阶段为 `eight-slots`、`disabled-slot-zero-action`、`center-cancel`；`cheat_sheet` 仅一条 `executed`。前后台 PID 检查包含在场景回执条件中，截图工具的后续激活不冒充面板抢焦点。

按技能核对 `launcher.json`、退出进程与副本/文档/偏好清理。`result.json` 记录代码 SHA、回执数、截图是否实际包含面板以及未验项目。

## 证据边界

参考 `docs/agents/research-context.md`、IA-11、既有输入及宿主排空契约。这里只使用已核实 `v.oai.rad` 的屏幕坐标方向，右为 0、下为 0.25。`kb.radial` 仍未接入。距离 0.5 开始选择、低于 0.35 回中是沿用的客户端策略，八向物理阈值与手感待硬件核验；不复制其他项目的回弹时间阈值。

实体八向、宽键同时操作、实际回中与旋钮手感、多屏/全屏 Spaces 及安装版权限需分别验收。合成输入、矩形边界测试和非激活窗口属性不能替代这些实体验收。不为菜单修改设备配置。
