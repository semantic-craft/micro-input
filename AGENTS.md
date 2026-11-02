# Micro Input

- 开工读 README.md、docs/development.md 和 lessons.md；当前是开发前项目，尚无可运行实现。
- 技术方向固定为 Rust 核心 + SwiftUI。初始化工程时补真实构建/运行说明和 scripts/check；不把文档校验当成应用验收。
- 兼容功能或协议工作查 README 中对应调研原件，分别标明观察、官方说明、社区线索和未知；社区旧版参数不能充当当前固件规格。
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
