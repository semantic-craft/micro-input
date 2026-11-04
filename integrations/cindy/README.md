# Cindy 原生所有权桥

本目录保存针对 Cindy 固定源码的增量补丁。它只协调 WorkLouder 输入、听写和灯光的所有权，不使用 Local Agent API，不访问任务正文，不切换设备功能开关，不修改 keymap。

Micro Input 的生产执行路径默认暂停。只有受限 Unix socket 的当前代回复明确确认 Cindy 已停止输入、听写和灯光，并声明 `fenced-release-v1`，才允许执行。未安装桥、失联、旧代回调和未确认的停止均保持暂停。

## 协议

端点为当前用户 `~/Library/Application Support/MicroInput/cindy-ownership.sock`。目录和 socket 限当前用户访问；客户端核对 socket 类型、文件所有者及 `getpeereid`，不接收远程端点。每连接只交换一条有界、换行结尾的 JSON 请求和回复。

请求包含 `version`、随机 `requestId`、本次 Micro Input 进程的 `clientInstance`、递增 `generation`、`desiredOwner` 和 `microDrained`。目标是 `native` 或 `micro-input`。回复回显请求身份，增加 `nativeInstance`、`nativeGeneration`、`state`、三项排空结果及能力列表。

- `native` 表示 Cindy 原生持有；Micro Input 暂停重叠输入和灯光。
- `unknown` 表示不能证明所有权；Micro Input 保持暂停。
- `released` 仅在 Cindy 排空所有消费者并保持禁止原生执行的状态后发出。回复必须包含 `inputDrained`、`pttDrained`、`lightingDrained` 三项真值和 `fenced-release-v1`。

客户端以请求发出时的单调时钟计算一秒有效期，回复不能自行延长有效期。切 Profile、设备连接换代和停止握手会使在途请求失效。超时主动取消 PTT、丢弃队列并停止灯光预览。旧请求的成功或失败均不覆盖新代。

Cindy 释放后保持禁止原生执行，不能因断连或超时自行抢回。Micro Input 请求归还前，等待已经开始的宿主动作、迟到的 PTT 启动/停止、进行中的灯光启动及发送循环完成；停止未确认时不发 `microDrained=true`。退出时也执行有界归还请求，失败保留暂停状态。

## 验证边界

`scripts/verify-offline` 的 `native-ownership` 场景使用合成 HID 和明确注入的协议对端，经真实 Rust 解码及 Swift 派发检查宽键合并、旋钮、Profile 换代、失联、PTT 和灯光门禁。回执将原生应用和实体结果固定为 `NOT_RUN`。其他既有合成场景显式注入隔离所有权，不接触用户 Cindy。

Swift 测试另覆盖真实 Unix socket 交换、请求身份、代次、到期、已经开始的宿主动作排空，以及迟到灯光启动后停止失败不能签收。它们不证明安装版 Cindy 已具备这个协议。

原生补丁固定提交见 `BASE_SHA`，补丁为 `native-ownership.patch`。不得把上游源码测试或隔离 QA 回执当成日常安装版、实体按键或灯光视觉验收。

## 参考与影响

参考 `docs/agents/research-context.md` 的 IA-04、IA-10、IA-11、IA-16、IA-17，以及 `docs/four-app-interfaces.md` 中固定提交的 Cindy 原生能力。它们要求保留唯一输入/灯光所有者、区分异步代次，并禁止用旧进程存在或设备开关代替当前释放回执。相应约束落实在握手校验、统一执行门禁和排空测试中。

## 重跑双端联测

在固定提交的 Cindy checkout 应用本目录补丁并安装其锁定的 desktop workspace 依赖后，在 Micro Input 仓库运行：

```sh
scripts/verify-cindy-paired --source /absolute/path/to/patched-cindy
```

命令构建合成应用，启动有 90 秒存活上限的原生源码夹具，经真实 Unix socket 交换所有权。每个事件同时驱动 Cindy 的 `WorkLouderCodexLightingController` 和 Micro Input 的合成 HID 解码链。结束时核对两端各自记录的计数，清理进程与端点，恢复生产 ABI。输出 `evidence/result.json` 的绝对路径。已构建合成应用时可用 `--skip-build`，此时调用者负责后续恢复生产构建。

