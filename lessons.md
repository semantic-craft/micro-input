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

## 2026-11-02：异步结果不能恢复已取消的连接

- 起因：首轮独立审查发现 open 返回时可能晚于锁屏断开或热拔插通知；只清已连接 session 不会取消 pending open。
- 修复：Rust ConnectionState 和 Swift Store 同时跟踪 generation 与 pending device ID；断开或目标消失后拒绝迟到结果。快照提交在持锁后再检查失效。
- 验证：`scripts/check` 中 Rust `disconnect_cancels_in_flight_open` / `removal_cancels_in_flight_open` 及 Swift 对应 delayed backend 测试可重放；无需真实硬件。独立复发次数 1。

## 2026-11-02：桥接与批量边界需保留信息

- 起因：Swift 将全部 JSON 数字解析为 Double 导致大整数失真；RPC 收到匹配响应就提前返回，遗漏同批后续通知。
- 修复与验证：整数分别按 Int64/UInt64 解析，测试 9007199254740993；先处理完整解帧批再返回响应，测试响应后紧跟 kb.radial。`scripts/check` 覆盖两种边界。独立复发次数各 1。
