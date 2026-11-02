# Micro Input

轻量、原生的 macOS Creator Micro 管理软件，采用 **Rust核心 + SwiftUI**，以 coding 工作为优先场景。产品中心是实体设备的配置与操作；Herdr、DeepSeek Harness、ZCode、QwenWork等是动作适配对象。

## 当前状态与下一步

2026-11-02：完整产品基线已经用户确认。本轮只统一规格、证据和分阶段验收，不接自动化、不操作或写入设备。

当前主分支为文档与调查结论。旧规格 [PR #7](https://github.com/semantic-craft/micro-input/pull/7) 和演示实现 [PR #8](https://github.com/semantic-craft/micro-input/pull/8) 均已关闭且未合并；后者可作选择性接续的候选，不是主分支已交付应用。当前主分支没有可运行构建命令；未来阶段1建立 `scripts/check` 与真实启动入口。

下一步按阶段1补齐真实只读查看器；可在协议补证期间独立推进阶段2离线草稿，但不提前解锁写入。既有 [MVP #1](https://github.com/semantic-craft/micro-input/issues/1) 及 #2–#6 仍属只读阶段，完成状态以GitHub为准。

## 规格入口

1. [SPEC：完整产品范围、用户故事与规则](docs/spec/SPEC.md)
2. [BUILD_PROMPT：阶段0–6构建与验收](docs/spec/BUILD_PROMPT.md)
3. [EVIDENCE：统一证据索引与冲突裁决](docs/spec/EVIDENCE.md)
4. [UNKNOWNS：未知项、占位与解锁条件](docs/spec/UNKNOWNS.md)

术语见 [GLOSSARY](GLOSSARY.md)，工程接续见 [development](docs/development.md)，约束见 [AGENTS](AGENTS.md)，经验见 [lessons](lessons.md)。

## 已有发现

[只读协议说明](docs/protocol-readonly.md) 保留已整理的静态接口行为、独立查询记录和社区版本差异；[合成向量](docs/protocol-vectors.json)用于离线验证；[参考仓库](docs/reference-repositories.md)保存来源与许可边界。它们经EVIDENCE关联进入产品规格，不直接等于当前应用或固件已验收。

独立探针曾读回固件0.6.3；这不证明真实配置读取、写入或恢复完成。历史原始材料留在本地忽略目录，不交给实现者作为厂商代码输入，不提交用户配置或回包。实现仅依据自撰行为规格与合成夹具。
