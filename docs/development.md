# 开发与验证入口

Micro Input 采用「纯 Rust 内核 + 极简 C-ABI 边界 + 原生 macOS 14+ SwiftUI/AppKit 薄壳」架构。七个 Rust 模块（`mi-proto`、`mi-transport`、`mi-model`、`mi-session`、`mi-bridge`、`mi-macos`、`mi-core`）承担协议分帧、配置模型、跨进程桥接、macOS 宿主执行与工作台状态机，`crates/mi-ffi` 暴露 9 函数静态库边界，`app/MicroInput` 仅负责 SwiftUI 单色中性工作台（契约见 [DESIGN](designs/DESIGN.md)）与菜单栏投影。

## 阅读入口

- 产品范围与六层路线见[桌面应用控制](designs/four-app-control.md)及[桌面应用接口](four-app-interfaces.md)。
- 持续交付范围与工单接续规则见[持续交付入口](autonomous-delivery.md)与 [#44](https://github.com/semantic-craft/micro-input/issues/44)。
- 模块职责、C-ABI 投影边界、本地文档与恢复契约见[模块契约](module-contracts.md)。
- Input 1.1.0 静态证据索引与接入约束见[研究成果接入规则](agents/research-context.md)和[只读协议说明](protocol-readonly.md)。
- 完整阶段验收清单与未知项边界见 [SPEC](spec/SPEC.md)、[BUILD_PROMPT](spec/BUILD_PROMPT.md)、[EVIDENCE](spec/EVIDENCE.md) 与 [UNKNOWNS](spec/UNKNOWNS.md)。

## 构建与离线验证

- `scripts/verify-offline` 执行全部静态门禁、Rust/Swift 单元测试、开发包构建以及六个隔离合成 QA 场景（`host-execution`、`dsh-navigation`、`antigravity-status`、`config-write`、`lighting-control`、`diagnostic-export`），自动核对 `launcher.json` 与清理结果后保留原构建包，不连接真实硬件或启动生产应用。附加 `--qa-only` 跳过单元门禁，仍执行构建和场景复验。
- `scripts/check` 运行 Python 启动器单测、`cargo fmt --all -- --check`、`cargo test --locked --workspace --all-features`、`scripts/build`、`swift test --package-path app/MicroInput` 与 `scripts/check-boundaries.py`（含 `UNKNOWN-*` 清单对齐、生产只读边界、`R-C01`–`R-C15` 静态门禁及全部已跟踪 Markdown 断链检查）。
- `scripts/build` 编译 `mi-model`、`mi-ffi` 静态库与 SwiftUI 可执行文件，调用 `scripts/generate-app-icon.swift` 与 `scripts/verify-app-icon.swift` 生成并校验 `AppIcon.icns`，打包并签名 `target/MicroInput.app`，随后注销临时包的 LaunchServices 登记（支持 `--release`）。
- `scripts/install-app` 作为安装 `/Applications/Micro Input.app` 的唯一入口，接收 `.app` 目录或 `--zip <release-archive.zip>`，默认强制校验 `CFBundleIdentifier`、`codesign --verify --deep --strict`、`Developer ID Application:` 签名与 `spctl --assess --type execute`（安装开发/ad-hoc 包须显式传 `--allow-dev`）；同时比对现有包与来包的 `codesign -d -r -`（Designated Requirement），仅当签名要求变化时执行 `tccutil reset Accessibility craft.semantic.micro-input.dev`（除非 `--skip-tcc-reset` / `--dry-run`），避免系统设置保留旧签名假开启开关。
- `scripts/release-macos` 构建、Developer ID 签名、公证、staple 并生成 Sparkle `appcast.xml` 与 `SHA256SUMS`（公证凭据依次读取 `MI_NOTARY_PROFILE`、`NOTARY_PROFILE` 或 `macos-notary-profile`；传 `--install` 在打包后调用 `scripts/install-app` 安装到 `/Applications/Micro Input.app`，传 `--publish` 发布到 GitHub Releases）。
- `scripts/run` 启动当前 checkout 的 `target/MicroInput.app`，并持有 `<documentDir>/instance.lock` 排他锁与剥离继承环境变量，避免与 `/Applications/Micro Input.app` 双开产生双菜单栏图标。附加 `--list` / `--kill` 查看或结束受管实例；附加 `--verify` 在 `.scratch/qa-runs/<run-id>/` 创建 `0700` 隔离副本，默认持有 `qa.lock` 并在结束后删除临时应用、文档与 QA 偏好 plist，同时保留最多 20 个已完成的默认 QA 运行目录（可通过 `MI_QA_MAX_RETAINED_RUNS` 覆盖，并发 `--unique-bundle-id` 活动目录不参与清理）。
- `scripts/perf-baseline` 构建并启动隔离 QA 实例，测量冷启动至工作台首次 `onAppear` 的耗时及空闲 CPU time 与 RSS（支持 `--seconds` 调整采样窗口），输出 `MEASURED` 基线而不设定未批准预算。

## 设备观测与实机验收边界

- `scripts/observe-input` 默认只跑合成回归；附加 `--observe-once` 后才在 45 秒只读被动窗口采集一次操作者配合的物理输入，脱敏日志写入受 Git 忽略的本地观测输出目录。
- `scripts/prepare-hardware-acceptance` 仅在本地生成 `0700`/`0600` 私有实机验收准备包并校验范围与摘要，不包含设备 I/O。
- `scripts/hardware-acceptance` 默认只跑假 USB 回归；只有显式授权、明确设备路径与新鲜 H0 备份才执行单临时文件 H1 实验。
- `scripts/verify-hardware` 在真实 USB Creator Micro 2（`303A:8298`）接入时编排只读 H0 核验、可选 `--include-h1` 与 `target/MicroInput.app` 自动 USB 连通及灯光同步核对（静默旧进程但不覆盖 `/Applications/Micro Input.app`），结果写入 `.scratch/hardware-acceptance/latest-result.json`。

## 图标、菜单栏与轮盘菜单

- 应用图标原画为 `app/MicroInput/Resources/AppIcon-artwork.png`，由 `scripts/build` 生成 16–1024 px 图标集并核对 824 px 轮廓与四边 100 px 留白，保持 4×4 物理拓扑与深浅对比。
- 菜单栏由 `StatusItemIcon.swift` 绘制 20 × 18 pt `isTemplate` 键盘与旋钮符号，弹层采用原生 `NSPopover` 展示当前生效方案的 4×4 静态按键速查（旋钮左旋/右旋/按下与摇杆四向）。
- 轮盘菜单默认关闭，可在工作台侧栏或菜单栏开启，主状态由 Rust `RadialMenuState` 与 `RadialMenuRouting` 持有，全仓统一以「轮盘菜单」单源命名（`R-C10`）。启用或唤醒后须先回中，再用摇杆八向选择、旋钮按下确认、回中取消；详见[轮盘菜单验证配方](../skills/verify-micro-input/features/radial-menu.md)。

## 来源隔离与私有材料

原始设备提取物、本地备份与设备日志保留在受 Git 忽略的本地私有目录，不随源码提交或进入诊断报告。独立实现仅基于脱敏协议契约与合成向量构建，严禁复制厂商代码、未授权 SDK 或读取用户原数据库。
