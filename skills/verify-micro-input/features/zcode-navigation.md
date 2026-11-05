# ZCode 精确会话导航

菜单栏的「ZCode 会话连接」默认关闭。ZCode 原生桥先授权具体工作区，Micro Input 开启连接后才读取私有授权文件与任务元数据。菜单用工作区和任务名区分同名项，执行时冻结完整地址、应用实例及连接代次。

## 隔离应用检查

```sh
MI_AUTO_HOST_READY=0 scripts/verify-offline
```

总入口包括 `zcode-navigation`。单独重跑时先构建合成应用，再运行：

```sh
scripts/build --synthetic
MI_AUTO_HOST_READY=0 MI_SYNTHETIC_SCENARIO=zcode-navigation scripts/run --verify --timeout=45
```

读取启动器打印的 `QA_RUN_DIR` 下 `evidence/launcher.json`、`audit.jsonl` 与 `zcode-navigation.json`。总入口核验它们后生成 `result.json`，并确认 QA 进程、应用副本和隔离文档已清理。

场景经菜单使用的 Store 队列与 Rust 宿主租约执行两个同名目标及受控 v4 会话命令。检查完整 selected 回读、双会话仅影响当前选中目标（`selected_only_isolation`）、工作区绑定 `createSession` 与新 ID 回读确认（`create_workspace_readback`）、运行中发送不误建新会话且严格区分 `guide` 实时 steering 与 `queue` 普通入队（`inflight_steering_vs_queue`）、携带旧 `executionId` 的迟到 `stop` 返回 `noop` 不停止新执行（`late_stop_noop`）、重复 `commandId` 幂等去重（`duplicate_command_dedup`）、超时或结果未知时锁存原 `commandId` 对账且绝不换新 ID 重放（`timeout_same_id_no_replay`）、错误选中未确认、删除后不重定向、换代旧目标不发送、断连清空列表、未启用及越权请求零读取。审计只有固定动作类别（`zcode_open`、`zcode_create`、`zcode_send`、`zcode_stop`、`zcode_query`）和结果，不记录任务标题、路径、任务 ID、正文或授权 token（`text_byte_len` 固定为 `null`）。

合成场景的 `native_app` 和 `physical_key` 固定为 `NOT_RUN`。它证明 Micro Input 的队列、生命周期与确认逻辑，不证明安装版 ZCode 已具备桥。

## 原生源码与双端检查

原生补丁、固定上游版本、授权配置和重跑命令见 [ZCode 桥](../../../integrations/zcode/README.md)。只有真实 UDS 双端运行才能证明 Swift 客户端与原生协议相容。源码夹具、实际 Electron 渲染层、日常安装版及实体按键分别记录，不互相替代。

本片已完成精确导航（Slice G）与受控新建/发送/停止及 steering 确认（Slice K1）；审批、问题回答与模型切换归 L1，带完整工作区身份的前进/后退归 N。
