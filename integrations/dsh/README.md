# DSH 同 Host 状态与导航桥

`navigation.patch` 固定于官方 `dsh-v0.2.1-rc.5`，SHA 见 `BASE_SHA`。桥装在原桌面 Host 进程的 `apps/desktop-host/src/index.ts` 中，借用原有 Session、Agent、审批与问题服务；没有创建第二套 runtime，也不读取桌面认证 cookie。

该版本的 `SessionFollowRequest` 没有 `activate` 参数，历史 follow 会打开正文路径。桥因此不调用该接口。它提供自己的 `subscribe(activate: false)` 元数据契约，只读取显式授权、已经加载且投影已更新至当前游标的 Session。冷会话、已删除会话或投影尚未建立的会话列入 `unavailableSessionIds`，不会被物化、唤醒或假报为空闲。该片没有提供冷目录发现功能。

`pendingInput` 是需要人回应的数量，包括审批、阻塞式问题和未回答的定时问题。它不是 inbox 队列长度。审批与前台问题由原服务持有计数，定时问题从已经物化的当前投影取计数；`currentTurn`（当前活跃轮次号）与 `queuedCount`（`agent/inbox/spliced` 待消费队列长度）在 `SessionProjectionRegistry` 随 `session/created`（`seq === 0`）与增量 `session/event` 同步维护，不会为了订阅或停止核对而扫描历史（`snapshotEvents` / `eventAt` 零调用）。同步状态日志保存轮询间的每次变化，超过 1024 条且客户端未追上时返回 `gap`，由客户端重新连接。测试包含 `1 → 0` 短变化、先存在后订阅的审批、两种投影注册顺序。

## 启用与协议

默认关闭。显式启动已应用补丁的桌面版时，`MI_DSH_BRIDGE_CONFIG` 指向当前用户拥有、0600 的配置文件，例如：

```json
{"directory":"/absolute/private/short-directory","sessionIds":["explicit-session-a","explicit-session-b"]}
```

目录必须为当前用户拥有的 0700 目录，Unix socket 路径须短于 104 字节。桥独占 `dsh.lock`，写出 0600 的 `dsh.sock` 与随机 capability 的 `dsh-grant.json`。Micro Input 默认查找 `~/Library/Application Support/MicroInput/dsh-grant.json`；可在测试客户端显式传入 grant 路径。配置错误、残留锁或端点占用仅使桥无法启动，原 Host 继续运行。锁不自动接管，确认对应桥退出后才清理其残留。

grant 固定 `version:1`，包含 `socketPath`、`token`、`hostId`、`generation` 和 `sessionIds`。请求先校验 schema、capability、Host 代次和订阅身份，然后才读取元数据。支持 `subscribe`、`read`、`open`、`create`、`prompt`、`cancel`、`query`、`close`。每次连接的 `subscriptionId` 唯一；当前订阅未关闭时，另一个订阅 ID 不能替换它。迟到的 close/open/subscribe 不得改变新连接。断开确认未知时保持关闭状态，不猜测远端已释放。

`open` 同时核对 sessionId 与 workspaceId。原 Host 将请求通过私有父子 IPC 发到其所属 Electron 窗口，再由真实 `UiWorkspaceService` 选择会话，等待所选引用 ready，并回读实际 selection。只有 ID 一致且窗口身份未变才确认并聚焦。宿主对象、同 ID 的 Session 对象或工作区发生变化后，旧选择被拒绝。

Slice K2 在同 Host 桥上接通受控执行：
- `create`：仅允许在当前授权会话已验证的 `workspaceId` 下调用 `sessions.create({ workspaceId })`，导航选中新建会话并回读 `selected == createdSessionId`，同时把新 `sessionId` 动态纳入当前订阅授权范围。
- `prompt`：要求目标会话已处于当前选中状态（`selected == sessionId`），复用 `agent.prompt(sessionId, { mode, message, requestId })` 显式区分 `steer`（运行中轮次实时转向，返回 `status: "steered", delivery: "steer"`）与 `queue`（下一轮排队，返回 `status: "queued", delivery: "queue"`）。
- `requestId` 幂等与 `query` 对账：同订阅内 `create` / `prompt` / `cancel` 均按 `requestId` 记录已结算结果，重复发送返回 `duplicate: true` 且零二次执行；超时或状态未知时客户端锁存原 `requestId` 并通过 `query` 对账，不换新 ID 重放。
- `cancel` 与同 Host 原子轮次核对：要求目标会话已选中（切会话后旧目标返回 `unconfirmed` / `guard.selectionMismatch`，绝不误停）且显式携带 `expectedTurn`。同 Host 在调用 `agent.cancel(sessionId)` 的同一同步临界区内核对 `currentTurn === expectedTurn`：若轮次已推进则返回 `status: "noop", reasonCode: "guard.turnChanged"`（零取消）；若会话运行中但 `currentTurn` 未知或宿主不支持同 Host 原子核对，则拒绝执行并返回 `status: "denied", reasonCode: "guard.unprovableTurnAtomicity"`；取消成功且 `queuedCount > 0` 时返回 `status: "cancelledQueueRemaining"`（明确提示当前轮次已停但队列可能继续，不谎报全部停止），队列为空时返回 `status: "cancelled"`。审批、提问回答与模型档位仍归后续 Slice L2。

## 验证与边界

在固定源码 checkout 应用补丁，运行 `pnpm install --ignore-scripts --frozen-lockfile`。Micro Input 仓库提供两个入口：

```sh
scripts/verify-dsh-native --source /absolute/path/to/patched-dsh
scripts/verify-dsh-paired --source /absolute/path/to/patched-dsh
```

native 入口核对源码与补丁字节，运行生成契约、受影响包类型检查、受影响文件 lint、真实 UDS 与原服务/原 UI store 测试，并生成 `.scratch/dsh-native-result.json`。paired 入口通过真实 Swift 客户端与隔离 Node fixture 交叉验证协议。fixture 使用真实审批/问题生产者和元数据适配器，导航端口为合成选择；真实 UI selection 另由 native 测试证明。fixture 有不超过 10 分钟的 TTL，测试入口回收进程和端点。

源码与进程间配对通过，不等于补丁已安装到日常 DSH，不等于完整 Electron 窗口或实体键盘验收。安装版、实体控件均单列 `NOT_RUN`。本片的读取范围不包含正文、请求问题文本、审批参数、队列消息或凭据。
