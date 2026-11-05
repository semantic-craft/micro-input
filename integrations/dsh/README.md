# DSH 同 Host 状态与导航桥

`navigation.patch` 固定于官方 `dsh-v0.2.1-rc.5`，SHA 见 `BASE_SHA`。桥装在原桌面 Host 进程的 `apps/desktop-host/src/index.ts` 中，借用原有 Session、Agent、审批与问题服务；没有创建第二套 runtime，也不读取桌面认证 cookie。

该版本的 `SessionFollowRequest` 没有 `activate` 参数，历史 follow 会打开正文路径。桥因此不调用该接口。它提供自己的 `subscribe(activate: false)` 元数据契约，只读取显式授权、已经加载且投影已更新至当前游标的 Session。冷会话、已删除会话或投影尚未建立的会话列入 `unavailableSessionIds`，不会被物化、唤醒或假报为空闲。该片没有提供冷目录发现功能。

`pendingInput` 是需要人回应的数量，包括审批、阻塞式问题和未回答的定时问题。它不是 inbox 队列长度。审批与前台问题由原服务持有计数，定时问题从已经物化的当前投影取计数；不会为了订阅而回放历史。同步状态日志保存轮询间的每次变化，超过 1024 条且客户端未追上时返回 `gap`，由客户端重新连接。测试包含 `1 → 0` 短变化、先存在后订阅的审批、两种投影注册顺序。

## 启用与协议

默认关闭。显式启动已应用补丁的桌面版时，`MI_DSH_BRIDGE_CONFIG` 指向当前用户拥有、0600 的配置文件，例如：

```json
{"directory":"/absolute/private/short-directory","sessionIds":["explicit-session-a","explicit-session-b"]}
```

目录必须为当前用户拥有的 0700 目录，Unix socket 路径须短于 104 字节。桥独占 `dsh.lock`，写出 0600 的 `dsh.sock` 与随机 capability 的 `dsh-grant.json`。Micro Input 默认查找 `~/Library/Application Support/MicroInput/dsh-grant.json`；可在测试客户端显式传入 grant 路径。配置错误、残留锁或端点占用仅使桥无法启动，原 Host 继续运行。锁不自动接管，确认对应桥退出后才清理其残留。

grant 固定 `version:1`，包含 `socketPath`、`token`、`hostId`、`generation` 和 `sessionIds`。请求先校验 schema、capability、Host 代次和订阅身份，然后才读取元数据。只支持 `subscribe`、`read`、`open`、`close`。每次连接的 `subscriptionId` 唯一；当前订阅未关闭时，另一个订阅 ID 不能替换它。迟到的 close/open/subscribe 不得改变新连接。断开确认未知时保持关闭状态，不猜测远端已释放。

`open` 同时核对 sessionId 与 workspaceId。原 Host 将请求通过私有父子 IPC 发到其所属 Electron 窗口，再由真实 `UiWorkspaceService` 选择会话，等待所选引用 ready，并回读实际 selection。只有 ID 一致且窗口身份未变才确认并聚焦。宿主对象、同 ID 的 Session 对象或工作区发生变化后，旧选择被拒绝。桥没有发送、停止轮次或审批写入接口。

## 验证与边界

在固定源码 checkout 应用补丁，运行 `pnpm install --ignore-scripts --frozen-lockfile`。Micro Input 仓库提供两个入口：

```sh
scripts/verify-dsh-native --source /absolute/path/to/patched-dsh
scripts/verify-dsh-paired --source /absolute/path/to/patched-dsh
```

native 入口核对源码与补丁字节，运行生成契约、受影响包类型检查、受影响文件 lint、真实 UDS 与原服务/原 UI store 测试，并生成 `.scratch/dsh-native-result.json`。paired 入口通过真实 Swift 客户端与隔离 Node fixture 交叉验证协议。fixture 使用真实审批/问题生产者和元数据适配器，导航端口为合成选择；真实 UI selection 另由 native 测试证明。fixture 有不超过 10 分钟的 TTL，测试入口回收进程和端点。

源码与进程间配对通过，不等于补丁已安装到日常 DSH，不等于完整 Electron 窗口或实体键盘验收。安装版、实体控件均单列 `NOT_RUN`。本片的读取范围不包含正文、请求问题文本、审批参数、队列消息或凭据。
