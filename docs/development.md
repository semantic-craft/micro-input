# 开发与验证

## 首轮范围及完成条件

本轮直接实现 [BUILD_PROMPT](spec/BUILD_PROMPT.md) 的只读 MVP。用户已确定 clean-room、Rust + SwiftUI、UniFFI 进程内、只读设备兼容、未知项占位；无需重复产品取舍。宿主动作名称被保留，但首轮不执行。设备写入与自动化不在范围内。

使用 software-factory 的 Task/Run 术语描述这次人工触发的开发：Trigger 是用户给定规格；Task 是实现、验证、独立审查与修正；Git host 与 ticket tracker 都是 GitHub；本机完成构建，不新增 orchestrator 或自动 trigger。Run 以实现 PR 和验证证据结束，人工在 PR 审查处重新进入；未通过项留在 PR，不等待人在 Run 内操作设备。独立审查/修正最多两轮；仍不合格的项明确交回人工。一个实现分支承接本轮，避免重复执行；不创建新的 ticket source。

## 架构与规格映射

| 实现 | 规则与边界 |
| --- | --- |
| `mi-model` | R-C03/R-C05/R-N03：类型、合成格式解析、原始值和缺失引用；真实 schema UNKNOWN-06。 |
| `mi-proto` | R-D04/R-D05/R-D08/R-D09：五个只读 Method enum、ASCII JSON、通知分派、唯一生产 Framer 始终 Unverified；R-H04–R-H12 动作类型不执行。 |
| `mi-transport` | R-D01/R-D02/R-D06/R-D07/R-N02/R-N04：hidapi 2.6.7，启用 macos-shared-device 明确非独占；OS Transport 提示；自编 IOKit 回调只触发重新枚举，约 175 ms 合并；Mock 内存文件表。 |
| `mi-session` | R-C01/R-D07：串行操作、50 ms 间隔、取消、超时、连接 id、原子替换/过期快照；fs schema 由 trait 隔离。 |
| `mi-ffi` | R-A02/R-A05：UniFFI 0.29.5 高层异步 API，阻塞 HID 操作在后台；连接世代阻止取消后的结果重新发布。 |
| SwiftUI | R-A01/R-H01/R-H02/R-N01：macOS 14+、原生三栏浏览与权限引导、本地选择，无原厂资源。 |
| `scripts/build/check/run` | BUILD_PROMPT 验收 1/2/7：静态库、生成 Swift 绑定、应用 bundle、Rust/Swift 测试和 UNKNOWN 检查；QA 有存活期限。 |

UniFFI 用 proc macro 生成 Swift 绑定，应用链接 Rust 静态库；cdylib 仅供生成器读取 metadata，交付应用不动态加载它。选择它是为了保持未来可替换的类型化异步边界，避免手写 C ABI。SwiftPM 编译并打包可启动 `.app`，无 Xcode 工程依赖。生成来源见 [UniFFI 官方手册](https://mozilla.github.io/uniffi-rs/)；异步调用使用其 tokio 适配。

HID 使用 [hidapi-rs](https://github.com/ruabmbua/hidapi-rs)，复用系统 IOKit 访问和错误处理；`macos-shared-device` 禁止独占打开。额外的 `discovery.c` 仅封装 OS 发现回调和 CGPreflightListenEventAccess，不包含设备协议。应用不请求辅助功能、自动化或蓝牙权限，不读 TCC 数据库。当前 bundle 为 ad-hoc 开发签名，不包含用户凭据存储或迁移。

## 合成数据与 UNKNOWN

模型输入是本项目的合成格式，见 `crates/mi-model/fixtures/demo.json`。Basic 的两个示例 token、`kind/ref` 及宏事件的 0/1/2 映射只适用于此格式。设备配置解析必须通过 `DeviceSchema`；真实适配器只返回 Unverified。没有真实设备文件 schema 兼容完成的声明。配置 raw JSON 保留未知字段，UI 以整数/Decimal 保持大整数展示精度。活动 Profile 永远 Unknown。

每个连接持有独立 link、framer、串行队列、id 序列与失效信号。超时 2 秒是显式本地策略，零重试；999 个请求后拒绝继续复用 id。断连关闭 link、使队列等待者失败、清重组状态；新连接不使用旧 link。更多协议边界见 [UNKNOWNS](spec/UNKNOWNS.md)。

## 验收

`scripts/check` 运行完整检查；`scripts/run --verify` 启动真实交付构建。演示模式检查 Profile/Layer、键位布局、旋钮槽、Unknown、来源/时间；Swift 测试跨真实 UniFFI 加载 Rust 演示，并用 spy backend 证明导航没有设备请求。会话测试使用自编测试 framing/schema 与 MockTransport，覆盖串行/间隔、匹配/错 id、批内通知、畸形、超时、断连/重连、快照失败保留。编译失败测试阻止构造写方法。

2026-11-02 本机验收环境：macOS 27.2 (26B123)，arm64，rustc 1.98.1，Apple Swift 6.4.2。实际应用已启动并浏览两组 Profile 和不同 Layer，显示 id 42/index 1、13 键、3 旋钮槽、未知绑定及演示来源。

真实 OS 枚举显示 Creator Micro 2（303A:8298、USB），输入监控未授权；界面显示引导并禁用连接。未更改权限、未打开设备、未发 RPC。硬件插拔实时更新、占用失败、授权后的非独占打开、锁屏/解锁及真实读取均未完成实机验收；真实协议读取本轮有意阻断。测试通过不替代这些硬件证据。

接续仅从 SPEC/EVIDENCE 补证；不读取原厂代码、资源、缓存或用户宏数据来填补 UNKNOWN。保存的新测试夹具必须自编合成，不提交设备回包、用户数据或本地验收日志。
