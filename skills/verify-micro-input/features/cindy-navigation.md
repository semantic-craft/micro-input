# Cindy 既有任务目录、导航确认与原生任务动作

菜单栏的「Cindy 任务连接」默认关闭。Cindy 原生服务旁新增受限元数据与任务动作桥（`cindy-grant.json` + `cindy.sock`），仅暴露显式授权工作区（`scopes: [{ workspacePath }]`）内的既有活跃可见任务元数据（`workspacePath`、`sessionId`、`title`、`phase`、`attention`、`pinned`、`runId`、三态 `unread`、`eventWatermark`、`queuedCount`、脱敏 `pendingInteraction`），并复用原生服务提供工作区 `create`、选中目标 `send`（`started` / `steered` / `queued`）、`expectedRunId` 绑定的 `stop`、`(interactionId, interactionType)` 绑定的 `resolve`、`query` 对账及不重复待办轮次切换，不暴露消息正文、摘要或凭据，绝不放宽 Cindy 的 Local Agent API 或插件 `cindy.tasks` 隔离。

## 隔离应用检查

```sh
MI_AUTO_HOST_READY=0 scripts/verify-offline
```

总入口包含 `cindy-navigation`。单独重跑时先构建合成应用，再运行：

```sh
scripts/build --synthetic
MI_AUTO_HOST_READY=0 MI_SYNTHETIC_SCENARIO=cindy-navigation scripts/run --verify --timeout=45
```

读取启动器打印的 `QA_RUN_DIR` 下 `evidence/launcher.json`、`audit.jsonl` 与 `cindy-navigation.json`。总入口核验它们后生成 `result.json`，并确认 QA 进程、应用副本和隔离文档已清理。

场景验证 13 项可证伪断言：
- `disabled_zero_reads`：未启用时零次元数据读取；
- `scoped_catalog_only`：仅列出显式授权工作区内的既有任务，越权工作区任务不入列；
- `delivered_vs_confirmed_separated`：沿用 `cindy://session/<sessionId>` 深链投递但渲染端尚未提交目标路由时返回 `delivered`（`delivered = target`, `selected = nil`，不写入导航历史），渲染端提交目标路由并回读 `selected == target` 后才返回 `confirmed`；
- `same_name_exact_identity`：同工作区内同名任务与跨工作区同名任务按完整 `CindyTaskAddress { workspacePath, sessionId }` 精确区分；
- `selected_readback_matches`：导航确认后回读 `selected` 与目标一致；
- `wrong_selected_unconfirmed`：回读 `selected != target` 时降级为 `unconfirmed`；
- `out_of_scope_zero_reads`：未授权或越权请求零次元数据读取（`unauthorized_metadata_reads == 0`）；
- `deleted_target_no_redirect`：目标任务删除后返回 `stale`，绝不回退打开同名兄弟任务；
- `account_or_generation_change_blocks_old_selection`：账号/实例/代次换代（`instanceId` / `generation` / `ownerEpoch`）后旧选择立即失效并清空目录；
- `stop_bound_to_run_and_stale_noop`：停止操作绑定目标任务与 `expectedRunId`，非当前选中任务返回 `unconfirmed`，旧 `expectedRunId` 遇新运行轮次返回 `noop`（`guard.stopTargetChanged`）且零误停；
- `resolve_bound_and_question_guard`：审批/拒绝/计划确认绑定 `(interactionId, interactionType)`，将 `question` 当作权限批准立即阻断（`guard.questionCannotApproveAsPermission`），过期或重复请求返回 `noop` / `duplicate: true` 零副作用；
- `workspace_create_and_target_prompt_steering`：工作区新建任务回读确认 `selected`，目标 Prompt 仅作用于当前已确认选中任务并区分运行中 `steered` / `queued` 与空闲 `started`，未知传输结果通过 `query` 对账恢复不重发；
- `unread_tristate_and_pending_round`：未读状态严格区分 `true | false | null`（不从 `completed` 伪造未读），待办轮次按 `needs-interaction > unread > error` 推进且同水位不重复访问，水位更新后立即重新入轮。

## 原生源码与双端检查

原生补丁、固定上游版本和双端联测命令见 [Cindy 原生所有权与任务导航桥](../../../integrations/cindy/README.md)：

```sh
scripts/verify-cindy-paired --source /absolute/path/to/patched-cindy
```

真实 UDS 双端运行同时验证原生所有权握手与 `cindy.sock` 任务目录/深链导航确认（`paired_navigation_source = "PASS"`）。合成场景与源码联测的 `native_app` / `native_install` 与 `physical_key` 固定为 `NOT_RUN`。
