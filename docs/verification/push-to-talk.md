# 验证按住说话

先运行 `scripts/check`。`PushToTalkTests` 覆盖宽键完整按下和释放、排队期间松开、迟到启动、开始失败、重复释放、外来会话、过期释放、生命周期清理、开始和停止超时、停止未知时拒绝设备写入及排队 UI 操作重新校验路由。

## 驱动合成应用

```sh
scripts/build --synthetic
MI_AUTO_HOST_READY=0 MI_SYNTHETIC_SCENARIO=push-to-talk scripts/run --verify --timeout=20
```

按项目 `verify-micro-input` 技能核对启动器隔离与清理回执。`audit.jsonl` 应有同一 Profile、`AG06` 的 `dictation_start` 和 `dictation_stop`，两条结果均为 `simulated`。模拟器不访问麦克风，不发送文本。记录 `result.json` 时明确实际设备和原生录音均未运行。

## 核对原生录音

取得桌面独占窗口后，在目标应用打开可丢弃的输入页面，确认没有进行中的录音。使用同一合成 HID 构建，显式选择原生目标。

```sh
MI_AUTO_HOST_READY=0 MI_SYNTHETIC_SCENARIO=push-to-talk-live MI_LIVE_DICTATION_ADAPTER=cindy scripts/run --verify --timeout=40
```

`MI_LIVE_DICTATION_ADAPTER` 只接受 `cindy`、`codex` 或 `qwenwork`。此场景使用合成 HID，但调用真实辅助功能录音控件。它不发送消息，也不连接实际键盘。核对原应用先出现停止录音状态，释放后回到开始录音状态。核对审计的开始和停止均为 `executed`；权限缺失或状态未知均登记为未通过。停止后控件重建时，仅允许在原进程实例、窗口身份及标题不变且原控件已尝试停止的条件下，完整扫描原窗口内唯一空闲控件来只读确认；替代控件永不接收操作。

测试退出后检查 `launcher.json` 的退出和副本清理字段。启动器清理并不能证明录音已停止，必须同时检查原应用与 `dictation_stop` 回执。不要重开已退出的 QA 应用获取状态。

## 核对实体与目标变化

在已授权的设备验收中，保持当前映射，逐个核对普通键和宽键。宽键只松开其中一个微动时必须继续录音；最后一个松开后停止。听写期间切配置、切应用、断连和睡眠，核对原窗口停止且未向新窗口或应用发送任何操作。

同窗切会话须单独检查。当前实现绑定窗口和 AX 控件，不读取应用内部会话 ID。若应用复用相同窗口标题和控件，不能仅凭这些身份宣称已锁定具体会话。

## 参考与边界

参考 `docs/agents/research-context.md`、`docs/module-contracts.md` 的宿主执行与排空契约，以及 IA-06、IA-10 的宿主通知边界。本次只消费已核实输入流的按下与释放，不新增 HID RPC、写映射或扩张设备协议许可。外部 Paseo 的按住说话行为仅作为交互参考，录音确认沿用本项目原生控件状态读取。
