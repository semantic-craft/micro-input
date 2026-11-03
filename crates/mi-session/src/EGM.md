# 串行请求会话与快照

## Overview

当前实现的代码契约；依据 [lib.rs](./lib.rs)。下列未带路径的符号均位于该文件。规划接口不视为已实现。

## Interface

- `SessionError::{Protocol,Transport,Disconnected,Timeout,Config}`；`SessionConfig`；`Snapshot { config, stale }`。
- `DeviceSchema::{identity_request,list_request,files,read_request,bytes,parse}`；UnverifiedSchema 对所有操作拒绝。
- `DeviceSession::{new,notifications,snapshot,disconnect,request,read_identity,read_config_snapshot}`。

## Invariants

- `SessionConfig::default` 为请求接收超时 2 秒、间隔 50ms；不是文档中拟定的 10 秒完整请求预算。
- `DeviceSession::new` 分配进程内 session ID，通知 broadcast 容量 32；没有常驻 reader。`exchange` 仅在请求期间读取并分发通知。
- `Queue/request` 串行请求；配置读取持同一队列锁。id 从 0 递增，不回绕，达到 999 返回需重连错误，但不主动结束会话。
- `exchange` 顺序写出帧，发送结束后才启动接收 timeout；匹配 id 完成请求，其他响应忽略。超时不显式清 framer 或断连。
- `guarded` 在明确 disconnect 时取消并清 framing；TransportError::Disconnected 使会话无效，其他错误不自动失效。
- `read_config_snapshot` 要求 keymap，先 keymap 后可选 smart，单次读取整个文件；解析且再次检查会话有效后发布。没有分块、摘要、二次 list 或读取内存上限。
- 读取失败保留旧快照并标 stale；`disconnect` 同样标 stale。生产 UnverifiedSchema 仍阻断上述真实读取流程；不能把可注入 schema 的流程当实机支持。

## Side Effects

- `exchange` 经 HidLink 写/读报告、等待节流并发出内存通知。
- `read_config_snapshot` 更新内存快照；`disconnect` 关闭 link 并唤醒等待，能取得队列锁时立即清 framer；Drop 关闭 link。

## Verification

仓库根目录运行 `cargo test --locked -p mi-session`；跨桥接入口为 `scripts/check`。本次独立只读 scout 已全文读取生产文件；未构建或运行设备验收。
