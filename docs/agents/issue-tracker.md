# Issue tracker: GitHub

本项目的工单与规格使用 [GitHub Issues](https://github.com/semantic-craft/micro-input/issues)。在仓库目录使用 `gh`，首次写入前用 `gh repo view --json nameWithOwner` 核对目标为 `semantic-craft/micro-input`。

## 操作约定

- 接手、实现或评审未完成工单前，执行[研究成果接入规则](research-context.md)；在交付说明中列出相关证据、实现影响与未验证边界。工单无需复制整份研究，引用该入口及实际使用的结论即可。
- 创建：`gh issue create --title "..." --body-file <path>`；多行正文先写文件。
- 读取：`gh issue view <number> --json number,title,body,labels,comments`。
- 列表：`gh issue list --state open --json number,title,body,labels`，按需加 `--label`。
- 子工单：`gh issue create --parent <parent> ...`；已有工单用 `gh issue edit <parent> --add-sub-issue <child>`。
- 阻塞关系：`gh issue edit <child> --add-blocked-by <blocker>`；不可用时在正文开头写 `Blocked by: #<number>`。
- 查询未关闭阻塞：`gh api repos/semantic-craft/micro-input/issues/<number> --jq .issue_dependencies_summary.blocked_by`；使用正文回退时逐项核对阻塞工单状态。
- 标签：`gh issue edit <number> --add-label "..."` 或 `--remove-label "..."`，含义见 `triage-labels.md`。
- 关闭：先用 `gh issue comment <number> --body-file <path>` 留下结果，再 `gh issue close <number>`。

技能要求“publish to the issue tracker”时创建 GitHub issue；“fetch the relevant ticket”时读取对应 issue。`.scratch/` 只留本地材料，不作为第二套工单系统。公开工单只写脱敏结论，不上传用户宏载荷、应用数据库或原始日志。

## Pull requests as a triage surface

**PRs as a request surface: no.**
