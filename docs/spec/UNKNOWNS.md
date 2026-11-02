# 待确认协议边界

证据只来自 [EVIDENCE.md](EVIDENCE.md)，裁决以 [SPEC.md](SPEC.md) 为准。下列边界均未宣称真实设备兼容；代码 TODO 由 `scripts/check-boundaries.py` 校验。

| TODO | 占位与规则 | 解锁所需证据 |
| --- | --- | --- |
| UNKNOWN-01 | `mi-proto::UnimplementedFramer` / `Framer`；R-D04。生产环境 encode/feed 都返回 Unverified，测试 Framer 仅在 `cfg(test)` 下。 | 当前固件 report ID、header 偏移、channel、长度与行边界的可靠规格。 |
| UNKNOWN-02 | `mi-session::SessionConfig`；R-D07。本地测试策略超时 2 秒、零重试，可注入；不是设备默认值。50 ms 间隔已有证据。 | 设备超时、取消、重试及迟到响应的约定。 |
| UNKNOWN-03 | `mi-proto::Identity::Unknown(Value)`、原始 `RpcResponse`，`UnverifiedSchema::identity_request`；R-D05/R-D09。 | sys.version/device.status 请求参数与响应、错误对象 schema；目前不把任何 raw response 解释为成功。 |
| UNKNOWN-04 | `mi-session::DeviceSchema` / `UnverifiedSchema`；R-D08/R-C01。真实 list/read/readbin 参数与解析全部阻断。 | 文件列表、参数名、编码、分块、校验和及错误结构。 |
| UNKNOWN-05 | `mi-transport::TransportHint`；R-D06。仅使用 hidapi 从 IOKit Transport 获取的 bus_type；无值为 Unknown，只有 USB 可打开。 | USB/BLE 双通道身份对应及去重规则；不猜 release 位。 |
| UNKNOWN-06 | `mi-model::ActiveProfile::Unknown`、`DeviceSchema::parse`；R-C05/R-N03。演示格式是独立定义，真实 schema 适配器不提供实现。 | 设备文件与缓存结构是否一致、id/index/活动 Profile、完整 token 与绑定 schema、数值语义。 |
| UNKNOWN-07 | `mi-proto::HostNotification` / `HostAction`；R-H04–R-H12。仅识别动作名称，保留 raw；host.focused_app 不在 MVP 请求 enum 中。 | kb.sa.*、kb.radial、kb.cs.* 与 host.focused_app 参数和权限行为；不执行、不上报。 |

`TestSchema` 的 `test_files`、`test_content`、`test_name`，以及测试 report 格式均为自编测试协议。不得移入生产代码。演示文件中的 `kind/ref`、macro act 数值 0/1/2、两个示例 token 的识别仅定义本项目演示格式，不证明厂商对应编码。原始 JSON 保留在 `DeviceConfig.raw`；UI 的已知模型不是对设备 schema 的声明。

下一轮应先补证，再替换对应占位并增加脱敏合成回归测试。不能以“只读方法”作为猜测未知报文的理由。硬件接入、插拔、占用和锁屏恢复仍需实际验收。
