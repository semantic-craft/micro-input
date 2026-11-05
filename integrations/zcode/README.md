# ZCode 同实例导航与受控执行桥

`navigation.patch` 基于 `BASE_SHA` 中的 ZCode 3.17.2 源码。它在原桌面窗口中接入受限 UDS 桥，读取同一个 Host 的 scoped v4 索引，沿真实 tab/session store 选择目标并回读，并复用同实例 `zcodeAgent.sendConversationCommandV4` / `queryConversationCommandsV4` 执行受控的 `createSession`、`sendText` 与带 `expectedForegroundExecutionId` 的 `stop`。没有启动另一套 ZCode runtime，没有读取用户的数据库或凭据。

默认关闭。显式启用时，`MI_ZCODE_BRIDGE_CONFIG` 指向当前用户拥有、0600 的 JSON 文件，例如以下合成范围。

```json
{"scopes":[{"workspacePath":"/synthetic/workspace-a"},{"workspacePath":"/synthetic/workspace-b"}]}
```

只允许精确列出的范围，不支持通配符。远程范围须完整填写 `workspaceIdentity`、`sourceServerId` 和 `remoteSessionId`，并匹配当前已打开的工作区。桥不会自动打开新工作区或回退到本地。配置可加绝对 `directory`；默认 `~/Library/Application Support/MicroInput`，目录必须属于当前用户且为 0700。桥在其中生成新随机 capability 的 `zcode-grant.json`（0600）和 `zcode.sock`（0600）；Micro Input 只有显式连接后才读取 grant。

请求固定为版本 1，`generation` 为正安全整数。支持 `list`、`open`、`create`、`send`、`stop` 与 `query`。认证、范围、schema、代次门禁先于元数据调用。响应只投影地址、标题、liveStatus 与运行中前台 `executionId`，不包含正文、摘要、搜索片段或 interaction payload。导航在完整目标身份核对后进行；`confirmed` 要求真实 store 的选中地址与请求一致，随后只显示、恢复并聚焦所属原窗口。

受控执行契约（Slice K1）：
- **仅影响选中会话**：`send` 与 `stop` 要求请求目标与当前选中地址（`ports.selected(scope)`）完全一致，否则返回 `unconfirmed` 且不触发底层命令。
- **工作区绑定新建与新 ID 回读**：`create` 绑定授权 `scope` 调用 v4 `createSession`，将新建会话同步选中并回读确认 `selected == createdAddress` 后返回 `confirmed`。
- **运行中发送与 steering / 排队区分**：目标会话处于 `running` 时发送消息走 `sendText`（绝不误建新会话），并按 v4 `inputAccepted.delivery` 将实时 steering（`"guide"` → `"steered"`）与等待队列（`"queue"` → `"queued"`）严格分开，绝不将仅排队入队误报为实时 steering。
- **按 execution ID 停止与迟到 stop 保护**：`stop` 必须携带非空 `executionId`（映射为 `expectedForegroundExecutionId`）；若目标会话当前前台 execution 已结束或切换为新 ID，返回 `noop`（`reasonCode: "guard.stopTargetChanged"`），绝不停止新 execution。
- **命令幂等去重与未知结果对账**：相同 `(sessionId, commandId)` 幂等返回 `duplicate: true`，不重复发送或重复新建；命令超时或结果未知时，客户端保持原 `commandId` 锁存并通过 `query`（`queryConversationCommandsV4`）对账，绝不换新 `commandId` 重放。

首次成功列举固定每个 scope 的 source generation。同一个 taskService 换代、断连恢复或 renderer 替换不能继续使用旧选择；source 换代后本授权保持 stale，需要重新启用桥或重载该窗口来获得新 grant。失联、任务删除、工作区关闭或模型切换期间均拒绝导航与执行。socket 所有者使用独占目录锁，活跃或遗留锁均不自动接管。

## 应用与验证

在干净的固定源码 checkout 中运行 `git apply /absolute/path/to/integrations/zcode/navigation.patch`，然后 `pnpm install --ignore-scripts --frozen-lockfile`。源码检查为 `pnpm typecheck`、`pnpm lint` 和 `node scripts/architecture/architecture-check.mjs check --changed`。

Micro Input 仓库中的 `scripts/verify-zcode-native --source <patched-checkout>` 运行真实 UDS 的隔离合成 owner，使用生产导航与执行适配器及真实 `createTabStore`、`useZCodeSessionStore`。测试覆盖跨工作区同名且同 ID、选中回读、双会话仅影响选中者、新建会话工作区绑定与新 ID 回读、运行中 `guide` steering 与 `queue` 排队区分、迟到 `stop`（旧 `executionId`）返回 `noop` 不停新执行、重复 `commandId` 幂等去重、超时/未知结果同 ID `query` 对账、越权零元数据读取、删除与换代竞争、断连，以及 socket 所有权交错、仅确认目标所属窗口聚焦。fixture TTL 上限十分钟，脚本始终回收自己启动的进程并核对 grant/socket 清理。

`pnpm exec tsx --tsconfig packages/ui/tsconfig.json scripts/micro-input-fixture.ts --directory <private-short-path> --ttl 120` 可供 Swift 配对验收。只输出 `GRANT_PATH` 与 `CONTROL_PATH`；控制 socket 支持 `stats`、`delete`、`rotate`、`replace-source`、`disconnect`、`reset`、`delete-during-open`、`rotate-during-open`、`set-task-state`、`delay-next-command`、`settle-held-commands`。全部会话和标题均为自编夹具。

源码配对、真实 renderer store 适配器与进程间协议证据分别保留。隔离测试没有渲染完整 Electron 窗口，没有安装该 ZCode 补丁，没有实体控件验收，不能据此标记安装版或实体通过。


Swift 客户端双端验收在 Micro Input 仓库运行：

```sh
scripts/verify-zcode-paired --source /absolute/path/to/patched-zcode
```

该入口核对源码提交和补丁字节，构建合成 Micro Input，启动有时限的原生夹具，通过真实 Swift 客户端交叉打开两个工作区并验证受控 `create` / `send`（含 `guide` vs `queue`） / `stop`（含迟到 `executionId` `noop`） / `query` 幂等对账，再检查越权、删除、换代与断连拒绝。结束时清理夹具、grant 和端点，恢复生产构建但不启动应用。`--skip-build` 仅用于已构建合成 ABI 的连续验证，调用者负责后续恢复生产构建。
