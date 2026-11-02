# Micro Input

面向 Work Louder Creator Micro 2 的独立 macOS 配置查看器，采用 Rust 核心与 SwiftUI 界面。当前首版是 **clean-room 只读 MVP**，支持合成配置浏览与 USB 候选设备发现。

## 构建与运行

要求 Apple Silicon Mac、macOS 14+、Xcode Command Line Tools / Swift 5.9+、Rust stable（含 rustfmt）。依赖由 Cargo.lock 固定；首次构建需要从 crates.io 下载依赖，不加载开发凭据。

```sh
scripts/check       # Rust 格式、构建与全部测试，UniFFI 生成，Swift 构建与测试，边界检查
scripts/run         # 启动当前 checkout 的 target/MicroInput.app，Ctrl-C 退出
scripts/run --verify # 同一应用，验收实例最多存活十分钟
```

已实测工具链与验收限制见 [开发说明](docs/development.md)。`scripts/build` 单独构建应用，生成桥接代码不入库。

## 当前可用范围

- “载入演示数据”走 Rust 解析 → UniFFI → SwiftUI，可浏览 Profile、Layer、13 键、旋钮 3 槽、摇杆扇区、灯光原始值和绑定详情；明确标注演示来源与读取时间。
- Profile id 与 index 分开显示，未知 token/字段保留，缺失引用提示；选择 Profile/Layer 只改变页面状态。
- 通过 OS 身份发现 VID 303A、PID 8297/8298、usage page FF00 的候选；检查输入监控权限并提供系统设置入口。
- 真实 HID 非独占打开的代码已接入；真实身份/文件读取由未知协议占位阻断，不发送 RPC、不回退演示结果。输入监控未授权时禁用连接。
- 无配置写入、Profile 激活、灯光预览、宿主动作执行、固件更新或自动化接线。

## 接续入口

实现前按顺序读 [SPEC](docs/spec/SPEC.md) → [BUILD_PROMPT](docs/spec/BUILD_PROMPT.md) → [EVIDENCE](docs/spec/EVIDENCE.md)。能力和宿主/设备边界以 SPEC 为准；验收以 BUILD_PROMPT 为准；未知项见 [UNKNOWNS](docs/spec/UNKNOWNS.md)。旧调研不作为本轮实现输入。

下一步先审查本轮实现 PR，再为 UNKNOWN 边界补可靠证据及硬件验收；当前不启用自动触发。工程说明见 [development](docs/development.md)，约束见 [AGENTS](AGENTS.md)，经验见 [lessons](lessons.md)。任务跟踪在 [GitHub Issues](https://github.com/semantic-craft/micro-input/issues)，首版父工单为 [#1](https://github.com/semantic-craft/micro-input/issues/1)。