同一宽键或旋钮输入在原生持有时应得到 `(Cindy=1, Micro Input=0)`，释放后为 `(0,1)`，旧代及失联时为 `(0,0)`。验证器拒绝双执行、缺动作、事件缺失、两端计数不一致及将安装版或实体结果写成 PASS 的回执。

## 原生范围与部署限制

桥管理同一 Cindy 运行实例的全部 WorkLouder 输入与灯光，包含工作进程重建和设备重连。释放状态写入私有的所有权记录；Cindy 重启先恢复暂停，再通过当前代握手归还。进程间旧输入不能重新盖上新代标签。失联不自动解除暂停。崩溃遗留的 socket 不自动删除，因为并发清理可能产生两个所有者；该情况下桥保持 unknown。维护时必须先确认所有参与实例退出，再显式清理旧端点；不能边运行边删除端点或所有权记录。

默认宽键、旋钮、普通任务槽导航、已停止的语音及滚动有排空回执。自定义 keyboard action 与 `composer.addFiles` / `composer.addPhotos` 没有可确认的异步完成 API；执行过这些动作时维持 unknown，直到对应渲染进程重建。仍在进行的手工全局录音也不会被握手擅自结束，确认结束后才能交接。恢复时若现有 keymap 不匹配，拒绝接管，不通过重写映射修复。

部署必须替换同一个 Cindy 运行实例。不同 userData 下的旧版 Cindy 不参与本协议；不能在旧版旁边启动一个桥或测试实例，就宣称所有原生消费者已释放。本目录及联测交付源码和软件证据；本轮没有替换用户的 Cindy 或完成实体验收。

Cindy 基础源码来自 `makecindy/cindy`，固定提交见 `BASE_SHA`。本目录保留上游 Apache-2.0 `LICENSE` 和 `NOTICE`。本补丁不复制厂商 SDK 实现，也不改变其许可范围。

## 应用补丁与原生检查

在准备好的 Cindy 源码目录中切到 `BASE_SHA` 指定的提交，再执行 `git apply /absolute/path/to/micro-input/integrations/cindy/native-ownership.patch`。依赖按上游锁文件安装：

```sh
pnpm --filter desktop... install --frozen-lockfile --prefer-offline
pnpm --filter mobile... install --frozen-lockfile --prefer-offline
pnpm --filter desktop typecheck
node --test scripts/test-micro-input-ownership.mjs
```

原生控制器、工作进程、滚动和渲染进程的针对性测试随补丁进入上游 `apps/desktop/src` 的现有测试目录，使用上游 desktop Vitest 配置。双端命令会比较当前源码 diff 与本目录补丁的精确字节，拒绝在别的补丁上签收。没有对当前安装版重新打包、签名或替换的记录时，安装状态仍为未验证。

原生发送端回归：

```sh
pnpm --filter desktop exec vitest run \
  src/main/worklouder-codex/__tests__/WorkLouderCodexHostClient.test.ts \
  src/main/worklouder-codex/__tests__/WorkLouderCodexHostProcess.test.ts \
  src/main/worklouder-codex/__tests__/WorkLouderCodexLightingController.test.ts \
  src/main/worklouder-codex/__tests__/systemFrontmostInput.test.ts --maxWorkers 1
```

渲染进程回归：

```sh
pnpm --dir apps/desktop exec vitest run \
  src/renderer/features/bots/__tests__/botSessionOwners.test.ts \
  src/renderer/features/cc-agent/lib/__tests__/sessionSwitchCommands.test.ts \
  src/renderer/lib/__tests__/workLouderNavigationDrain.test.ts \
  src/renderer/lib/__tests__/workLouderCodexOwnership.test.ts \
  src/renderer/lib/__tests__/workLouderCodexVoiceGesture.test.ts \
  src/renderer/lib/__tests__/inputDeviceActions.test.ts --maxWorkers=1
```

触及文件的 ESLint 在固定上游基线已有 15 项问题，当前补丁未新增；不能将此描述为上游全量 lint 通过。原生类型检查、针对性测试及双端联测分别验收。
