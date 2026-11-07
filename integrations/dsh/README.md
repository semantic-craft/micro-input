# DeepSeek Harness 零补丁原生适配说明

Micro Input 直接依据 DeepSeek Harness（`com.deepseek.dsh`，`dsh-desktop`）开源代码与 macOS Accessibility 树进行适配，不再向 DSH 维护或注入任何第三方 `.patch` 补丁。

## 开源行为契约与键位对齐

1. **原生快捷键（`packages/client/ui-workspace/src/client/shortcuts.ts`）**：
   - 新建会话（`new-chat` / `session.new`）：`Cmd+N`
   - 搜索会话（`search` / `session.search`）：`Cmd+K`
   - 添加工作区/项目（`project` / `workspace.add`）：`Cmd+O`
   - 新建终端（`terminal` / `terminal.new`）：`Ctrl+\``
   - 新建浏览器（`browser` / `browser.new`）：`Cmd+T`
   - 发送消息（`submit` / `fixed.send`）：聚焦输入框后按 `Return`
2. **侧边栏项目与会话树识别（`rows/Rows.tsx` & `locales.ts`）**：
   - 项目/工作区行（`ProjectRowItem`）：`role="treeitem"`，内含 `工作区“…”的操作` / `Actions for workspace` 与 `在“…”中新建会话` / `New session in` 按钮。
   - 会话行（`SessionNodeItem`）：`role="treeitem"`，内含 `会话“…”的操作` / `Actions for session` 按钮。
   - 状态标签（`SessionStatusDots` 的 `.visuallyHidden` 文本）：
     - Working（运行中）：`进行中`、`Running`、`子智能体运行中`、`subagents running`
     - Completed（已完成未读）：`已完成`、`Completed`
     - Attention（需关注）：`等待审批`、`计划待审`、`等待回答`、`待审批`、`待回答`、`Waiting for approval`、`Plan review`、`Waiting for answer`
