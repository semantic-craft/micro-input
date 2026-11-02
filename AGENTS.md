# Micro Input

- 开工读 README.md、docs/development.md 和 lessons.md；当前实现与运行入口见 README；构建和验证运行 `scripts/check`。
- 技术方向固定为 Rust 核心 + SwiftUI。Rust + UniFFI + SwiftUI 的构建/运行说明在 docs/development.md；不把文档校验当成应用验收。
- 兼容与协议实现依次读 docs/spec/SPEC.md、BUILD_PROMPT.md、EVIDENCE.md；只依据其行为描述，不读取原厂代码/资源或旧调研补协议。UNKNOWN 保持 trait/enum 占位并同步 UNKNOWNS.md。
- 真实设备操作保留已有映射。默认先做只读或脱敏夹具验证；涉及写入时依据当轮用户授权并验证恢复。Profile 选择在原 Input 中可能自动同步设备。
- 当前原始材料唯一位置是 .scratch/input-study，受 Git 忽略；正式实现结论进入源码或 docs，不提交用户宏载荷、应用数据库或原始日志。
- 后续在本项目维护事实；原 agent-tasks 入口只作跳转，不双写项目状态。

## Agent skills

### Issue tracker

工单与规格使用 GitHub Issues；读取或发布工单前见 [issue-tracker.md](docs/agents/issue-tracker.md)。

### Triage labels

采用五个默认分诊标签；分诊或修改标签前见 [triage-labels.md](docs/agents/triage-labels.md)。

### Domain docs

采用 single-context；探索领域模型或架构前见 [domain.md](docs/agents/domain.md)。
