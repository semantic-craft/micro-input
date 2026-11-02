# Micro Input

- 开工读 README.md、docs/development.md 和 lessons.md；当前首轮为离线工作台；阶段范围见 docs/spec/BUILD_PROMPT.md 的 S1a。
- 接手未完成工单、实现或评审产品改动前，必读 [研究成果接入规则](docs/agents/research-context.md)，按任务选读 Input 架构、候选协议和模块契约，并在交付中记录采用的证据与验证缺口。拆分任务或交接时传递该入口。
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
